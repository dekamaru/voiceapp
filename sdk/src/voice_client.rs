use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::{mpsc, RwLock, Mutex};
use tracing::{debug, error, info, warn};

use voiceapp_protocol::{
    PacketId, VoiceData,
    parse_packet, encode_login_request, encode_join_voice_channel_request,
    encode_leave_voice_channel_request, encode_voice_auth_request,
    decode_user_joined_voice, decode_user_left_voice,
};

// Note: we use decode_login_response in handle_tcp_packet at line ~296

use crate::udp_voice_receiver::UdpVoiceReceiver;
use crate::user_voice_stream::UserVoiceStreamManager;

/// Main VoiceClient - handles all TCP/UDP communication transparently
///
/// Provides a clean async API for voice chat without exposing transport details.
/// All networking complexity is hidden inside this struct.
#[derive(Clone)]
pub struct VoiceClient {
    // Private network state
    tcp_socket: Arc<Mutex<TcpStream>>,

    // Private user/session state
    user_id: Arc<RwLock<Option<u64>>>,
    voice_token: Arc<RwLock<Option<u64>>>,
    participants: Arc<RwLock<Vec<Participant>>>,

    // Audio channels (only public interface to networking)
    audio_input_tx: mpsc::UnboundedSender<AudioFrame>,
    audio_output_rx: Arc<Mutex<Option<mpsc::UnboundedReceiver<AudioFrame>>>>,
    #[allow(dead_code)]
    audio_output_tx: mpsc::UnboundedSender<AudioFrame>,

    // Response notifications (for blocking on request responses)
    response_rx: Arc<Mutex<mpsc::UnboundedReceiver<ResponseNotification>>>,

    // Voice state - internal management
    voice_state: Arc<VoiceState>,
}

/// Request ID for tracking pending requests
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct RequestId(u64);

/// Generic response notification
#[derive(Debug, Clone)]
pub enum ResponseNotification {
    LoginResponse(Result<(), String>),
    JoinChannelResponse(Result<(), String>),
}

/// Internal voice state management
struct VoiceState {
    manager: Arc<UserVoiceStreamManager>,
    receiver: Arc<UdpVoiceReceiver>,
}

/// Represents a participant in the voice channel
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Participant {
    pub user_id: u64,
    pub in_voice: bool,
}

/// Audio frame for voice transmission (wraps raw VoiceData with meaningful name)
pub type AudioFrame = VoiceData;

/// Represents errors that can occur during voice operations
#[derive(Debug)]
pub enum VoiceClientError {
    IoError(std::io::Error),
    ChannelError(String),
    AuthError(String),
    StateError(String),
}

impl From<std::io::Error> for VoiceClientError {
    fn from(err: std::io::Error) -> Self {
        VoiceClientError::IoError(err)
    }
}

impl From<Box<dyn std::error::Error>> for VoiceClientError {
    fn from(err: Box<dyn std::error::Error>) -> Self {
        VoiceClientError::StateError(err.to_string())
    }
}

impl std::fmt::Display for VoiceClientError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            VoiceClientError::IoError(e) => write!(f, "IO error: {}", e),
            VoiceClientError::ChannelError(e) => write!(f, "Channel error: {}", e),
            VoiceClientError::AuthError(e) => write!(f, "Auth error: {}", e),
            VoiceClientError::StateError(e) => write!(f, "State error: {}", e),
        }
    }
}

impl std::error::Error for VoiceClientError {}

