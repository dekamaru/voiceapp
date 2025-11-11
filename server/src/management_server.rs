use std::net::SocketAddr;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{broadcast, RwLock};
use tracing::{debug, error, info};
use voiceapp_common::{TcpPacket, PacketTypeId, encode_username, decode_username, encode_participant_list_with_voice, ParticipantInfo, encode_voice_token};
use rand::Rng;
use std::collections::HashMap;

const MAX_BUFFER_SIZE: usize = 65536; // Prevent memory exhaustion attacks

#[derive(Clone, Debug)]
pub enum BroadcastEvent {
    UserJoined { username: String },
    UserLeft { username: String },
    UserJoinedVoice { username: String },
    UserLeftVoice { username: String },
}

/// Represents a connected user with their voice channel status and authentication token
#[derive(Clone, Debug)]
pub struct User {
    pub username: String,
    pub in_voice: bool,
    pub token: u64, // Authentication token for UDP connections
}

type BroadcastSender = broadcast::Sender<BroadcastEvent>;

/// ManagementServer handles TCP connections, user login, presence management,
/// and broadcasts events to all connected clients
#[derive(Clone)]
pub struct ManagementServer {
    pub broadcast_tx: Arc<BroadcastSender>,
    pub users: Arc<RwLock<HashMap<SocketAddr, User>>>,
}

impl ManagementServer {
    pub fn new() -> Self {
        let (tx, _rx) = broadcast::channel(100);

        ManagementServer {
            broadcast_tx: Arc::new(tx),
            users: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Check if a token is valid (belongs to any logged-in user)
    pub async fn is_token_valid(&self, token: u64) -> bool {
        let users_lock = self.users.read().await;
        users_lock.values().any(|user| user.token == token)
    }

    /// Start the TCP listener and accept client connections
    pub async fn run(&self, addr: &str) -> Result<(), Box<dyn std::error::Error>> {
        let listener = TcpListener::bind(addr).await?;
        let local_addr = listener.local_addr()?;
        info!("ManagementServer listening on {}", local_addr);

        loop {
            let (socket, peer_addr) = listener.accept().await?;
            info!("[{}] New connection", peer_addr);

            let management = self.clone();
            tokio::spawn(async move {
                if let Err(e) = management.handle_client(socket, peer_addr).await {
                    error!("[{}] Error: {}", peer_addr, e);
                }
            });
        }
    }

    /// Handle a single TCP client connection
    async fn handle_client(
        &self,
        mut socket: TcpStream,
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
                    Self::validate_username(&username)?;
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
            let mut lock = self.users.write().await;
            lock.insert(peer_addr, User {
                username: username.clone(),
                in_voice: false,
                token,
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
            let users_lock = self.users.read().await;
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
            info!("[{}] Sending participant list to {}: {} users total", peer_addr, username, participant_infos.len());
            socket.write_all(&pkt.encode()?).await?;
            socket.flush().await?;
            debug!("[{}] Sent participant list with voice status ({} users)", peer_addr, participant_infos.len());
        }

        // Broadcast user joined
        let _ = self.broadcast_tx.send(BroadcastEvent::UserJoined { username: username.clone() });

        // === CLIENT LOOP PHASE ===
        let mut broadcast_rx = self.broadcast_tx.subscribe();

        loop {
            tokio::select! {
                result = socket.read(&mut read_buf) => {
                    match result {
                        Ok(0) => {
                            info!("[{}] {} disconnected", peer_addr, username);
                            self.remove_participant(peer_addr, &username).await;
                            break;
                        }
                        Ok(n) => {
                            buffer.extend_from_slice(&read_buf[..n]);

                            // Check for buffer overflow
                            if buffer.len() > MAX_BUFFER_SIZE {
                                error!("[{}] Buffer overflow", peer_addr);
                                self.remove_participant(peer_addr, &username).await;
                                return Err("Buffer overflow".into());
                            }

                            // Try to parse packets from buffer
                            while let Ok((packet, bytes_read)) = TcpPacket::decode(&buffer) {
                                match packet.packet_type {
                                    PacketTypeId::JoinVoiceChannel => {
                                        // No payload needed - server knows which user is on this connection
                                        debug!("[{}] {} requesting to join voice channel", peer_addr, username);

                                        // Update user's voice status
                                        let mut users_lock = self.users.write().await;
                                        if let Some(user) = users_lock.get_mut(&peer_addr) {
                                            user.in_voice = true;
                                        }
                                        drop(users_lock);

                                        debug!("[{}] {} joined voice channel", peer_addr, username);

                                        // Broadcast voice join
                                        let _ = self.broadcast_tx.send(BroadcastEvent::UserJoinedVoice {
                                            username: username.clone()
                                        });
                                    }
                                    PacketTypeId::UserLeftVoice => {
                                        debug!("[{}] {} leaving voice channel", peer_addr, username);

                                        // Update user's voice status
                                        let mut users_lock = self.users.write().await;
                                        if let Some(user) = users_lock.get_mut(&peer_addr) {
                                            user.in_voice = false;
                                        }
                                        drop(users_lock);

                                        let _ = self.broadcast_tx.send(BroadcastEvent::UserLeftVoice {
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
                            self.remove_participant(peer_addr, &username).await;
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
                        Ok(BroadcastEvent::UserJoinedVoice { username: other_user }) => {
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

    /// Remove user from users HashMap and broadcast UserLeft event
    async fn remove_participant(&self, peer_addr: SocketAddr, username: &str) {
        let mut lock = self.users.write().await;
        lock.remove(&peer_addr);
        drop(lock);
        let _ = self.broadcast_tx.send(BroadcastEvent::UserLeft {
            username: username.to_string(),
        });
    }
}