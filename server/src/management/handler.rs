use std::net::SocketAddr;
use std::sync::Arc;
use dashmap::DashMap;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::broadcast;
use tokio::sync::mpsc::UnboundedSender;
use tracing::{debug, error, warn};
use voiceapp_protocol::{Packet, ParticipantInfo, ProtocolError};
use crate::config::{MAX_USERNAME_LEN, PACKET_BUFFER_SIZE};
use crate::error::ServerError;
use crate::management::broadcast::BroadcastMessage;
use crate::event::Event;
use crate::event::Event::{VoiceJoined, VoiceLeft};
use crate::management::user::User;

pub struct UserHandler {
    server_users: Arc<DashMap<SocketAddr, User>>,
    socket: TcpStream,
    address: SocketAddr,
    broadcast_channel: broadcast::Sender<BroadcastMessage>,
    events_channel: UnboundedSender<Event>,
}

impl UserHandler {
    pub fn new(
        users: Arc<DashMap<SocketAddr, User>>,
        socket: TcpStream,
        address: SocketAddr,
        broadcast_channel: broadcast::Sender<BroadcastMessage>,
        events_channel: UnboundedSender<Event>,
    ) -> Self {
        Self { server_users: users, socket, address, broadcast_channel, events_channel }
    }

    pub async fn handle(&mut self) -> Result<(), ServerError> {
        let mut read_buf = vec![0u8; PACKET_BUFFER_SIZE];
        let mut packet_buffer = Vec::new(); // Accumulates partial packets
        let mut broadcast_rx = self.broadcast_channel.subscribe();

        loop {
            tokio::select! {
                // Handle incoming packets from the client
                read_result = self.socket.read(&mut read_buf) => {
                    match read_result {
                        Ok(n) => {
                            if n == 0 {
                                // User disconnected, clean up and exit
                                self.handle_disconnect().await;
                                return Ok(());
                            }

                            // Append new data to packet buffer
                            packet_buffer.extend_from_slice(&read_buf[..n]);

                            // Process all complete packets in the buffer
                            loop {
                                match Packet::decode(&packet_buffer) {
                                    Ok((packet, size)) => {
                                        // Handle the packet
                                        if let Err(e) = self.handle_packet(packet).await {
                                            error!("[{}] Error handling packet: {}", self.address, e);
                                        }

                                        // Remove consumed bytes from buffer
                                        packet_buffer.drain(..size);
                                    }
                                    Err(ProtocolError::IncompletePayload { .. }) | Err(ProtocolError::PacketTooShort { .. }) => {
                                        // Not enough data yet, wait for more
                                        break;
                                    }
                                    Err(e) => {
                                        warn!("[{}] Protocol error: {}", self.address, e);
                                        packet_buffer.clear();
                                        break;
                                    }
                                }
                            }
                        }
                        Err(e) => {
                            error!("[{}] TCP receive error: {}", self.address, e);
                            self.handle_disconnect().await;
                            return Ok(());
                        }
                    }
                }

                // Handle broadcast messages
                broadcast_result = broadcast_rx.recv() => {
                    match broadcast_result {
                        Ok(message) => {
                            if let Err(e) = self.handle_broadcast_message(message).await {
                                error!("[{}] Failed to send broadcast message: {}", self.address, e);
                                return Ok(());
                            }
                        }
                        Err(broadcast::error::RecvError::Lagged(_)) => {
                            warn!("[{}] Broadcast channel lagged", self.address);
                        }
                        Err(broadcast::error::RecvError::Closed) => {
                            error!("[{}] Broadcast channel closed, skipping messages", self.address);
                            return Ok(());
                        }
                    }
                }
            }
        }
    }