impl VoiceClient {
    /// Connect to the voice server and initialize all internal systems
    ///
    /// This is the entry point for the VoiceClient. It establishes connection,
    /// spawns all background tasks, and prepares the client for use.
    pub async fn connect(server_addr: &str) -> Result<Self, VoiceClientError> {
        info!("Connecting to voice server at {}", server_addr);

        // Connect TCP socket
        let tcp_socket = TcpStream::connect(server_addr).await?;
        info!("TCP connected to {}", server_addr);

        // Create response notification channel (before VoiceState)
        let (response_tx, response_rx) = mpsc::unbounded_channel::<ResponseNotification>();

        // Initialize UDP voice receiver on random port
        let voice_state = Arc::new(VoiceState::new("127.0.0.1:0", response_tx.clone()).await?);
        info!("Voice receiver listening on {}", voice_state.receiver.local_addr()?);

        // Create audio channels
        let (audio_input_tx, audio_input_rx) = mpsc::unbounded_channel::<AudioFrame>();
        let (audio_output_tx, audio_output_rx) = mpsc::unbounded_channel::<AudioFrame>();

        let tcp_socket = Arc::new(Mutex::new(tcp_socket));
        let user_id = Arc::new(RwLock::new(None));
        let voice_token = Arc::new(RwLock::new(None));
        let participants = Arc::new(RwLock::new(Vec::new()));

        // Spawn TCP listener task in the main tokio context
        // We can use tokio::spawn here since we're already in an async context
        let tcp_socket_clone = tcp_socket.clone();
        let voice_state_clone = voice_state.clone();
        let user_id_clone = user_id.clone();
        let voice_token_clone = voice_token.clone();
        let participants_clone = participants.clone();
        let response_tx_clone = response_tx;

        tokio::spawn(async move {
            Self::tcp_listener_loop(
                tcp_socket_clone,
                voice_state_clone,
                user_id_clone,
                voice_token_clone,
                participants_clone,
                response_tx_clone,
            ).await;
        });

        // Spawn UDP input task (audio_input_rx -> send to server)
        Self::spawn_audio_input_task(
            audio_input_rx,
            voice_state.receiver.clone(),
            server_addr.replace("9001", "9002"),
        );

        Ok(VoiceClient {
            tcp_socket,
            user_id,
            voice_token,
            participants,
            audio_input_tx,
            audio_output_rx: Arc::new(Mutex::new(Some(audio_output_rx))),
            audio_output_tx,
            response_rx: Arc::new(Mutex::new(response_rx)),
            voice_state,
        })
    }

    /// Authenticate with the server using a username
    ///
    /// This completes the login handshake and blocks until receiving LoginResponse,
    /// populating user ID and initial participants.
    pub async fn authenticate(&mut self, username: &str) -> Result<(), VoiceClientError> {
        debug!("Authenticating as '{}'", username);

        // Send login request
        let login_packet = encode_login_request(username)?;

        let mut socket = self.tcp_socket.lock().await;
        socket.write_all(&login_packet).await?;
        socket.flush().await?;

        info!("Sent login request for '{}'", username);
        drop(socket); // Release the lock before waiting

        // Wait for LoginResponse
        let mut response_rx = self.response_rx.lock().await;
        while let Some(notification) = response_rx.recv().await {
            match notification {
                ResponseNotification::LoginResponse(result) => {
                    return result.map_err(|e| VoiceClientError::AuthError(e));
                }
                _ => {
                    // Ignore other notifications while waiting for login
                    debug!("Ignoring non-login response while waiting for authentication");
                }
            }
        }

        Err(VoiceClientError::StateError("Response channel closed before login response".to_string()))
    }

    /// Join the voice channel
    ///
    /// This initiates voice channel join and UDP authentication internally.
    /// Blocks until server acknowledges the join request.
    pub async fn join_channel(&mut self) -> Result<(), VoiceClientError> {
        debug!("Joining voice channel");

        // Send join request via TCP
        let join_packet = encode_join_voice_channel_request()?;

        let mut socket = self.tcp_socket.lock().await;
        socket.write_all(&join_packet).await?;
        socket.flush().await?;

        info!("Sent join voice channel request");
        drop(socket); // Release the lock before waiting

        // Get voice token (user_id is not used here)
        let voice_token = self.voice_token.read().await.ok_or_else(|| {
            VoiceClientError::StateError("Voice token not available - login incomplete".to_string())
        })?;

        // Start UDP voice authentication
        let server_voice_addr = self.voice_state.receiver.local_addr()?
            .to_string()
            .split(':')
            .next()
            .unwrap_or("127.0.0.1")
            .to_string() + ":9002";

        self.authenticate_voice_udp(voice_token, &server_voice_addr).await?;

        // Start receiving voice packets
        self.voice_state.receiver.start_receiving();

        info!("Successfully joined voice channel");
        Ok(())
    }

    /// Leave the voice channel
    pub async fn leave_channel(&mut self) -> Result<(), VoiceClientError> {
        debug!("Leaving voice channel");

        let leave_packet = encode_leave_voice_channel_request()?;

        let mut socket = self.tcp_socket.lock().await;
        socket.write_all(&leave_packet).await?;
        socket.flush().await?;

        info!("Left voice channel");
        Ok(())
    }

