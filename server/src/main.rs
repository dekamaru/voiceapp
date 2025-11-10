use std::net::SocketAddr;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream, UdpSocket};
use tokio::sync::{broadcast, RwLock};
use tracing::{debug, error, info};
use voiceapp_common::{TcpPacket, PacketTypeId, encode_username, decode_username, encode_participant_list_with_voice, ParticipantInfo, VoicePacket, UdpAuthPacket, UdpAuthResponse, encode_voice_token};
use std::collections::HashMap;
use rand::Rng;

const MAX_BUFFER_SIZE: usize = 65536; // Prevent memory exhaustion attacks

type BroadcastSender = broadcast::Sender<BroadcastEvent>;

#[derive(Clone, Debug)]
enum BroadcastEvent {
    UserJoined { username: String },
    UserLeft { username: String },
    UserJoinedVoice { username: String, udp_addr: SocketAddr },
    UserLeftVoice { username: String },
}

/// Represents a connected user with their voice channel status and UDP authentication token
#[derive(Clone, Debug)]
struct User {
    username: String,
    in_voice: bool,
    token: u64, // Authentication token for UDP connections
    udp_address: Option<SocketAddr>, // Authenticated UDP address (set after token validation)
}

struct Server {
    broadcast_tx: BroadcastSender,
    users: Arc<RwLock<HashMap<String, User>>>, // All connected users with voice status and UDP address
    voice_listen_port: u16, // UDP port for voice relay
}

impl Server {
    fn new() -> Self {
        let (tx, _rx) = broadcast::channel(100);
        Server {
            broadcast_tx: tx,
            users: Arc::new(RwLock::new(HashMap::new())),
            voice_listen_port: 9002, // Default port for voice relay
        }
    }

    fn with_voice_port(mut self, port: u16) -> Self {
        self.voice_listen_port = port;
        self
    }

    async fn run(&self, addr: &str) -> Result<(), Box<dyn std::error::Error>> {
        let listener = TcpListener::bind(addr).await?;
        let local_addr = listener.local_addr()?;
        info!("Server listening on {}", local_addr);

        // Start UDP voice relay listener on configured port
        let udp_addr = format!("127.0.0.1:{}", self.voice_listen_port);
        let udp_socket = create_voice_socket(&udp_addr).await?;
        info!("Voice relay listening on {}", udp_socket.local_addr()?);
        let udp_socket = Arc::new(udp_socket);
        let users = self.users.clone();
        tokio::spawn({
            let udp_socket = udp_socket.clone();
            let users = users.clone();
            async move {
                handle_udp_relay(udp_socket, users).await
            }
        });

        loop {
            let (socket, peer_addr) = listener.accept().await?;
            info!("[{}] New connection", peer_addr);

            let broadcast_tx = self.broadcast_tx.clone();
            let users = self.users.clone();
            tokio::spawn(async move {
                if let Err(e) = handle_client(socket, broadcast_tx, users, peer_addr).await {
                    error!("[{}] Error: {}", peer_addr, e);
                }
            });
        }
    }
}

/// Validate username: non-empty, reasonable length, valid UTF-8
fn validate_username(username: &str) -> Result<(), Box<dyn std::error::Error>> {
    const MIN_LEN: usize = 1;
    const MAX_LEN: usize = 32;

    if username.len() < MIN_LEN {
        return Err("Username too short".into());
    }
    if username.len() > MAX_LEN {
        return Err("Username too long".into());
    }
    if !username.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-') {
        return Err("Username contains invalid characters".into());
    }
    Ok(())
}

/// Create a UDP socket for voice relay
async fn create_voice_socket(addr: &str) -> Result<UdpSocket, Box<dyn std::error::Error>> {
    let udp_socket = UdpSocket::bind(addr).await?;
    Ok(udp_socket)
}

