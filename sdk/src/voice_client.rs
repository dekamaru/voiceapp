use async_channel::{unbounded, Receiver, Sender};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpStream, UdpSocket};
use tokio::sync::RwLock;
use tracing::{debug, error, info};

use crate::voice_decoder::VoiceDecoder;
use voiceapp_protocol::{PacketId, ParticipantInfo};

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

/// Events emitted by VoiceClient when state changes occur
#[derive(Debug, Clone)]
pub enum VoiceClientEvent {
    /// Initial participant list sent after successful connection
    ParticipantsList {
        user_id: u64,
        participants: Vec<ParticipantInfo>,
    },
    /// A user joined the server
    UserJoinedServer { user_id: u64, username: String },
    /// A user joined a voice channel
    UserJoinedVoice { user_id: u64 },
    /// A user left a voice channel
    UserLeftVoice { user_id: u64 },
    /// A user left the server
    UserLeftServer { user_id: u64 },
    /// A user sent a chat message
    UserSentMessage {
        user_id: u64,
        timestamp: u64,
        message: String,
    },
}

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
    request_tx: Sender<Vec<u8>>,
    request_rx: Receiver<Vec<u8>>,
    response_tx: Sender<(PacketId, Vec<u8>)>,
    response_rx: Receiver<(PacketId, Vec<u8>)>,
    event_tx: Sender<(PacketId, Vec<u8>)>,
    event_rx: Receiver<(PacketId, Vec<u8>)>,
    decoder: Arc<VoiceDecoder>,
    udp_send_tx: Sender<Vec<u8>>,
    udp_send_rx: Receiver<Vec<u8>>,
    udp_recv_tx: Sender<(PacketId, Vec<u8>)>,
    udp_recv_rx: Receiver<(PacketId, Vec<u8>)>,
    client_events_tx: Sender<VoiceClientEvent>,
    client_events_rx: Receiver<VoiceClientEvent>,
}

impl VoiceClient {
    /// Create a new VoiceClient with all channels initialized
    pub fn new() -> Result<Self, VoiceClientError> {
        // Create TCP channels
        let (request_tx, request_rx) = unbounded();
        let (response_tx, response_rx) = unbounded();
        let (event_tx, event_rx) = unbounded();

        // Create decoder
        let decoder = Arc::new(
            VoiceDecoder::new().map_err(|e| VoiceClientError::SystemError(e.to_string()))?,
        );

        // Create UDP channels
        let (udp_send_tx, udp_send_rx) = unbounded();
        let (udp_recv_tx, udp_recv_rx) = unbounded();

        // Create client event channels
        let (client_events_tx, client_events_rx) = unbounded();

        // Create client with empty state
        Ok(VoiceClient {
            state: Arc::new(RwLock::new(ClientState {
                user_id: None,
                voice_token: None,
                participants: HashMap::new(),
                in_voice_channel: false,
            })),
            request_tx,
            request_rx,
            response_tx,
            response_rx,
            event_tx,
            event_rx,
            decoder,
            udp_send_tx,
            udp_send_rx,
            udp_recv_tx,
            udp_recv_rx,
            client_events_tx,
            client_events_rx,
        })
    }

    /// Subscribe to the event stream from VoiceClient
    /// Returns a cloneable receiver that will receive all events from this point forward
    pub fn event_stream(&self) -> Receiver<VoiceClientEvent> {
        self.client_events_rx.clone()
    }

