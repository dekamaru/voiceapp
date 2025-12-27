use rand::random;
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{broadcast, RwLock};
use tracing::{debug, error, info};
use voiceapp_protocol::{Packet, ParticipantInfo, ProtocolError};

/// Broadcast message sent to all connected clients
#[derive(Clone, Debug)]
pub struct BroadcastMessage {
    pub sender_addr: Option<SocketAddr>, // None means server broadcast (all receive)
    pub for_all: bool,                   // If true, include sender; if false, exclude sender
    pub packet_data: Vec<u8>,            // Complete encoded packet to forward
}

/// Represents a connected user with their voice channel status and authentication token
#[derive(Clone, Debug)]
pub struct User {
    pub id: u64,
    pub username: String,
    pub in_voice: bool,
    pub is_muted: bool,
    pub token: u64, // Authentication token for UDP connections
}

/// ManagementServer handles TCP connections, user login, presence management,
/// and broadcasts events to all connected clients
#[derive(Clone)]
pub struct ManagementServer {
    pub users: Arc<RwLock<HashMap<SocketAddr, User>>>,
    next_user_id: Arc<RwLock<u64>>,
    broadcast_tx: Arc<broadcast::Sender<BroadcastMessage>>,
    disconnect_tx: Arc<broadcast::Sender<u64>>,
}

impl ManagementServer {
    pub fn new() -> Self {
        let (broadcast_tx, _) = broadcast::channel(100);
        let (disconnect_tx, _) = broadcast::channel(100);

        ManagementServer {
            users: Arc::new(RwLock::new(HashMap::new())),
            next_user_id: Arc::new(RwLock::new(1)),
            broadcast_tx: Arc::new(broadcast_tx),
            disconnect_tx: Arc::new(disconnect_tx),
        }
    }

    /// Get user ID by token, returns None if token is invalid
    pub async fn get_user_id_by_token(&self, token: u64) -> Option<u64> {
        let users_lock = self.users.read().await;
        users_lock
            .values()
            .find(|user| user.token == token)
            .map(|user| user.id)
    }

    /// Check if a user is in the voice channel
    /// TODO: not optimised lookup
    pub async fn is_user_in_voice(&self, user_id: u64) -> bool {
        let users_lock = self.users.read().await;
        users_lock
            .values()
            .find(|user| user.id == user_id)
            .map(|user| user.in_voice)
            .unwrap_or(false)
    }

    /// Get a receiver for disconnect events (broadcasts user_id when user disconnects)
    pub fn get_disconnect_rx(&self) -> broadcast::Receiver<u64> {
        self.disconnect_tx.subscribe()
    }