    /// Send audio frame to other participants
    ///
    /// Non-blocking operation. Returns error only if the sender is dropped.
    pub fn send_audio(&self, frame: AudioFrame) -> Result<(), VoiceClientError> {
        self.audio_input_tx.send(frame)
            .map_err(|_| VoiceClientError::ChannelError("Audio input channel closed".to_string()))
    }

    /// Get the current user's ID (None if not authenticated)
    pub async fn current_user_id(&self) -> Option<u64> {
        *self.user_id.read().await
    }

    /// Get the list of participants in the voice channel
    pub async fn participants(&self) -> Vec<Participant> {
        self.participants.read().await.clone()
    }

    /// Subscribe to incoming audio frames
    ///
    /// Returns a receiver that yields audio frames from other participants.
    /// Should only be called once - subsequent calls will return None.
    pub async fn audio_output(&self) -> Option<mpsc::UnboundedReceiver<AudioFrame>> {
        self.audio_output_rx.lock().await.take()
    }

    // ============== INTERNAL IMPLEMENTATION ==============

    /// TCP listener loop - continuously reads server packets
    async fn tcp_listener_loop(
        tcp_socket: Arc<Mutex<TcpStream>>,
        voice_state: Arc<VoiceState>,
        user_id: Arc<RwLock<Option<u64>>>,
        voice_token: Arc<RwLock<Option<u64>>>,
        participants: Arc<RwLock<Vec<Participant>>>,
        response_tx: mpsc::UnboundedSender<ResponseNotification>,
    ) {
        let mut buf = vec![0u8; 4096];
        loop {
            let mut socket = tcp_socket.lock().await;
            match socket.read(&mut buf).await {
                Ok(0) => {
                    info!("Server closed TCP connection");
                    break;
                }
                Ok(n) => {
                    let packet_buf = buf[..n].to_vec();
                    drop(socket); // Release lock before processing

                    match parse_packet(&packet_buf) {
                        Ok((packet_id, payload)) => {
                            // Handle packets asynchronously
                            handle_tcp_packet_impl(
                                packet_id,
                                payload,
                                &voice_state.manager,
                                &voice_state.receiver,
                                &user_id,
                                &voice_token,
                                &participants,
                                &response_tx,
                            ).await;
                        }
                        Err(e) => {
                            error!("Failed to parse packet: {}", e);
                        }
                    }
                }
                Err(e) => {
                    error!("TCP socket error: {}", e);
                    break;
                }
            }
        }
    }

    /// Authenticate with UDP voice channel
    async fn authenticate_voice_udp(
        &self,
        token: u64,
        server_voice_addr: &str,
    ) -> Result<(), VoiceClientError> {
        debug!("Authenticating UDP voice channel with token");

        let max_attempts = 3;
        for attempt in 1..=max_attempts {
            match encode_voice_auth_request(token) {
                Ok(auth_data) => {
                    if let Err(e) = self.voice_state.receiver.send_to(&auth_data, server_voice_addr).await {
                        warn!("Attempt {}: Failed to send UDP auth: {}", attempt, e);
                        continue;
                    }

                    match self.voice_state.receiver.wait_auth_response(5).await {
                        Ok(true) => {
                            info!("UDP voice authentication successful");
                            return Ok(());
                        }
                        Ok(false) => {
                            warn!("Attempt {}: UDP auth rejected by server", attempt);
                            continue;
                        }
                        Err(e) => {
                            warn!("Attempt {}: UDP auth timeout or error: {}", attempt, e);
                            continue;
                        }
                    }
                }
                Err(e) => {
                    error!("Attempt {}: Failed to encode auth packet: {}", attempt, e);
                    continue;
                }
            }
        }

        Err(VoiceClientError::AuthError(
            format!("Failed to authenticate with voice server after {} attempts", max_attempts)
        ))
    }

    /// Spawn task to handle outgoing audio frames
    fn spawn_audio_input_task(
        mut audio_input_rx: mpsc::UnboundedReceiver<AudioFrame>,
        receiver: Arc<UdpVoiceReceiver>,
        server_voice_addr: String,
    ) {
        tokio::spawn(async move {
            while let Some(frame) = audio_input_rx.recv().await {
                if let Err(e) = receiver.send_voice_packet(&frame, &server_voice_addr).await {
                    error!("Failed to send voice packet: {}", e);
                }
            }
        });
    }
}