/// Remove user from users HashMap and broadcast UserLeft event
async fn remove_participant(
    users: &Arc<RwLock<HashMap<String, User>>>,
    broadcast_tx: &BroadcastSender,
    username: &str,
) {
    let mut lock = users.write().await;
    lock.remove(username);
    drop(lock);
    let _ = broadcast_tx.send(BroadcastEvent::UserLeft {
        username: username.to_string(),
    });
}

/// Handle UDP voice packet relay
/// Listens for incoming voice packets and authenticates them with tokens
/// Maps authenticated UDP addresses to users and forwards voice packets
async fn handle_udp_relay(
    udp_socket: Arc<UdpSocket>,
    users: Arc<RwLock<HashMap<String, User>>>,
) {
    let mut buf = vec![0u8; 4096];
    // Map UDP address to token for one-time authentication
    let mut authenticated_addrs: HashMap<SocketAddr, u64> = HashMap::new();

    loop {
        match udp_socket.recv_from(&mut buf).await {
            Ok((n, src_addr)) => {
                // Check if address is already authenticated
                if authenticated_addrs.contains_key(&src_addr) {
                    // Try to decode as voice packet
                    match VoicePacket::decode(&buf[..n]) {
                        Ok((packet, _)) => {
                            debug!("Received voice packet from {}: seq={}, ts={}", src_addr, packet.sequence, packet.timestamp);

                            // Get all users in voice channel with their UDP addresses
                            let users_lock = users.read().await;

                            // Forward to all users in voice channel except sender
                            for user in users_lock.values() {
                                if user.in_voice && user.udp_address.is_some() {
                                    if let Some(dest_addr) = user.udp_address {
                                        if dest_addr != src_addr {
                                            // Send packet to this user
                                            if let Err(e) = udp_socket.send_to(&buf[..n], dest_addr).await {
                                                error!("Failed to forward voice packet to {}: {}", dest_addr, e);
                                            }
                                        }
                                    }
                                }
                            }
                        }
                        Err(e) => {
                            debug!("Failed to decode voice packet from {}: {}", src_addr, e);
                        }
                    }
                } else {
                    // Try to decode as auth packet first
                    match UdpAuthPacket::decode(&buf[..n]) {
                        Ok((auth_packet, _)) => {
                            debug!("Received auth packet from {}: username={}", src_addr, auth_packet.username);

                            // Validate token against user's stored token
                            let auth_success = {
                                let mut users_lock = users.write().await;
                                if let Some(user) = users_lock.get_mut(&auth_packet.username) {
                                    if user.token == auth_packet.token && user.in_voice {
                                        // Token is valid, authenticate this address
                                        authenticated_addrs.insert(src_addr, auth_packet.token);
                                        // Update user's UDP address
                                        user.udp_address = Some(src_addr);
                                        debug!("Authenticated voice connection from {} for user {}", src_addr, auth_packet.username);
                                        true
                                    } else {
                                        error!("Invalid token or user not in voice from {}", src_addr);
                                        false
                                    }
                                } else {
                                    error!("User {} not found for auth from {}", auth_packet.username, src_addr);
                                    false
                                }
                            };

                            // Send response back to client
                            let response = UdpAuthResponse::new(auth_success);
                            if let Ok(response_data) = response.encode() {
                                if let Err(e) = udp_socket.send_to(&response_data, src_addr).await {
                                    error!("Failed to send auth response to {}: {}", src_addr, e);
                                } else {
                                    debug!("Sent auth response (success={}) to {}", auth_success, src_addr);
                                }
                            }
                        }
                        Err(_) => {
                            debug!("Received packet from {} that is neither auth nor voice", src_addr);
                        }
                    }
                }
            }
            Err(e) => {
                error!("UDP receive error: {}", e);
            }
        }
    }
}

