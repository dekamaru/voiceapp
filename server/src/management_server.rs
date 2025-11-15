use std::net::SocketAddr;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{broadcast, RwLock};
use tracing::{debug, error, info};
use voiceapp_protocol::{
    parse_packet, PacketId, ParticipantInfo,
    decode_login_request, encode_login_response,
    encode_user_joined_server, encode_user_left_server,
    encode_user_joined_voice, encode_user_left_voice,
};
use std::collections::HashMap;
use rand::random;

/// Broadcast message sent to all connected clients
#[derive(Clone, Debug)]
pub struct BroadcastMessage {
    pub sender_addr: Option<SocketAddr>, // None means server broadcast (all receive)
    pub for_all: bool,                    // If true, include sender; if false, exclude sender
    pub packet_data: Vec<u8>,             // Complete encoded packet to forward
}

/// Represents a connected user with their voice channel status and authentication token
#[derive(Clone, Debug)]
pub struct User {
    pub id: u64,
    pub username: String,
    pub in_voice: bool,
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

                            if let Err(e) = self.handle_incoming_request(&mut socket, peer_addr, &read_buf).await {
                                error!("[{}] Error handling incoming request: {}", peer_addr, e);
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
                            return Ok(());
                        }
                    }
                }
            }
        }
    }

    /// Handle incoming request packet from client
    async fn handle_incoming_request(
        &self,
        socket: &mut TcpStream,
        peer_addr: SocketAddr,
        read_buf: &[u8],
    ) -> Result<bool, Box<dyn std::error::Error>> {
        // Try to parse the packet
        match parse_packet(read_buf) {
            Ok((packet_id, payload)) => {
                // Dispatch to appropriate handler based on packet type
                match packet_id {
                    PacketId::LoginRequest => {
                        if let Err(e) = self.handle_login_request(socket, peer_addr, &payload).await {
                            error!("[{}] Failed to handle login request: {}", peer_addr, e);
                        }
                    }
                    PacketId::JoinVoiceChannelRequest => {
                        if let Err(e) = self.handle_join_voice_channel_request(peer_addr).await {
                            error!("[{}] Failed to handle join voice channel request: {}", peer_addr, e);
                        }
                    }
                    PacketId::LeaveVoiceChannelRequest => {
                        if let Err(e) = self.handle_leave_voice_channel_request(peer_addr).await {
                            error!("[{}] Failed to handle leave voice channel request: {}", peer_addr, e);
                        }
                    }
                    _ => {
                        error!("[Management] Unknown packet id {:?}", packet_id);
                    }
                }
            }
            Err(e) => {
                error!("[Management] Failed to parse packet from {}: {}", peer_addr, e);
            }
        }
        Ok(true) // Continue processing
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
        }

        Ok(())
    }

    /// Handle login request: decode username, create user, store in users map, send response
    async fn handle_login_request(
        &self,
        socket: &mut TcpStream,
        peer_addr: SocketAddr,
        payload: &[u8],
    ) -> Result<(), Box<dyn std::error::Error>> {
        // Decode the login request to get username
        let username = decode_login_request(payload)?;

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
                .map(|u| ParticipantInfo {
                    user_id: u.id,
                    in_voice: u.in_voice,
                })
                .collect::<Vec<_>>()
        };

        // Send login response with participant list
        let response_packet = encode_login_response(user_id, voice_token, &participants)?;
        socket.write_all(&response_packet).await?;

        // Broadcast user joined server event to all other clients
        let joined_packet = encode_user_joined_server(user_id, &username_clone)?;
        let broadcast_msg = BroadcastMessage {
            sender_addr: Some(peer_addr),
            for_all: false, // Exclude the sender (new user) from this broadcast
            packet_data: joined_packet,
        };
        
        // Ignore broadcast send errors (they might happen if no subscribers)
        let _ = self.broadcast_tx.send(broadcast_msg);

        debug!("[{}] User logged in: id={}, username={}", peer_addr, user_id, username_clone);

        Ok(())
    }

    /// Handle join voice channel request: update user voice state and broadcast event
    async fn handle_join_voice_channel_request(&self, peer_addr: SocketAddr) -> Result<(), Box<dyn std::error::Error>> {
        // Get user ID and update in_voice state
        let user_id = {
            let mut users_lock = self.users.write().await;
            if let Some(user) = users_lock.get_mut(&peer_addr) {
                let user_id = user.id;
                user.in_voice = true;
                user_id
            } else {
                return Err("User not found in users map".into());
            }
        };

        // Broadcast user joined voice event to all clients
        let joined_voice_packet = encode_user_joined_voice(user_id)?;
        let broadcast_msg = BroadcastMessage {
            sender_addr: None,  // Server-initiated broadcast
            for_all: true,      // Send to all clients
            packet_data: joined_voice_packet,
        };
        let _ = self.broadcast_tx.send(broadcast_msg);

        debug!("[{}] User joined voice channel: id={}", peer_addr, user_id);

        Ok(())
    }

    /// Handle leave voice channel request: update user voice state and broadcast event
    async fn handle_leave_voice_channel_request(&self, peer_addr: SocketAddr) -> Result<(), Box<dyn std::error::Error>> {
        // Get user ID and update in_voice state
        let user_id = {
            let mut users_lock = self.users.write().await;
            if let Some(user) = users_lock.get_mut(&peer_addr) {
                let user_id = user.id;
                user.in_voice = false;
                user_id
            } else {
                return Err("User not found in users map".into());
            }
        };

        // Broadcast user left voice event to all clients
        let left_voice_packet = encode_user_left_voice(user_id)?;
        let broadcast_msg = BroadcastMessage {
            sender_addr: None,  // Server-initiated broadcast
            for_all: true,      // Send to all clients
            packet_data: left_voice_packet,
        };

        let _ = self.broadcast_tx.send(broadcast_msg);
        let _ = self.disconnect_tx.send(user_id); // For UDP

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
            if let Ok(left_packet) = encode_user_left_server(user.id) {
                let broadcast_msg = BroadcastMessage {
                    sender_addr: None, // Server-initiated broadcast
                    for_all: true,     // Send to all clients
                    packet_data: left_packet,
                };
                // Ignore broadcast send errors
                let _ = self.broadcast_tx.send(broadcast_msg);
            }

            // If user was in voice channel, broadcast user left voice event
            if user.in_voice {
                if let Ok(left_voice_packet) = encode_user_left_voice(user.id) {
                    let broadcast_msg = BroadcastMessage {
                        sender_addr: None,
                        for_all: true,
                        packet_data: left_voice_packet,
                    };
                    // Ignore broadcast send errors
                    let _ = self.broadcast_tx.send(broadcast_msg);
                }
            }

            // Notify voice relay server to clean up UDP session for this user
            let _ = self.disconnect_tx.send(user.id);

            debug!("[{}] User disconnected: id={}, username={}", peer_addr, user.id, user.username);
        } else {
            debug!("[{}] User disconnected but was not in users map", peer_addr);
        }
    }
}
