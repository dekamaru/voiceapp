use std::collections::HashMap;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::{mpsc, broadcast, RwLock};
use tracing::{debug, error, info};

use voiceapp_protocol::{PacketId, ParticipantInfo};
use crate::voice_encoder::{VoiceEncoder, OPUS_FRAME_SAMPLES};

/// Errors that can occur with VoiceClient
#[derive(Debug, Clone)]
pub enum VoiceClientError {
    ConnectionFailed(String),
    Disconnected,
    Timeout(String),
    SystemError(String),
}

impl std::fmt::Display for VoiceClientError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            VoiceClientError::ConnectionFailed(msg) => write!(f, "Connection failed: {}", msg),
            VoiceClientError::Disconnected => write!(f, "Disconnected from server"),
            VoiceClientError::Timeout(msg) => write!(f, "Timeout exceeded {}", msg),
            VoiceClientError::SystemError(msg) => write!(f, "System error: {}", msg),
        }
    }
}

impl std::error::Error for VoiceClientError {}

/// Client-side state for voice connection
struct ClientState {
    user_id: Option<u64>,
    voice_token: Option<u64>,
    participants: HashMap<u64, ParticipantInfo>,
    in_voice_channel: bool,
}

/// The VoiceClient for managing voice connections
pub struct VoiceClient {
    state: Arc<RwLock<ClientState>>,
    request_tx: mpsc::UnboundedSender<Vec<u8>>,
    response_rx: mpsc::UnboundedReceiver<(PacketId, Vec<u8>)>,
    event_rx: broadcast::Receiver<(PacketId, Vec<u8>)>,
    voice_input_tx: mpsc::UnboundedSender<Vec<f32>>,
}

impl VoiceClient {
    pub async fn connect(server_addr: &str) -> Result<Self, VoiceClientError> {
        // Create channels
        let (request_tx, request_rx) = mpsc::unbounded_channel();
        let (response_tx, response_rx) = mpsc::unbounded_channel();
        let (event_tx, _) = broadcast::channel(100);
        let event_rx = event_tx.subscribe();
        let (voice_input_tx, voice_input_rx) = mpsc::unbounded_channel();

        // Create client with empty state
        let client = VoiceClient {
            state: Arc::new(RwLock::new(ClientState {
                user_id: None,
                voice_token: None,
                participants: HashMap::new(),
                in_voice_channel: false,
            })),
            request_tx: request_tx.clone(),
            response_rx,
            event_rx,
            voice_input_tx,
        };

        // Connect to server
        let tcp_socket = TcpStream::connect(server_addr)
            .await
            .map_err(|e| VoiceClientError::ConnectionFailed(e.to_string()))?;

        info!("Connected to {}", server_addr);

        // Spawn background tasks
        client.spawn_tcp_handler(tcp_socket, request_rx, response_tx, event_tx);
        client.spawn_event_processor();
        client.spawn_voice_transmitter(voice_input_rx);

        Ok(client)
    }

    pub async fn authenticate(&mut self, username: &str) -> Result<(), VoiceClientError> {
        debug!("Authenticating as '{}'", username);

        self.request_tx.send(voiceapp_protocol::encode_login_request(username)).map_err(|_| VoiceClientError::Disconnected)?;
        let response = self.wait_for_response_with(PacketId::LoginResponse, 5, voiceapp_protocol::decode_login_response).await?;

        // Update client state
        let mut state = self.state.write().await;
        state.user_id = Some(response.id);
        state.voice_token = Some(response.voice_token);
        state.participants = response
            .participants
            .into_iter()
            .map(|p| (p.user_id, p))
            .collect();

        info!("Authenticated, user_id={}", response.id);
        Ok(())
    }

    pub async fn join_channel(&mut self) -> Result<(), VoiceClientError> {
        debug!("Joining voice channel");

        // Encode and send JoinVoiceChannelRequest
        let payload = voiceapp_protocol::encode_join_voice_channel_request();

        self.request_tx
            .send(payload)
            .map_err(|_| VoiceClientError::Disconnected)?;

        // Wait for response
        let _success = self
            .wait_for_response_with(
                PacketId::JoinVoiceChannelResponse,
                5,
                voiceapp_protocol::decode_join_voice_channel_response,
            )
            .await?;

        // Update client state
        let mut state = self.state.write().await;
        state.in_voice_channel = true;

        info!("Joined voice channel");
        Ok(())
    }