async fn handle_client(
    mut socket: TcpStream,
    broadcast_tx: BroadcastSender,
    users: Arc<RwLock<HashMap<String, User>>>,
    peer_addr: SocketAddr,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut buffer = Vec::new();
    let mut read_buf = vec![0u8; 4096];

    // === LOGIN PHASE ===
    // Accumulate data until we have a complete Login packet
    let username = loop {
        match TcpPacket::decode(&buffer) {
            Ok((packet, bytes_read)) => {
                if packet.packet_type != PacketTypeId::Login {
                    error!("[{}] Expected Login, got {:?}", peer_addr, packet.packet_type);
                    return Err("Expected Login packet".into());
                }

                let username = decode_username(&packet.payload)?;
                validate_username(&username)?;
                info!("[{}] {} logged in", peer_addr, username);

                buffer.drain(0..bytes_read);
                break username;
            }
            Err(_) => {
                // Need more data
                if buffer.len() > MAX_BUFFER_SIZE {
                    return Err("Buffer overflow during login".into());
                }

                let n = socket.read(&mut read_buf).await?;
                if n == 0 {
                    debug!("[{}] Disconnected before login", peer_addr);
                    return Ok(());
                }
                buffer.extend_from_slice(&read_buf[..n]);
            }
        }
    };

    // Add user to users HashMap with random token
    let token = {
        let mut rng = rand::thread_rng();
        rng.gen::<u64>()
    };
    {
        let mut lock = users.write().await;
        lock.insert(username.clone(), User {
            username: username.clone(),
            in_voice: false,
            token,
            udp_address: None,
        });
    }

    // Send LoginResponse with the token
    {
        let token_payload = encode_voice_token(token)?;
        let login_response = TcpPacket::new(PacketTypeId::LoginResponse, token_payload);
        socket.write_all(&login_response.encode()?).await?;
        socket.flush().await?;
        debug!("[{}] Sent LoginResponse with UDP voice token to {}", peer_addr, username);
    }

    // Send ServerParticipantList with voice status to the new user
    {
        let users_lock = users.read().await;
        let participant_infos: Vec<ParticipantInfo> = users_lock
            .values()
            .map(|user| {
                ParticipantInfo {
                    username: user.username.clone(),
                    in_voice: user.in_voice,
                }
            })
            .collect();
        let payload = encode_participant_list_with_voice(&participant_infos)?;
        let pkt = TcpPacket::new(PacketTypeId::ServerParticipantList, payload);
        socket.write_all(&pkt.encode()?).await?;
        socket.flush().await?;
        debug!("[{}] Sent participant list with voice status ({} users)", peer_addr, participant_infos.len());
    }

    // Broadcast user joined
    let _ = broadcast_tx.send(BroadcastEvent::UserJoined { username: username.clone() });

    // === CLIENT LOOP PHASE ===
    let mut broadcast_rx = broadcast_tx.subscribe();

    loop {
        tokio::select! {
            result = socket.read(&mut read_buf) => {
                match result {
                    Ok(0) => {
                        info!("[{}] {} disconnected", peer_addr, username);
                        remove_participant(&users, &broadcast_tx, &username).await;
                        break;
                    }
                    Ok(n) => {
                        buffer.extend_from_slice(&read_buf[..n]);

                        // Check for buffer overflow
                        if buffer.len() > MAX_BUFFER_SIZE {
                            error!("[{}] Buffer overflow", peer_addr);
                            remove_participant(&users, &broadcast_tx, &username).await;
                            return Err("Buffer overflow".into());
                        }

                        // Try to parse packets from buffer
                        while let Ok((packet, bytes_read)) = TcpPacket::decode(&buffer) {
                            match packet.packet_type {
                                PacketTypeId::JoinVoiceChannel => {
                                    // Extract username from payload (no UDP port anymore)
                                    match decode_username(&packet.payload) {
                                        Ok(join_username) => {
                                            debug!("[{}] {} requesting to join voice channel", peer_addr, join_username);

                                            // Get token for this user
                                            let token = {
                                                let users_lock = users.read().await;
                                                users_lock.get(&join_username).map(|u| u.token)
                                            };

                                            if let Some(_token) = token {
                                                // Update user's voice status
                                                let mut users_lock = users.write().await;
                                                if let Some(user) = users_lock.get_mut(&join_username) {
                                                    user.in_voice = true;
                                                }
                                                drop(users_lock);

                                                debug!("[{}] {} joined voice channel", peer_addr, join_username);

                                                // Broadcast voice join (UDP address will be set on auth)
                                                let _ = broadcast_tx.send(BroadcastEvent::UserJoinedVoice {
                                                    username: join_username.clone(),
                                                    udp_addr: peer_addr,
                                                });
                                            } else {
                                                error!("[{}] User {} not found for voice join", peer_addr, join_username);
                                            }
                                        }
                                        Err(e) => {
                                            error!("[{}] Failed to decode join voice packet: {}", peer_addr, e);
                                        }
                                    }
                                }
                                PacketTypeId::UserLeftVoice => {
                                    debug!("[{}] {} leaving voice channel", peer_addr, username);

                                    // Update user's voice status
                                    let mut users_lock = users.write().await;
                                    if let Some(user) = users_lock.get_mut(&username) {
                                        user.in_voice = false;
                                        user.udp_address = None;
                                    }
                                    drop(users_lock);

                                    let _ = broadcast_tx.send(BroadcastEvent::UserLeftVoice {
                                        username: username.clone()
                                    });
                                }
                                _ => {
                                    debug!("[{}] Ignoring packet type {:?}", peer_addr, packet.packet_type);
                                }
                            }
                            buffer.drain(0..bytes_read);
                        }
                    }
                    Err(e) => {
                        error!("[{}] Read error: {}", peer_addr, e);
                        remove_participant(&users, &broadcast_tx, &username).await;
                        return Err(e.into());
                    }
                }
            }
            result = broadcast_rx.recv() => {
                match result {
                    Ok(BroadcastEvent::UserJoined { username: other_user }) => {
                        if other_user != username {
                            let pkt = TcpPacket::new(
                                PacketTypeId::UserJoinedServer,
                                encode_username(&other_user),
                            );
                            socket.write_all(&pkt.encode()?).await?;
                            socket.flush().await?;
                            debug!("[{}] Broadcasted {} joined", peer_addr, other_user);
                        }
                    }
                    Ok(BroadcastEvent::UserLeft { username: other_user }) => {
                        if other_user != username {
                            let pkt = TcpPacket::new(
                                PacketTypeId::UserLeftServer,
                                encode_username(&other_user),
                            );
                            socket.write_all(&pkt.encode()?).await?;
                            socket.flush().await?;
                            debug!("[{}] Broadcasted {} left", peer_addr, other_user);
                        }
                    }
                    Ok(BroadcastEvent::UserJoinedVoice { username: other_user, udp_addr: _ }) => {
                        // Broadcast to all participants so they can update UI and create output streams
                        if other_user != username {
                            let pkt = TcpPacket::new(
                                PacketTypeId::UserJoinedVoice,
                                encode_username(&other_user),
                            );
                            socket.write_all(&pkt.encode()?).await?;
                            socket.flush().await?;
                            debug!("[{}] Broadcasted {} joined voice", peer_addr, other_user);
                        }
                    }
                    Ok(BroadcastEvent::UserLeftVoice { username: other_user }) => {
                        // Broadcast to all participants so they can update UI
                        if other_user != username {
                            let pkt = TcpPacket::new(
                                PacketTypeId::UserLeftVoice,
                                encode_username(&other_user),
                            );
                            socket.write_all(&pkt.encode()?).await?;
                            socket.flush().await?;
                            debug!("[{}] Broadcasted {} left voice", peer_addr, other_user);
                        }
                    }
                    Err(_) => break,
                }
            }
        }
    }

    Ok(())
}

#[tokio::main]
async fn main() {
    #[cfg(debug_assertions)]
    {
        use tracing::Level;
        tracing_subscriber::fmt()
            .with_max_level(Level::DEBUG)
            .init();
    }

    #[cfg(not(debug_assertions))]
    {
        tracing_subscriber::fmt::init();
    }

    let server = Server::new();
    if let Err(e) = server.run("127.0.0.1:9001").await {
        error!("Server error: {}", e);
    }
}
