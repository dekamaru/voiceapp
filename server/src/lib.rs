use std::net::SocketAddr;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{broadcast, RwLock};
use tracing::{debug, error, info};
use voiceapp_common::{TcpPacket, PacketTypeId, encode_username, decode_username, encode_participant_list_with_voice, ParticipantInfo};

const MAX_BUFFER_SIZE: usize = 65536; // Prevent memory exhaustion attacks

type BroadcastSender = broadcast::Sender<BroadcastEvent>;

#[derive(Clone, Debug)]
enum BroadcastEvent {
    UserJoined { username: String },
    UserLeft { username: String },
    UserJoinedVoice { username: String },
    UserLeftVoice { username: String },
}

pub struct Server {
    broadcast_tx: BroadcastSender,
    participants: Arc<RwLock<Vec<String>>>,
    voice_channel_members: Arc<RwLock<Vec<String>>>, // Users currently in voice channel
}

impl Server {
    pub fn new() -> Self {
        let (tx, _rx) = broadcast::channel(100);
        Server {
            broadcast_tx: tx,
            participants: Arc::new(RwLock::new(Vec::new())),
            voice_channel_members: Arc::new(RwLock::new(Vec::new())),
        }
    }

    pub async fn run(&self, addr: &str) -> Result<(), Box<dyn std::error::Error>> {
        let listener = TcpListener::bind(addr).await?;
        let local_addr = listener.local_addr()?;
        info!("Server listening on {}", local_addr);

        loop {
            let (socket, peer_addr) = listener.accept().await?;
            info!("[{}] New connection", peer_addr);

            let broadcast_tx = self.broadcast_tx.clone();
            let participants = self.participants.clone();
            let voice_channel_members = self.voice_channel_members.clone();
            tokio::spawn(async move {
                if let Err(e) = handle_client(socket, broadcast_tx, participants, voice_channel_members, peer_addr).await {
                    error!("[{}] Error: {}", peer_addr, e);
                }
            });
        }
    }

    pub async fn bind(&self, addr: &str) -> Result<SocketAddr, Box<dyn std::error::Error>> {
        let listener = TcpListener::bind(addr).await?;
        let local_addr = listener.local_addr()?;
        info!("Server listening on {}", local_addr);

        // Start accepting in background
        let broadcast_tx = self.broadcast_tx.clone();
        let participants = self.participants.clone();
        let voice_channel_members = self.voice_channel_members.clone();
        tokio::spawn(async move {
            loop {
                match listener.accept().await {
                    Ok((socket, peer_addr)) => {
                        info!("[{}] New connection", peer_addr);
                        let broadcast_tx = broadcast_tx.clone();
                        let participants = participants.clone();
                        let voice_channel_members = voice_channel_members.clone();
                        tokio::spawn(async move {
                            if let Err(e) = handle_client(socket, broadcast_tx, participants, voice_channel_members, peer_addr).await {
                                error!("[{}] Error: {}", peer_addr, e);
                            }
                        });
                    }
                    Err(e) => {
                        error!("Accept error: {}", e);
                        break;
                    }
                }
            }
        });

        Ok(local_addr)
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

/// Remove user from participants list and broadcast UserLeft event
async fn remove_participant(
    participants: &Arc<RwLock<Vec<String>>>,
    broadcast_tx: &BroadcastSender,
    username: &str,
) {
    let mut lock = participants.write().await;
    lock.retain(|u| u != username);
    drop(lock);
    let _ = broadcast_tx.send(BroadcastEvent::UserLeft {
        username: username.to_string(),
    });
}

async fn handle_client(
    mut socket: TcpStream,
    broadcast_tx: BroadcastSender,
    participants: Arc<RwLock<Vec<String>>>,
    voice_channel_members: Arc<RwLock<Vec<String>>>,
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

    // Add user to participants
    {
        let mut lock = participants.write().await;
        lock.push(username.clone());
    }

    // Send ServerParticipantList with voice status to the new user
    {
        let participants_lock = participants.read().await;
        let voice_lock = voice_channel_members.read().await;
        let participant_infos: Vec<ParticipantInfo> = participants_lock
            .iter()
            .map(|u| ParticipantInfo {
                username: u.clone(),
                in_voice: voice_lock.contains(u),
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
                        remove_participant(&participants, &broadcast_tx, &username).await;
                        break;
                    }
                    Ok(n) => {
                        buffer.extend_from_slice(&read_buf[..n]);

                        // Check for buffer overflow
                        if buffer.len() > MAX_BUFFER_SIZE {
                            error!("[{}] Buffer overflow", peer_addr);
                            remove_participant(&participants, &broadcast_tx, &username).await;
                            return Err("Buffer overflow".into());
                        }

                        // Try to parse packets from buffer
                        while let Ok((packet, bytes_read)) = TcpPacket::decode(&buffer) {
                            match packet.packet_type {
                                PacketTypeId::JoinVoiceChannel => {
                                    debug!("[{}] {} joining voice channel", peer_addr, username);
                                    let mut members = voice_channel_members.write().await;
                                    if !members.contains(&username) {
                                        members.push(username.clone());
                                    }
                                    drop(members);
                                    let _ = broadcast_tx.send(BroadcastEvent::UserJoinedVoice {
                                        username: username.clone()
                                    });
                                }
                                PacketTypeId::UserLeftVoice => {
                                    debug!("[{}] {} leaving voice channel", peer_addr, username);
                                    let mut members = voice_channel_members.write().await;
                                    members.retain(|u| u != &username);
                                    drop(members);
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
                        remove_participant(&participants, &broadcast_tx, &username).await;
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
                        // Broadcast to all participants so they can update UI
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