    /// Handle a decoded packet from client
    async fn handle_packet(&mut self, packet: Packet) -> Result<(), ServerError> {
        match packet {
            Packet::LoginRequest { request_id, username } => {
                self.handle_login_request(request_id, username).await
            }
            Packet::JoinVoiceChannelRequest { request_id } => {
                self.handle_join_voice_channel_request(request_id).await
            }
            Packet::LeaveVoiceChannelRequest { request_id } => {
                self.handle_leave_voice_channel_request(request_id).await
            }
            Packet::ChatMessageRequest { request_id, message } => {
                self.handle_chat_message_request(request_id, message).await
            }
            Packet::PingRequest { request_id } => {
                self.handle_ping_request(request_id).await
            }
            Packet::UserMuteState { user_id, is_muted } => {
                self.handle_user_mute_state(user_id, is_muted).await
            }
            _ => {
                warn!("[{}] Unexpected packet: {:?}", self.address, packet);
                Ok(())
            }
        }
    }

    /// Handle broadcast message: filter and send to client if appropriate
    async fn handle_broadcast_message(
        &mut self,
        message: BroadcastMessage,
    ) -> Result<(), ServerError> {
        if message.should_send_to(self.address) {
            self.socket.write_all(message.data()).await?;
            self.socket.flush().await?;
        }

        Ok(())
    }

    /// Handle login request: create user, store in users map, send response
    async fn handle_login_request(
        &mut self,
        request_id: u64,
        username: String,
    ) -> Result<(), ServerError> {
        // Validate username
        if username.is_empty() || username.len() > MAX_USERNAME_LEN {
            let response = Packet::LoginResponse {
                request_id,
                id: 0,
                voice_token: 0,
                participants: vec![],
            };
            self.socket.write_all(&response.encode()).await?;
            self.socket.flush().await?;
            return Ok(());
        }

        let (user_id, voice_token) = if let Some(mut user) = self.server_users.get_mut(&self.address) {
            user.username = Some(username.clone());
            (user.id, user.token)
        } else {
            return Err(ServerError::UserNotFound(self.address));
        };

        // Collect current participants for login response
        let participants = self
            .server_users
            .iter()
            .filter(|entry| entry.value().username.is_some())
            .map(|entry| {
                let u = entry.value();
                ParticipantInfo::new(u.id, u.username.clone().unwrap(), u.in_voice, u.is_muted)
            })
            .collect::<Vec<_>>();

        // Send login response with participant list
        let response = Packet::LoginResponse {
            request_id,
            id: user_id,
            voice_token,
            participants,
        };

        self.socket.write_all(&response.encode()).await?;
        self.socket.flush().await?;

        // Broadcast user joined server event to all other clients
        let joined_event = Packet::UserJoinedServer {
            participant: ParticipantInfo::new(user_id, username.clone(), false, false),
        };
        let _ = self.broadcast_channel.send(BroadcastMessage::excluding(self.address, &joined_event));

        debug!(
            "[{}] User logged in: id={}, username={}",
            self.address, user_id, username.clone()
        );

        Ok(())
    }

    /// Handle join voice channel request: send response to caller and broadcast event excluding caller
    async fn handle_join_voice_channel_request(
        &mut self,
        request_id: u64,
    ) -> Result<(), ServerError> {
        // Get user ID and update in_voice state
        let user_id = {
            if let Some(mut user) = self.server_users.get_mut(&self.address) {
                let user_id = user.id;
                user.in_voice = true;
                user.is_muted = false; // by default user is not muted
                user_id
            } else {
                return Err(ServerError::UserNotFound(self.address));
            }
        };

        // Send response to caller
        let response = Packet::JoinVoiceChannelResponse { request_id, success: true };
        self.socket.write_all(&response.encode()).await?;
        self.socket.flush().await?;

        let _ = self.events_channel.send(VoiceJoined { id: user_id });

        // Broadcast user joined voice event to all other clients (exclude caller)
        let joined_event = Packet::UserJoinedVoice { user_id };
        let _ = self.broadcast_channel.send(BroadcastMessage::excluding(self.address, &joined_event));

        debug!("[{}] User joined voice channel: id={}", self.address, user_id);

        Ok(())
    }