/// Handle incoming TCP packets from server
async fn handle_tcp_packet_impl(
    packet_id: PacketId,
    payload: &[u8],
    manager: &Arc<UserVoiceStreamManager>,
    receiver: &Arc<UdpVoiceReceiver>,
    user_id: &Arc<RwLock<Option<u64>>>,
    voice_token: &Arc<RwLock<Option<u64>>>,
    participants: &Arc<RwLock<Vec<Participant>>>,
    response_tx: &mpsc::UnboundedSender<ResponseNotification>,
) {
    match packet_id {
        PacketId::LoginResponse => {
            match voiceapp_protocol::decode_login_response(payload) {
                Ok(response) => {
                    info!("Login successful: user_id={}", response.id);
                    debug!("Received voice token: {}", response.voice_token);

                    // Store user ID and token
                    *user_id.write().await = Some(response.id);
                    *voice_token.write().await = Some(response.voice_token);

                    // Update participants
                    let mut parts = participants.write().await;
                    parts.clear();
                    for p in response.participants.iter() {
                        parts.push(Participant {
                            user_id: p.user_id,
                            in_voice: p.in_voice,
                        });
                    }
                    drop(parts);

                    // Register existing participants in voice
                    for participant in response.participants.iter() {
                        if participant.in_voice {
                            receiver.register_user(
                                participant.user_id,
                                format!("user_{}", participant.user_id)
                            ).await;
                            debug!("Registered user {} in voice channel", participant.user_id);
                        }
                    }

                    // Notify that login was successful
                    let _ = response_tx.send(ResponseNotification::LoginResponse(Ok(())));
                }
                Err(e) => {
                    error!("Failed to decode login response: {}", e);
                    let _ = response_tx.send(ResponseNotification::LoginResponse(Err(format!("Failed to decode login response: {}", e))));
                }
            }
        }
        PacketId::UserJoinedVoice => {
            match decode_user_joined_voice(payload) {
                Ok(user_id) => {
                    info!("User joined voice channel: {}", user_id);

                    // Update participants list
                    let mut parts = participants.write().await;
                    if let Some(p) = parts.iter_mut().find(|p| p.user_id == user_id) {
                        p.in_voice = true;
                    } else {
                        parts.push(Participant {
                            user_id,
                            in_voice: true,
                        });
                    }
                    drop(parts);

                    receiver.register_user(
                        user_id,
                        format!("user_{}", user_id)
                    ).await;

                    // Create voice packet channel for this user
                    let (tx, _rx) = mpsc::unbounded_channel::<VoiceData>();
                    let username = format!("user_{}", user_id);

                    if let Err(e) = manager.register_sender(username, tx).await {
                        error!("Failed to register sender for user {}: {}", user_id, e);
                    }
                }
                Err(e) => error!("Failed to decode user joined voice event: {}", e),
            }
        }
        PacketId::UserLeftVoice => {
            match decode_user_left_voice(payload) {
                Ok(user_id) => {
                    info!("User left voice channel: {}", user_id);

                    // Update participants list
                    let mut parts = participants.write().await;
                    if let Some(p) = parts.iter_mut().find(|p| p.user_id == user_id) {
                        p.in_voice = false;
                    }
                    drop(parts);

                    receiver.unregister_user(user_id).await;

                    let username = format!("user_{}", user_id);
                    if let Err(e) = manager.unregister_sender(&username).await {
                        warn!("Failed to unregister sender for user {}: {}", user_id, e);
                    }
                }
                Err(e) => error!("Failed to decode user left voice event: {}", e),
            }
        }
        _ => {
            debug!("Ignoring packet type: {:?}", packet_id);
        }
    }
}

impl VoiceState {
    async fn new(voice_bind_addr: &str, _response_tx: mpsc::UnboundedSender<ResponseNotification>) -> Result<Self, VoiceClientError> {
        let manager = Arc::new(UserVoiceStreamManager::new());
        let receiver = Arc::new(UdpVoiceReceiver::new(voice_bind_addr, manager.clone()).await?);

        Ok(VoiceState {
            manager,
            receiver,
        })
    }
}