    /// Start the TCP listener and accept client connections
    pub async fn run(&self, addr: &str) -> Result<(), Box<dyn std::error::Error>> {
        let listener = TcpListener::bind(addr).await?;
        let local_addr = listener.local_addr()?;
        info!("ManagementServer listening on {}", local_addr);

        loop {
            let (socket, peer_addr) = listener.accept().await?;
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
        let mut read_buf = vec![0u8; 4096];
        let mut packet_buffer = Vec::new(); // Accumulates partial packets
        let mut broadcast_rx = self.broadcast_tx.subscribe();

        loop {
            tokio::select! {
                // Handle incoming packets from the client
                read_result = socket.read(&mut read_buf) => {
                    match read_result {
                        Ok(n) => {
                            if n == 0 {
                                // User disconnected, clean up and exit
                                self.handle_user_disconnect(peer_addr).await;
                                return Ok(());
                            }

                            // Append new data to packet buffer
                            packet_buffer.extend_from_slice(&read_buf[..n]);

                            // Process all complete packets in the buffer
                            loop {
                                match Packet::decode(&packet_buffer) {
                                    Ok((packet, size)) => {
                                        // Handle the packet
                                        if let Err(e) = self.handle_packet(&mut socket, peer_addr, packet).await {
                                            error!("[{}] Error handling packet: {}", peer_addr, e);
                                        }

                                        // Remove consumed bytes from buffer
                                        packet_buffer.drain(..size);
                                    }
                                    Err(ProtocolError::IncompletePayload { .. }) | Err(ProtocolError::PacketTooShort { .. }) => {
                                        // Not enough data yet, wait for more
                                        break;
                                    }
                                    Err(e) => {
                                        error!("[{}] Protocol error: {}, clearing buffer", peer_addr, e);
                                        packet_buffer.clear();
                                        break;
                                    }
                                }
                            }
                        }
                        Err(e) => {
                            error!("[{}] TCP receive error: {}", peer_addr, e);
                            self.handle_user_disconnect(peer_addr).await;
                            return Ok(());
                        }
                    }
                }

                // Handle broadcast messages
                broadcast_result = broadcast_rx.recv() => {
                    match broadcast_result {
                        Ok(message) => {
                            if let Err(e) = self.handle_broadcast_message(&mut socket, peer_addr, message).await {
                                error!("[{}] Failed to send broadcast message: {}", peer_addr, e);
                                return Ok(());
                            }
                        }
                        Err(broadcast::error::RecvError::Lagged(_)) => {
                            error!("[{}] Broadcast channel lagged, skipping messages", peer_addr);
                        }
                        Err(broadcast::error::RecvError::Closed) => {
                            error!("[{}] Broadcast channel closed, skipping messages", peer_addr);
                            return Ok(());
                        }
                    }
                }
            }
        }
    }

    /// Handle a decoded packet from client
    async fn handle_packet(
        &self,
        socket: &mut TcpStream,
        peer_addr: SocketAddr,
        packet: Packet,
    ) -> Result<(), Box<dyn std::error::Error>> {
        match packet {
            Packet::LoginRequest { request_id, username } => {
                self.handle_login_request(socket, peer_addr, request_id, username).await
            }
            Packet::JoinVoiceChannelRequest { request_id } => {
                self.handle_join_voice_channel_request(socket, peer_addr, request_id).await
            }
            Packet::LeaveVoiceChannelRequest { request_id } => {
                self.handle_leave_voice_channel_request(socket, peer_addr, request_id).await
            }
            Packet::ChatMessageRequest { request_id, message } => {
                self.handle_chat_message_request(socket, peer_addr, request_id, message).await
            }
            Packet::UserMuteState { user_id, is_muted } => {
                self.handle_user_mute_state(peer_addr, user_id, is_muted).await
            }
            _ => {
                error!("[{}] Unexpected packet type: {:?}", peer_addr, packet);
                Ok(())
            }
        }
    }

    /// Handle broadcast message: filter and send to client if appropriate
    async fn handle_broadcast_message(
        &self,
        socket: &mut TcpStream,
        peer_addr: SocketAddr,
        message: BroadcastMessage,
    ) -> Result<(), Box<dyn std::error::Error>> {
        // Check if we should send this message to this client
        let should_send = match message.sender_addr {
            Some(sender) => {
                // If for_all is false, skip if this is the sender
                message.for_all || sender != peer_addr
            }
            None => true, // Server broadcasts go to everyone
        };

        if should_send {
            socket.write_all(&message.packet_data).await?;
            socket.flush().await?;
        }

        Ok(())
    }

    /// Handle login request: create user, store in users map, send response
    async fn handle_login_request(
        &self,
        socket: &mut TcpStream,
        peer_addr: SocketAddr,
        request_id: u64,
        username: String,
    ) -> Result<(), Box<dyn std::error::Error>> {

        // Generate new user ID
        let user_id = {
            let mut id_lock = self.next_user_id.write().await;
            let current_id = *id_lock;
            *id_lock = current_id + 1;
            current_id
        };

        let voice_token = random::<u64>();

        // Create and store user
        let username_clone = username.clone();
        let user = User {
            id: user_id,
            username,
            in_voice: false,
            is_muted: false,
            token: voice_token,
        };

        {
            let mut users_lock = self.users.write().await;
            users_lock.insert(peer_addr, user);
        }

        // Collect current participants for login response
        let participants = {
            let users_lock = self.users.read().await;
            users_lock
                .values()
                .map(|u| ParticipantInfo::new(u.id, u.username.clone(), u.in_voice, u.is_muted))
                .collect::<Vec<_>>()
        };

        // Send login response with participant list
        let response = Packet::LoginResponse {
            request_id,
            id: user_id,
            voice_token,
            participants,
        };
        socket.write_all(&response.encode()).await?;
        socket.flush().await?;

        // Broadcast user joined server event to all other clients
        let joined_event = Packet::UserJoinedServer {
            participant: ParticipantInfo::new(user_id, username_clone.clone(), false, false),
        };
        let broadcast_msg = BroadcastMessage {
            sender_addr: Some(peer_addr),
            for_all: false,
            packet_data: joined_event.encode(),
        };

        // Ignore broadcast send errors (they might happen if no subscribers)
        let _ = self.broadcast_tx.send(broadcast_msg);

        debug!(
            "[{}] User logged in: id={}, username={}",
            peer_addr, user_id, username_clone
        );

        Ok(())
    }

    /// Handle join voice channel request: send response to caller and broadcast event excluding caller
    async fn handle_join_voice_channel_request(
        &self,
        socket: &mut TcpStream,
        peer_addr: SocketAddr,
        request_id: u64,
    ) -> Result<(), Box<dyn std::error::Error>> {
        // Get user ID and update in_voice state
        let user_id = {
            let mut users_lock = self.users.write().await;
            if let Some(user) = users_lock.get_mut(&peer_addr) {
                let user_id = user.id;
                user.in_voice = true;
                user.is_muted = false; // by default user is not muted
                user_id
            } else {
                return Err("User not found in users map".into());
            }
        };

        // Send response to caller
        let response = Packet::JoinVoiceChannelResponse { request_id, success: true };
        socket.write_all(&response.encode()).await?;
        socket.flush().await?;

        // Broadcast user joined voice event to all other clients (exclude caller)
        let joined_event = Packet::UserJoinedVoice { user_id };
        let broadcast_msg = BroadcastMessage {
            sender_addr: Some(peer_addr),
            for_all: false,
            packet_data: joined_event.encode(),
        };
        let _ = self.broadcast_tx.send(broadcast_msg);

        debug!("[{}] User joined voice channel: id={}", peer_addr, user_id);

        Ok(())
    }

    /// Handle leave voice channel request: send response to caller and broadcast event excluding caller
    async fn handle_leave_voice_channel_request(
        &self,
        socket: &mut TcpStream,
        peer_addr: SocketAddr,
        request_id: u64,
    ) -> Result<(), Box<dyn std::error::Error>> {
        // Get user ID and update in_voice state
        let user_id = {
            let mut users_lock = self.users.write().await;
            if let Some(user) = users_lock.get_mut(&peer_addr) {
                let user_id = user.id;
                user.in_voice = false;
                user.is_muted = false;
                user_id
            } else {
                return Err("User not found in users map".into());
            }
        };

        // Send response to caller
        let response = Packet::LeaveVoiceChannelResponse { request_id, success: true };
        socket.write_all(&response.encode()).await?;
        socket.flush().await?;

        // Broadcast user left voice event to all other clients (exclude caller)
        let left_event = Packet::UserLeftVoice { user_id };
        let broadcast_msg = BroadcastMessage {
            sender_addr: Some(peer_addr),
            for_all: false,
            packet_data: left_event.encode(),
        };

        let _ = self.broadcast_tx.send(broadcast_msg);

        debug!("[{}] User left voice channel: id={}", peer_addr, user_id);

        Ok(())
    }

    /// Handle user disconnection: remove from users map and broadcast left server event
    async fn handle_user_disconnect(&self, peer_addr: SocketAddr) {
        // Remove user from the users HashMap
        let user_option = {
            let mut users_lock = self.users.write().await;
            users_lock.remove(&peer_addr)
        };

        // If user was found, broadcast the disconnection and log
        if let Some(user) = user_option {
            // Broadcast user left server event to all clients
            let left_event = Packet::UserLeftServer { user_id: user.id };
            let broadcast_msg = BroadcastMessage {
                sender_addr: None,
                for_all: true,
                packet_data: left_event.encode(),
            };
            let _ = self.broadcast_tx.send(broadcast_msg);

            // If user was in voice channel, broadcast user left voice event
            if user.in_voice {
                let left_voice_event = Packet::UserLeftVoice { user_id: user.id };
                let broadcast_msg = BroadcastMessage {
                    sender_addr: None,
                    for_all: true,
                    packet_data: left_voice_event.encode(),
                };
                let _ = self.broadcast_tx.send(broadcast_msg);
            }

            // Notify voice relay server to clean up UDP session for this user
            let _ = self.disconnect_tx.send(user.id);

            debug!(
                "[{}] User disconnected: id={}, username={}",
                peer_addr, user.id, user.username
            );
        } else {
            debug!("[{}] User disconnected but was not in users map", peer_addr);
        }
    }

    /// Handle chat message request: send response to caller and broadcast event to all clients
    async fn handle_chat_message_request(
        &self,
        socket: &mut TcpStream,
        peer_addr: SocketAddr,
        request_id: u64,
        message: String,
    ) -> Result<(), Box<dyn std::error::Error>> {
        // Get user info
        let user_id = {
            let users_lock = self.users.read().await;
            if let Some(user) = users_lock.get(&peer_addr) {
                user.id
            } else {
                return Err("User not found in users map".into());
            }
        };

        // Send response to caller with success status
        let response = Packet::ChatMessageResponse { request_id, success: true };
        socket.write_all(&response.encode()).await?;
        socket.flush().await?;

        // Broadcast user sent message event to all clients (including sender)
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;
        let message_event = Packet::UserSentMessage {
            user_id,
            timestamp,
            message: message.clone(),
        };
        let broadcast_msg = BroadcastMessage {
            sender_addr: None,
            for_all: true,
            packet_data: message_event.encode(),
        };
        let _ = self.broadcast_tx.send(broadcast_msg);

        debug!(
            "[{}] User sent message: id={}, len={}",
            peer_addr,
            user_id,
            message.len()
        );

        Ok(())
    }

    /// Handle user mute state event: broadcast to all clients excluding sender
    async fn handle_user_mute_state(
        &self,
        peer_addr: SocketAddr,
        user_id: u64,
        is_muted: bool,
    ) -> Result<(), Box<dyn std::error::Error>> {
        // Update user's mute state
        {
            let mut users_lock = self.users.write().await;
            if let Some(user) = users_lock.get_mut(&peer_addr) {
                user.is_muted = is_muted;
            } else {
                return Err("User not found in users map".into());
            }
        }

        // Broadcast user mute state event to all clients (excluding sender)
        let mute_event = Packet::UserMuteState { user_id, is_muted };
        let broadcast_msg = BroadcastMessage {
            sender_addr: Some(peer_addr),
            for_all: false,
            packet_data: mute_event.encode(),
        };
        let _ = self.broadcast_tx.send(broadcast_msg);

        debug!(
            "[{}] User mute state changed: id={}, is_muted={}",
            peer_addr, user_id, is_muted
        );

        Ok(())
    }
}