    /// Handle leave voice channel request: send response to caller and broadcast event excluding caller
    async fn handle_leave_voice_channel_request(
        &mut self,
        request_id: u64,
    ) -> Result<(), ServerError> {
        // Get user ID and update in_voice state
        let user_id = {
            if let Some(mut user) = self.server_users.get_mut(&self.address) {
                let user_id = user.id;
                user.in_voice = false;
                user.is_muted = false;
                user_id
            } else {
                return Err(ServerError::UserNotFound(self.address));
            }
        };

        // Send response to caller
        let response = Packet::LeaveVoiceChannelResponse { request_id, success: true };
        self.socket.write_all(&response.encode()).await?;
        self.socket.flush().await?;

        let _ = self.events_channel.send(VoiceLeft { id: user_id });

        // Broadcast user left voice event to all other clients (exclude caller)
        let left_event = Packet::UserLeftVoice { user_id };
        let _ = self.broadcast_channel.send(BroadcastMessage::excluding(self.address, &left_event));

        debug!("[{}] User left voice channel: id={}", self.address, user_id);

        Ok(())
    }

    /// Handle chat message request: send response to caller and broadcast event to all clients
    async fn handle_chat_message_request(
        &mut self,
        request_id: u64,
        message: String,
    ) -> Result<(), ServerError> {
        // Get user info
        let user_id = {
            if let Some(user) = self.server_users.get(&self.address) {
                user.id
            } else {
                return Err(ServerError::UserNotFound(self.address));
            }
        };

        // Send response to caller with success status
        let response = Packet::ChatMessageResponse { request_id, success: true };
        self.socket.write_all(&response.encode()).await?;
        self.socket.flush().await?;

        // Broadcast user sent message event to all clients (including sender)
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);
        let message_event = Packet::UserSentMessage {
            user_id,
            timestamp,
            message: message.clone(),
        };
        let _ = self.broadcast_channel.send(BroadcastMessage::to_all(&message_event));

        debug!(
            "[{}] User sent message: id={}, len={}",
            self.address,
            user_id,
            message.len()
        );

        Ok(())
    }

    /// Handle ping request: immediately respond with PingResponse
    async fn handle_ping_request(&mut self, request_id: u64) -> Result<(), ServerError> {
        let response = Packet::PingResponse { request_id };
        self.socket.write_all(&response.encode()).await?;
        self.socket.flush().await?;
        Ok(())
    }

    /// Handle user mute state event: broadcast to all clients excluding sender
    async fn handle_user_mute_state(
        &mut self,
        user_id: u64,
        is_muted: bool,
    ) -> Result<(), ServerError> {
        // Update user's mute state
        {
            if let Some(mut user) = self.server_users.get_mut(&self.address) {
                user.is_muted = is_muted;
            } else {
                return Err(ServerError::UserNotFound(self.address));
            }
        }

        // Broadcast user mute state event to all clients (excluding sender)
        let mute_event = Packet::UserMuteState { user_id, is_muted };
        let _ = self.broadcast_channel.send(BroadcastMessage::excluding(self.address, &mute_event));

        debug!(
            "[{}] User mute state changed: id={}, is_muted={}",
            self.address, user_id, is_muted
        );

        Ok(())
    }

    /// Handle user disconnection: remove from users map and broadcast left server event
    async fn handle_disconnect(&mut self) {
        // Remove user from the users DashMap
        let user_option = self.server_users.remove(&self.address).map(|(_, user)| user);

        // If user was found, broadcast the disconnection and log
        if let Some(user) = user_option {
            // Broadcast user left server event to all clients
            let left_event = Packet::UserLeftServer { user_id: user.id };
            let _ = self.broadcast_channel.send(BroadcastMessage::to_all(&left_event));

            // If user was in voice channel, broadcast user left voice event
            if user.in_voice {
                let left_voice_event = Packet::UserLeftVoice { user_id: user.id };
                let _ = self.broadcast_channel.send(BroadcastMessage::to_all(&left_voice_event));
            }

            debug!(
                "[{}] User disconnected: id={}, username={}",
                self.address, user.id, user.username.unwrap_or("None".to_string())
            );
        }
    }
}