    pub async fn leave_channel(&mut self) -> Result<(), VoiceClientError> {
        debug!("Leaving voice channel");

        // Encode and send LeaveVoiceChannelRequest
        let payload = voiceapp_protocol::encode_leave_voice_channel_request();

        self.request_tx
            .send(payload)
            .map_err(|_| VoiceClientError::Disconnected)?;

        // Wait for response
        let _success = self
            .wait_for_response_with(
                PacketId::LeaveVoiceChannelResponse,
                5,
                voiceapp_protocol::decode_leave_voice_channel_response,
            )
            .await?;

        // Update client state
        let mut state = self.state.write().await;
        state.in_voice_channel = false;

        info!("Left voice channel");
        Ok(())
    }

    pub async fn get_user_id(&self) -> Result<Option<u64>, VoiceClientError> {
        let state = self.state.read().await;
        Ok(state.user_id)
    }

    /// Get a sender for raw voice samples (Vec<f32>)
    /// Returns a cloneable sender that can be moved into async tasks
    pub fn voice_input_sender(&self) -> mpsc::UnboundedSender<Vec<f32>> {
        self.voice_input_tx.clone()
    }

    /// Spawn event processor task (called internally from connect)
    fn spawn_event_processor(&self) {
        let state = Arc::clone(&self.state);
        let mut event_rx = self.event_rx.resubscribe();

        tokio::spawn(async move {
            loop {
                match event_rx.recv().await {
                    Ok((packet_id, payload)) => {
                        if let Err(e) = Self::handle_event(&state, packet_id, payload).await {
                            error!("Event handling error: {}", e);
                            break;
                        }
                    }
                    Err(broadcast::error::RecvError::Closed) => {
                        debug!("Event stream closed");
                        break;
                    }
                    Err(broadcast::error::RecvError::Lagged(_)) => {
                        debug!("Event stream lagged");
                    }
                }
            }
        });
    }

    /// Spawn voice transmitter task that encodes audio frames
    fn spawn_voice_transmitter(&self, voice_input_rx: mpsc::UnboundedReceiver<Vec<f32>>) {
        tokio::spawn(async move {
            let mut rx = voice_input_rx;
            let mut encoder = match VoiceEncoder::new() {
                Ok(enc) => enc,
                Err(e) => {
                    error!("Failed to create voice encoder: {}", e);
                    return;
                }
            };

            // Buffer to accumulate samples until we have OPUS_FRAME_SAMPLES
            let mut sample_buffer = Vec::with_capacity(OPUS_FRAME_SAMPLES * 2);

            while let Some(samples) = rx.recv().await {
                // Add received samples to buffer
                sample_buffer.extend_from_slice(&samples);

                // Encode complete frames
                while sample_buffer.len() >= OPUS_FRAME_SAMPLES {
                    let frame: Vec<f32> = sample_buffer.drain(0..OPUS_FRAME_SAMPLES).collect();
                    // Encode frame (ignore result for now)
                    let _ = encoder.encode_frame(&frame);
                    debug!("Encoded frame with {} samples", frame.len());
                }
            }

            // Encode any remaining samples (less than 960)
            if !sample_buffer.is_empty() {
                debug!("Encoding final frame with {} samples", sample_buffer.len());
                let _ = encoder.encode_frame(&sample_buffer);
            }

            debug!("Voice transmitter task ended");
        });
    }

    /// Spawn TCP handler task (called internally from connect)
    fn spawn_tcp_handler(
        &self,
        socket: TcpStream,
        request_rx: mpsc::UnboundedReceiver<Vec<u8>>,
        response_tx: mpsc::UnboundedSender<(PacketId, Vec<u8>)>,
        event_tx: broadcast::Sender<(PacketId, Vec<u8>)>,
    ) {
        tokio::spawn(async move {
            if let Err(e) = tcp_handler(socket, request_rx, response_tx, event_tx).await {
                error!("TCP handler error: {}", e);
            }
        });
    }

