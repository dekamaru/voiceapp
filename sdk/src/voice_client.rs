use std::collections::HashMap;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpStream, UdpSocket};
use tokio::sync::{mpsc, broadcast, RwLock};
use tracing::{debug, error, info};

use voiceapp_protocol::{PacketId, ParticipantInfo};
use crate::voice_encoder::{VoiceEncoder, OPUS_FRAME_SAMPLES};
use crate::voice_decoder::VoiceDecoder;

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
    voice_output_rx: broadcast::Receiver<Vec<f32>>,
    udp_send_tx: mpsc::UnboundedSender<Vec<u8>>,
    udp_recv_rx: mpsc::UnboundedReceiver<(PacketId, Vec<u8>)>,
}

impl VoiceClient {
    pub async fn connect(management_server_addr: &str, voice_server_addr: &str) -> Result<Self, VoiceClientError> {
        // Create TCP channels
        let (request_tx, request_rx) = mpsc::unbounded_channel();
        let (response_tx, response_rx) = mpsc::unbounded_channel();
        let (event_tx, _) = broadcast::channel(100);
        let event_rx = event_tx.subscribe();
        let (voice_input_tx, voice_input_rx) = mpsc::unbounded_channel();
        let (voice_output_tx, voice_output_rx) = broadcast::channel(100);

        // Create UDP channels
        let (udp_send_tx, udp_send_rx) = mpsc::unbounded_channel();
        let (udp_recv_tx, udp_recv_rx) = mpsc::unbounded_channel();

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
            voice_output_rx,
            udp_send_tx: udp_send_tx.clone(),
            udp_recv_rx,
        };

        // Connect TCP socket
        let tcp_socket = TcpStream::connect(management_server_addr)
            .await
            .map_err(|e| VoiceClientError::ConnectionFailed(e.to_string()))?;

        info!("[Management] Connected to {}", management_server_addr);

        // Create UDP socket (bind to any available port)
        let udp_socket = UdpSocket::bind("0.0.0.0:0")
            .await
            .map_err(|e| VoiceClientError::ConnectionFailed(format!("UDP bind failed: {}", e)))?;

        // Connect UDP socket to server address
        udp_socket.connect(voice_server_addr)
            .await
            .map_err(|e| VoiceClientError::ConnectionFailed(format!("UDP connect failed: {}", e)))?;

        info!("[Voice] Connected to {}", voice_server_addr);

        // Spawn background tasks
        client.spawn_tcp_handler(tcp_socket, request_rx, response_tx, event_tx);
        client.spawn_udp_handler(udp_socket, udp_send_rx, udp_recv_tx, voice_output_tx);
        client.spawn_event_processor();
        client.spawn_voice_transmitter(voice_input_rx, udp_send_tx.clone());

