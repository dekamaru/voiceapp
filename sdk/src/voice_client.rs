use async_channel::{unbounded, Receiver, Sender};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::net::UdpSocket;
use tokio::sync::RwLock;
use tracing::{debug, error, info};

use voiceapp_protocol::{PacketId, ParticipantInfo};
use crate::tcp_client::TcpClient;
use crate::voice_decoder_manager::VoiceDecoderManager;

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
    tcp_client: TcpClient,
    decoder_manager: Arc<VoiceDecoderManager>,
    udp_send_tx: Sender<Vec<u8>>,
    udp_send_rx: Receiver<Vec<u8>>,
    udp_recv_tx: Sender<(PacketId, Vec<u8>)>,
    udp_recv_rx: Receiver<(PacketId, Vec<u8>)>,
    client_events_tx: Sender<VoiceClientEvent>,
    client_events_rx: Receiver<VoiceClientEvent>,
}

impl VoiceClient {
    /// Create a new VoiceClient with all channels initialized
    /// sample_rate: target output sample rate for the decoder (should match audio device)
    pub fn new(output_sample_rate: u32) -> Result<Self, VoiceClientError> {
        // Create TCP client
        let tcp_client = TcpClient::new();

        // Create decoder manager with specified sample rate
        let decoder_manager = Arc::new(VoiceDecoderManager::new(output_sample_rate));

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
            tcp_client,
            decoder_manager,
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
        &self,
        management_server_addr: &str,
        voice_server_addr: &str,
        username: &str,
    ) -> Result<(), VoiceClientError> {
        // Connect TCP socket
        self.tcp_client.connect(management_server_addr).await?;

        info!("[Management server] Connected to {}", management_server_addr);

        // Subscribe to TCP event stream and spawn processor
        self.spawn_tcp_event_processor();

        // Create UDP socket (bind to any available port)
        let udp_socket = UdpSocket::bind("0.0.0.0:0")
            .await
            .map_err(|e| VoiceClientError::ConnectionFailed(format!("UDP bind failed: {}", e)))?;

        // Connect UDP socket to server address
        udp_socket.connect(voice_server_addr).await.map_err(|e| {
            VoiceClientError::ConnectionFailed(format!("UDP connect failed: {}", e))
        })?;

        info!("[Voice server] Connected to {}", voice_server_addr);

        // Spawn UDP handler
        self.spawn_udp_handler(udp_socket);

        // Authenticate
        debug!("Authenticating as '{}'", username);

        // TCP authentication
        let response = self
            .tcp_client
            .send_request_with_response(
                voiceapp_protocol::encode_login_request(username),
                PacketId::LoginResponse,
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

        info!("[Management server] Authenticated, user_id={}", response.id);

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
                    info!("[Voice server] Authenticated successfully");
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

    pub async fn join_channel(&self) -> Result<(), VoiceClientError> {
        debug!("Joining voice channel");

        // Send request and wait for response
        self.tcp_client.send_request(
            voiceapp_protocol::encode_join_voice_channel_request(),
            PacketId::JoinVoiceChannelResponse,
        )
        .await?;

        // Update client state
        let mut state = self.state.write().await;
        state.in_voice_channel = true;

        info!("Joined voice channel");
        Ok(())
    }

    pub async fn leave_channel(&self) -> Result<(), VoiceClientError> {
        debug!("Leaving voice channel");

        // Send request and wait for response
        self.tcp_client.send_request(
            voiceapp_protocol::encode_leave_voice_channel_request(),
            PacketId::LeaveVoiceChannelResponse,
        )
        .await?;

        // Update client state
        let mut state = self.state.write().await;
        state.in_voice_channel = false;

        info!("Left voice channel");
        Ok(())
    }

    pub async fn send_message(&self, message: &str) -> Result<(), VoiceClientError> {
        debug!("Sending chat message");

        // Send request and wait for response
        self.tcp_client.send_request(
            voiceapp_protocol::encode_chat_message_request(message),
            PacketId::ChatMessageResponse,
        )
        .await?;

        debug!("Chat message sent successfully");
        Ok(())
    }

    /// Get a receiver for decoded voice output (mono F32 PCM samples at 48kHz)
    /// Subscribe to incoming voice from other participants
    pub fn get_decoder_manager(&self) -> Arc<VoiceDecoderManager> {
        self.decoder_manager.clone()
    }

    /// Get list of user IDs currently in voice channel (blocking version)
    pub fn get_users_in_voice(&self) -> Vec<u64> {
        let state = self.state.blocking_read();
        state.participants
            .iter()
            .filter(|(_, info)| info.in_voice)
            .map(|(id, _)| *id)
            .collect()
    }

    pub fn is_in_voice_channel(&self) -> bool {
        let state = self.state.blocking_read();
        state.in_voice_channel
    }

    /// Get UDP send channel for AudioManager to forward encoded voice packets
    pub fn get_udp_send_tx(&self) -> Sender<Vec<u8>> {
        self.udp_send_tx.clone()
    }

    /// Spawn TCP event processor task
    fn spawn_tcp_event_processor(&self) {
        let tcp_event_rx = self.tcp_client.event_stream();
        let state = Arc::clone(&self.state);
        let client_events_tx = self.client_events_tx.clone();
        let decoder_manager = self.decoder_manager.clone();

        tokio::spawn(async move {
            loop {
                match tcp_event_rx.recv().await {
                    Ok((packet_id, payload)) => {
                        // Handle event based on packet type
                        let result = match packet_id {
                            PacketId::UserJoinedServer => {
                                match voiceapp_protocol::decode_user_joined_server(&payload) {
                                    Ok((user_id, username)) => {
                                        let mut s = state.write().await;
                                        s.participants.insert(
                                            user_id,
                                            ParticipantInfo {
                                                user_id,
                                                username: username.clone(),
                                                in_voice: false,
                                            },
                                        );
                                        drop(s);

                                        let _ = client_events_tx
                                            .send(VoiceClientEvent::UserJoinedServer { user_id, username })
                                            .await;

                                        debug!("User joined server: id={}", user_id);
                                        Ok(())
                                    }
                                    Err(e) => Err(format!("Decode error: {}", e)),
                                }
                            }
                            PacketId::UserLeftServer => {
                                match voiceapp_protocol::decode_user_left_server(&payload) {
                                    Ok(user_id) => {
                                        let mut s = state.write().await;
                                        s.participants.remove(&user_id);
                                        drop(s);

                                        let _ = client_events_tx
                                            .send(VoiceClientEvent::UserLeftServer { user_id })
                                            .await;

                                        debug!("User left server: id={}", user_id);
                                        Ok(())
                                    }
                                    Err(e) => Err(format!("Decode error: {}", e)),
                                }
                            }
                            PacketId::UserJoinedVoice => {
                                match voiceapp_protocol::decode_user_joined_voice(&payload) {
                                    Ok(user_id) => {
                                        let mut s = state.write().await;
                                        if let Some(participant) = s.participants.get_mut(&user_id) {
                                            participant.in_voice = true;
                                        }
                                        drop(s);

                                        let _ = client_events_tx
                                            .send(VoiceClientEvent::UserJoinedVoice { user_id })
                                            .await;

                                        debug!("User joined voice: id={}", user_id);
                                        Ok(())
                                    }
                                    Err(e) => Err(format!("Decode error: {}", e)),
                                }
                            }
                            PacketId::UserLeftVoice => {
                                match voiceapp_protocol::decode_user_left_voice(&payload) {
                                    Ok(user_id) => {
                                        // Remove decoder for user who left
                                        decoder_manager.remove_user(user_id).await;

                                        let mut s = state.write().await;
                                        if let Some(participant) = s.participants.get_mut(&user_id) {
                                            participant.in_voice = false;
                                        }
                                        drop(s);

                                        let _ = client_events_tx
                                            .send(VoiceClientEvent::UserLeftVoice { user_id })
                                            .await;

                                        debug!("User left voice: id={}", user_id);
                                        Ok(())
                                    }
                                    Err(e) => Err(format!("Decode error: {}", e)),
                                }
                            }
                            PacketId::UserSentMessage => {
                                match voiceapp_protocol::decode_user_sent_message(&payload) {
                                    Ok((user_id, timestamp, message)) => {
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
                                    Err(e) => Err(format!("Decode error: {}", e)),
                                }
                            }
                            _ => {
                                // Ignore non-event packets
                                debug!("Received non-event packet in event stream: {:?}", packet_id);
                                Ok(())
                            }
                        };

                        if let Err(e) = result {
                            error!("Event handling error: {}", e);
                        }
                    }
                    Err(_) => {
                        debug!("TCP event stream closed");
                        break;
                    }
                }
            }
        });
    }

    /// Spawn UDP handler task (called internally from connect)
    fn spawn_udp_handler(&self, socket: UdpSocket) {
        let send_rx = self.udp_send_rx.clone();
        let recv_tx = self.udp_recv_tx.clone();
        let decoder_manager = self.decoder_manager.clone();

        tokio::spawn(async move {
            let mut read_buf = [0u8; 4096];

            loop {
                tokio::select! {
                    // Handle outgoing packets
                    result = send_rx.recv() => {
                        match result {
                            Ok(packet) => {
                                if let Err(e) = socket.send(&packet).await {
                                    error!("UDP handler error: {}", e);
                                    break;
                                }
                                // TODO: packet.len() for bytes sent
                            }
                            Err(_) => {
                                error!("UDP handler error: Send channel closed");
                                break;
                            }
                        }
                    }

                    // Handle incoming packets
                    Ok(n) = socket.recv(&mut read_buf) => {
                        if n == 0 {
                            debug!("UDP socket closed");
                            break;
                        }

                        // Parse incoming packet
                        match voiceapp_protocol::parse_packet(&read_buf[..n]) {
                            Ok((packet_id, payload)) => {
                                // Decode voice data packets
                                if packet_id == PacketId::VoiceData {
                                    if let Ok(voice_packet) = voiceapp_protocol::decode_voice_data(&payload) {
                                        if let Err(e) = decoder_manager.insert_packet(voice_packet).await {
                                            debug!("Failed to insert voice packet: {}", e);
                                        }
                                    }
                                } else {
                                    // Route non-voice packets to receiver
                                    let _ = recv_tx.send((packet_id, payload.to_vec())).await;
                                }
                            }
                            Err(e) => {
                                error!("UDP handler error: {}", e);
                                break;
                            }
                        }
                    }
                }
            }
        });
    }
}