    /// Handle a single event and update state
    async fn handle_event(
        state: &Arc<RwLock<ClientState>>,
        packet_id: PacketId,
        payload: Vec<u8>,
    ) -> Result<(), VoiceClientError> {
        match packet_id {
            PacketId::UserJoinedServer => {
                let (user_id, username) = voiceapp_protocol::decode_user_joined_server(&payload)
                    .map_err(|e| VoiceClientError::ConnectionFailed(e.to_string()))?;

                let mut s = state.write().await;
                s.participants.insert(
                    user_id,
                    ParticipantInfo {
                        user_id,
                        in_voice: false,
                    },
                );

                debug!("User joined server: id={}, username={}", user_id, username);
                Ok(())
            }
            PacketId::UserLeftServer => {
                let user_id = voiceapp_protocol::decode_user_left_server(&payload)
                    .map_err(|e| VoiceClientError::ConnectionFailed(e.to_string()))?;

                let mut s = state.write().await;
                s.participants.remove(&user_id);

                debug!("User left server: id={}", user_id);
                Ok(())
            }
            PacketId::UserJoinedVoice => {
                let user_id = voiceapp_protocol::decode_user_joined_voice(&payload)
                    .map_err(|e| VoiceClientError::ConnectionFailed(e.to_string()))?;

                let mut s = state.write().await;
                if let Some(participant) = s.participants.get_mut(&user_id) {
                    participant.in_voice = true;
                }

                debug!("User joined voice: id={}", user_id);
                Ok(())
            }
            PacketId::UserLeftVoice => {
                let user_id = voiceapp_protocol::decode_user_left_voice(&payload)
                    .map_err(|e| VoiceClientError::ConnectionFailed(e.to_string()))?;

                let mut s = state.write().await;
                if let Some(participant) = s.participants.get_mut(&user_id) {
                    participant.in_voice = false;
                }

                debug!("User left voice: id={}", user_id);
                Ok(())
            }
            _ => {
                debug!("Ignoring unexpected event: {:?}", packet_id);
                Ok(())
            }
        }
    }

    /// Wait for a response packet, decode it, and handle timeout with custom timeout
    async fn wait_for_response_with<T, F>(
        &mut self,
        expected_id: PacketId,
        timeout_secs: u64,
        decoder: F,
    ) -> Result<T, VoiceClientError>
    where
        F: Fn(&[u8]) -> std::io::Result<T>,
    {
        let timeout = tokio::time::timeout(
            std::time::Duration::from_secs(timeout_secs),
            async {
                while let Some((packet_id, payload)) = self.response_rx.recv().await {
                    if packet_id == expected_id {
                        return Ok(payload);
                    }
                }
                Err(VoiceClientError::Disconnected)
            },
        );

        match timeout.await {
            Ok(Ok(payload)) => decoder(&payload)
                .map_err(|e| VoiceClientError::ConnectionFailed(e.to_string())),
            Ok(Err(e)) => Err(e),
            Err(_) => Err(VoiceClientError::Timeout(format!(
                "packet {}",
                expected_id.as_u8()
            ))),
        }
    }
}

/// TCP handler task that multiplexes between requests and server responses
async fn tcp_handler(
    mut socket: TcpStream,
    mut request_rx: mpsc::UnboundedReceiver<Vec<u8>>,
    response_tx: mpsc::UnboundedSender<(PacketId, Vec<u8>)>,
    event_tx: broadcast::Sender<(PacketId, Vec<u8>)>,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut read_buf = [0u8; 4096];

    loop {
        tokio::select! {
            // Handle outgoing requests (already fully encoded)
            Some(packet) = request_rx.recv() => { socket.write_all(&packet).await? }

            // Handle incoming responses/events
            n = socket.read(&mut read_buf) => {
                let n = n?;
                if n == 0 {
                    return Err("Connection closed by server".into());
                }

                // Parse incoming packet
                let (packet_id, payload) = voiceapp_protocol::parse_packet(&read_buf[..n])?;
                debug!("Received packet: {:?}", packet_id);

                // Route to appropriate channel
                match packet_id {
                    // Responses (0x11-0x14)
                    PacketId::LoginResponse
                    | PacketId::VoiceAuthResponse
                    | PacketId::JoinVoiceChannelResponse
                    | PacketId::LeaveVoiceChannelResponse => {
                        response_tx.send((packet_id, payload.to_vec()))?;
                    }
                    // Events (0x21-0x24)
                    PacketId::UserJoinedServer
                    | PacketId::UserJoinedVoice
                    | PacketId::UserLeftVoice
                    | PacketId::UserLeftServer => {
                        let _ = event_tx.send((packet_id, payload.to_vec()));
                    }
                    // Unexpected packets
                    _ => {
                        debug!("Ignoring unexpected packet: {:?}", packet_id);
                    }
                }
            }
        }
    }
}