        Ok(client)
    }

    pub async fn authenticate(&mut self, username: &str) -> Result<(), VoiceClientError> {
        debug!("Authenticating as '{}'", username);

        // TCP authentication
        self.request_tx.send(voiceapp_protocol::encode_login_request(username)).map_err(|_| VoiceClientError::Disconnected)?;
        let response = self.wait_for_response_with(PacketId::LoginResponse, 5, voiceapp_protocol::decode_login_response).await?;

        // Update client state with user_id and voice_token
        let mut state = self.state.write().await;
        state.user_id = Some(response.id);
        state.voice_token = Some(response.voice_token);
        state.participants = response
            .participants
            .into_iter()
            .map(|p| (p.user_id, p))
            .collect();
        let voice_token = response.voice_token;
        drop(state); // Release lock

        info!("[Management] Authenticated, user_id={}", response.id);

        // UDP authentication (with retry logic: 3 attempts, 5s timeout each)
        let max_retries = 3;
        for attempt in 1..=max_retries {
            debug!("[Voice] Sending auth request (attempt {}/{})", attempt, max_retries);

            // Send VoiceAuthRequest
            let auth_request = voiceapp_protocol::encode_voice_auth_request(voice_token);
            self.udp_send_tx.send(auth_request).map_err(|_| VoiceClientError::Disconnected)?;

            // Wait for VoiceAuthResponse with 5s timeout
            let timeout_result = tokio::time::timeout(
                std::time::Duration::from_secs(5),
                async {
                    loop {
                        match self.udp_recv_rx.recv().await {
                            Some((packet_id, payload)) => {
                                if packet_id == PacketId::VoiceAuthResponse {
                                    return match voiceapp_protocol::decode_voice_auth_response(&payload) {
                                        Ok(true) => Ok(()),
                                        Ok(false) => Err(VoiceClientError::ConnectionFailed("Voice auth denied".to_string())),
                                        Err(e) => Err(VoiceClientError::ConnectionFailed(e.to_string())),
                                    }
                                }
                                // Ignore other packets, keep waiting for auth response
                            }
                            None => return Err(VoiceClientError::Disconnected),
                        }
                    }
                }
            ).await;

            match timeout_result {
                Ok(Ok(())) => {
                    info!("[Voice] Authenticated successfully");
                    return Ok(());
                }
                Ok(Err(e)) => return Err(e),
                Err(_) => {
                    debug!("[Voice] Auth response timeout on attempt {}", attempt);
                    if attempt < max_retries {
                        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                    }
                }
            }
        }

        Err(VoiceClientError::Timeout("Voice authentication failed after 3 attempts".to_string()))
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

    /// Get a receiver for decoded voice output (mono F32 PCM samples at 48kHz)
    /// Subscribe to incoming voice from other participants
    pub fn voice_output_receiver(&self) -> broadcast::Receiver<Vec<f32>> {
        self.voice_output_rx.resubscribe()
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

    /// Spawn voice transmitter task that encodes audio frames and sends them over UDP
    fn spawn_voice_transmitter(&self, voice_input_rx: mpsc::UnboundedReceiver<Vec<f32>>, udp_send_tx: mpsc::UnboundedSender<Vec<u8>>) {
        tokio::spawn(async move {
            let mut rx = voice_input_rx;
            let mut codec = match VoiceEncoder::new() {
                Ok(c) => c,
                Err(e) => {
                    error!("Failed to create voice codec: {}", e);
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

                    match codec.encode(&frame) {
                        Ok(Some(packet)) => {
                            let encoded = voiceapp_protocol::encode_voice_data(&packet);
                            if let Err(e) = udp_send_tx.send(encoded) {
                                error!("Failed to send voice data over UDP: {}", e);
                                return;
                            }
                        }
                        Ok(None) => {
                            error!("Encoder returned None for full frame");
                            return;
                        }
                        Err(e) => {
                            error!("Failed to encode voice frame: {}", e);
                            return;
                        }
                    }
                }
            }

            // Encode and send any remaining samples
            if !sample_buffer.is_empty() {
                debug!("Encoding {} remaining samples", sample_buffer.len());
                match codec.encode(&sample_buffer) {
                    Ok(Some(packet)) => {
                        let encoded = voiceapp_protocol::encode_voice_data(&packet);
                        if let Err(e) = udp_send_tx.send(encoded) {
                            error!("Failed to send final voice data over UDP: {}", e);
                        }
                    }
                    Ok(None) => {
                        debug!("No final frame to send");
                    }
                    Err(e) => {
                        error!("Failed to encode remaining samples: {}", e);
                    }
                }
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

    /// Spawn UDP handler task (called internally from connect)
    fn spawn_udp_handler(
        &self,
        socket: UdpSocket,
        send_rx: mpsc::UnboundedReceiver<Vec<u8>>,
        recv_tx: mpsc::UnboundedSender<(PacketId, Vec<u8>)>,
        voice_output_tx: broadcast::Sender<Vec<f32>>,
    ) {
        tokio::spawn(async move {
            if let Err(e) = udp_handler(socket, send_rx, recv_tx, voice_output_tx).await {
                error!("UDP handler error: {}", e);
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

/// UDP handler task for voice data and auth
async fn udp_handler(
    socket: UdpSocket,
    mut send_rx: mpsc::UnboundedReceiver<Vec<u8>>,
    recv_tx: mpsc::UnboundedSender<(PacketId, Vec<u8>)>,
    voice_output_tx: broadcast::Sender<Vec<f32>>,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut read_buf = [0u8; 4096];
    let decoder = VoiceDecoder::new(voice_output_tx)?;

    loop {
        tokio::select! {
            // Handle outgoing packets
            Some(packet) = send_rx.recv() => {
                socket.send(&packet).await?;
                // TODO: packet.len() for bytes sent
            }

            // Handle incoming packets
            Ok(n) = socket.recv(&mut read_buf) => {
                if n == 0 {
                    debug!("UDP socket closed");
                    break;
                }

                // Parse incoming packet
                let (packet_id, payload) = voiceapp_protocol::parse_packet(&read_buf[..n])?;

                // Decode voice data packets
                if packet_id == PacketId::VoiceData {
                    if let Ok(voice_packet) = voiceapp_protocol::decode_voice_data(&payload) {
                        // Insert packet into NetEQ for jitter buffering and reordering
                        // Timer loop in VoiceDecoder automatically pulls audio every 10ms
                        if let Err(e) = decoder.insert_packet(voice_packet).await {
                            debug!("Failed to insert voice packet: {}", e);
                        }
                    }
                } else {
                    // Route non-voice packets to receiver
                    let _ = recv_tx.send((packet_id, payload.to_vec()));
                }
            }
        }
    }

    Ok(())
}