    pub async fn connect(
        &mut self,
        management_server_addr: &str,
        voice_server_addr: &str,
        username: &str,
    ) -> Result<(), VoiceClientError> {
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
        udp_socket.connect(voice_server_addr).await.map_err(|e| {
            VoiceClientError::ConnectionFailed(format!("UDP connect failed: {}", e))
        })?;

        info!("[Voice] Connected to {}", voice_server_addr);

        // Spawn background tasks
        self.spawn_tcp_handler(
            tcp_socket,
            self.request_rx.clone(),
            self.response_tx.clone(),
            self.event_tx.clone(),
        );
        self.spawn_udp_handler(
            udp_socket,
            self.udp_send_rx.clone(),
            self.udp_recv_tx.clone(),
            self.decoder.clone(),
        );
        self.spawn_event_processor();

        // Authenticate
        debug!("Authenticating as '{}'", username);

        // TCP authentication
        self.request_tx
            .send(voiceapp_protocol::encode_login_request(username))
            .await
            .map_err(|_| VoiceClientError::Disconnected)?;
        let response = self
            .wait_for_response_with(
                PacketId::LoginResponse,
                5,
                voiceapp_protocol::decode_login_response,
            )
            .await?;

        // Update client state with user_id and voice_token
        let mut state = self.state.write().await;
        state.user_id = Some(response.id);
        state.voice_token = Some(response.voice_token);
        state.participants = response
            .participants
            .iter()
            .map(|p| (p.user_id, p.clone()))
            .collect();
        let voice_token = response.voice_token;
        let participants_list = response.participants.clone();
        drop(state); // Release lock

        // Send initial participants list event
        let _ = self
            .client_events_tx
            .send(VoiceClientEvent::ParticipantsList {
                user_id: response.id,
                participants: participants_list,
            })
            .await;

        info!("[Management] Authenticated, user_id={}", response.id);

        // UDP authentication (with retry logic: 3 attempts, 5s timeout each)
        let max_retries = 3;
        for attempt in 1..=max_retries {
            debug!(
                "[Voice] Sending auth request (attempt {}/{})",
                attempt, max_retries
            );

            // Send VoiceAuthRequest
            let auth_request = voiceapp_protocol::encode_voice_auth_request(voice_token);
            self.udp_send_tx
                .send(auth_request)
                .await
                .map_err(|_| VoiceClientError::Disconnected)?;

            // Wait for VoiceAuthResponse with 5s timeout
            let timeout_result = tokio::time::timeout(std::time::Duration::from_secs(5), async {
                loop {
                    match self.udp_recv_rx.recv().await {
                        Ok((packet_id, payload)) => {
                            if packet_id == PacketId::VoiceAuthResponse {
                                return match voiceapp_protocol::decode_voice_auth_response(&payload)
                                {
                                    Ok(true) => Ok(()),
                                    Ok(false) => Err(VoiceClientError::ConnectionFailed(
                                        "Voice auth denied".to_string(),
                                    )),
                                    Err(e) => {
                                        Err(VoiceClientError::ConnectionFailed(e.to_string()))
                                    }
                                };
                            }
                            // Ignore other packets, keep waiting for auth response
                        }
                        Err(_) => return Err(VoiceClientError::Disconnected),
                    }
                }
            })
            .await;

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

        Err(VoiceClientError::Timeout(
            "Voice authentication failed after 3 attempts".to_string(),
        ))
    }

    pub async fn join_channel(&mut self) -> Result<(), VoiceClientError> {
        debug!("Joining voice channel");

        // Encode and send JoinVoiceChannelRequest
        let payload = voiceapp_protocol::encode_join_voice_channel_request();

        self.request_tx
            .send(payload)
            .await
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
            .await
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

    pub async fn send_message(&mut self, message: &str) -> Result<(), VoiceClientError> {
        debug!("Sending chat message");

        // Encode and send ChatMessageRequest
        let payload = voiceapp_protocol::encode_chat_message_request(message);

        self.request_tx
            .send(payload)
            .await
            .map_err(|_| VoiceClientError::Disconnected)?;

        // Wait for response
        let _success = self
            .wait_for_response_with(
                PacketId::ChatMessageResponse,
                5,
                voiceapp_protocol::decode_chat_message_response,
            )
            .await?;

        debug!("Chat message sent successfully");
        Ok(())
    }

    /// Get a receiver for decoded voice output (mono F32 PCM samples at 48kHz)
    /// Subscribe to incoming voice from other participants
    pub fn get_decoder(&self) -> Arc<VoiceDecoder> {
        self.decoder.clone()
    }

    /// Get UDP send channel for AudioManager to forward encoded voice packets
    pub fn get_udp_send_tx(&self) -> Sender<Vec<u8>> {
        self.udp_send_tx.clone()
    }

    /// Spawn event processor task (called internally from connect)
    fn spawn_event_processor(&self) {
        let state = Arc::clone(&self.state);
        let event_rx = self.event_rx.clone();
        let client_events_tx = self.client_events_tx.clone();

        tokio::spawn(async move {
            loop {
                match event_rx.recv().await {
                    Ok((packet_id, payload)) => {
                        if let Err(e) =
                            Self::handle_event(&state, packet_id, payload, &client_events_tx).await
                        {
                            error!("Event handling error: {}", e);
                            break;
                        }
                    }
                    Err(_) => {
                        debug!("Event stream closed");
                        break;
                    }
                }
            }
        });
    }

    /// Spawn TCP handler task (called internally from connect)
    fn spawn_tcp_handler(
        &self,
        socket: TcpStream,
        request_rx: Receiver<Vec<u8>>,
        response_tx: Sender<(PacketId, Vec<u8>)>,
        event_tx: Sender<(PacketId, Vec<u8>)>,
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
        send_rx: Receiver<Vec<u8>>,
        recv_tx: Sender<(PacketId, Vec<u8>)>,
        decoder: Arc<VoiceDecoder>,
    ) {
        tokio::spawn(async move {
            if let Err(e) = udp_handler(socket, send_rx, recv_tx, decoder).await {
                error!("UDP handler error: {}", e);
            }
        });
    }

    /// Handle a single event and update state
    async fn handle_event(
        state: &Arc<RwLock<ClientState>>,
        packet_id: PacketId,
        payload: Vec<u8>,
        client_events_tx: &Sender<VoiceClientEvent>,
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
                        username: username.clone(),
                        in_voice: false,
                    },
                );
                drop(s); // Release lock before sending event

                let _ = client_events_tx
                    .send(VoiceClientEvent::UserJoinedServer { user_id, username })
                    .await;

                debug!("User joined server: id={}", user_id);
                Ok(())
            }
            PacketId::UserLeftServer => {
                let user_id = voiceapp_protocol::decode_user_left_server(&payload)
                    .map_err(|e| VoiceClientError::ConnectionFailed(e.to_string()))?;

                let mut s = state.write().await;
                s.participants.remove(&user_id);
                drop(s); // Release lock before sending event

                let _ = client_events_tx
                    .send(VoiceClientEvent::UserLeftServer { user_id })
                    .await;

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
                drop(s); // Release lock before sending event

                let _ = client_events_tx
                    .send(VoiceClientEvent::UserJoinedVoice { user_id })
                    .await;

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
                drop(s); // Release lock before sending event

                let _ = client_events_tx
                    .send(VoiceClientEvent::UserLeftVoice { user_id })
                    .await;

                debug!("User left voice: id={}", user_id);
                Ok(())
            }
            PacketId::UserSentMessage => {
                let (user_id, timestamp, message) =
                    voiceapp_protocol::decode_user_sent_message(&payload)
                        .map_err(|e| VoiceClientError::ConnectionFailed(e.to_string()))?;

                let _ = client_events_tx
                    .send(VoiceClientEvent::UserSentMessage {
                        user_id,
                        timestamp,
                        message: message.clone(),
                    })
                    .await;

                debug!("User sent message: id={}, timestamp={}", user_id, timestamp);
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
        let timeout = tokio::time::timeout(std::time::Duration::from_secs(timeout_secs), async {
            loop {
                match self.response_rx.recv().await {
                    Ok((packet_id, payload)) => {
                        if packet_id == expected_id {
                            return Ok(payload);
                        }
                    }
                    Err(_) => return Err(VoiceClientError::Disconnected),
                }
            }
        });

        match timeout.await {
            Ok(Ok(payload)) => {
                decoder(&payload).map_err(|e| VoiceClientError::ConnectionFailed(e.to_string()))
            }
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
    request_rx: Receiver<Vec<u8>>,
    response_tx: Sender<(PacketId, Vec<u8>)>,
    event_tx: Sender<(PacketId, Vec<u8>)>,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut read_buf = [0u8; 4096];

    loop {
        tokio::select! {
            // Handle outgoing requests (already fully encoded)
            result = request_rx.recv() => {
                match result {
                    Ok(packet) => { socket.write_all(&packet).await? }
                    Err(_) => return Err("Request channel closed".into()),
                }
            }

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
                    // Responses (0x11-0x15)
                    PacketId::LoginResponse
                    | PacketId::VoiceAuthResponse
                    | PacketId::JoinVoiceChannelResponse
                    | PacketId::LeaveVoiceChannelResponse
                    | PacketId::ChatMessageResponse => {
                        response_tx.send((packet_id, payload.to_vec())).await?;
                    }
                    // Events (0x21-0x25)
                    PacketId::UserJoinedServer
                    | PacketId::UserJoinedVoice
                    | PacketId::UserLeftVoice
                    | PacketId::UserLeftServer
                    | PacketId::UserSentMessage => {
                        let _ = event_tx.send((packet_id, payload.to_vec())).await;
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
    send_rx: Receiver<Vec<u8>>,
    recv_tx: Sender<(PacketId, Vec<u8>)>,
    decoder: Arc<VoiceDecoder>,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut read_buf = [0u8; 4096];

    loop {
        tokio::select! {
            // Handle outgoing packets
            result = send_rx.recv() => {
                match result {
                    Ok(packet) => {
                        socket.send(&packet).await?;
                        // TODO: packet.len() for bytes sent
                    }
                    Err(_) => return Err("Send channel closed".into()),
                }
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
                        // CPAL callback will pull audio on-demand via get_audio()
                        if let Err(e) = decoder.insert_packet(voice_packet).await {
                            debug!("Failed to insert voice packet: {}", e);
                        }
                    }
                } else {
                    // Route non-voice packets to receiver
                    let _ = recv_tx.send((packet_id, payload.to_vec())).await;
                }
            }
        }
    }

    Ok(())
}
