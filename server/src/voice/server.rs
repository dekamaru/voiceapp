use std::net::SocketAddr;
use std::sync::Arc;
use dashmap::{DashMap};
use tokio::net::UdpSocket;
use tokio::sync::mpsc::{UnboundedReceiver};
use tracing::{debug, error, info};
use voiceapp_protocol::Packet;
use crate::management::event::Event;
use crate::voice::user::User;

/// VoiceRelayServer handles UDP voice packet relaying
/// It depends on ManagementServer for user authentication and state
pub struct VoiceRelayServer {
    events_channel: UnboundedReceiver<Event>,
    users: DashMap<u64, User>,
    ids_by_addresses: DashMap<SocketAddr, u64>, // Caching map for better performance in relay
}

impl VoiceRelayServer {
    pub fn new(events_channel: UnboundedReceiver<Event>) -> Self {
        VoiceRelayServer {
            events_channel,
            users: DashMap::new(),
            ids_by_addresses: DashMap::new()
        }
    }

    /// Start listening for UDP voice packets and relay them
    pub async fn run(&mut self, addr: &str) -> Result<(), Box<dyn std::error::Error>> {
        let udp_socket = match UdpSocket::bind(addr).await {
            Ok(socket) => {
                info!("VoiceRelayServer listening on {}", socket.local_addr()?);
                Arc::new(socket)
            }
            Err(e) => {
                error!("Failed to bind UDP socket: {}", e);
                return Err(e.into());
            }
        };

        let mut buf = vec![0u8; 4096];

        loop {
            tokio::select! {
                // Handle incoming UDP voice packets
                udp_result = udp_socket.recv_from(&mut buf) => {
                    match udp_result {
                        Ok((n, src_addr)) => {self.handle_packet(src_addr, &buf[..n], &udp_socket).await;}
                        Err(e) => {error!("UDP receive error: {}", e);}
                    }
                }

                // Handle user disconnect events from management server
                Some(event) = self.events_channel.recv() => {
                    match event {
                        Event::UserJoinedServer { id, token } => {
                            info!("Inserting user {} {}", id, token);
                            self.users.insert(id, User {
                                token,
                                in_voice: false,
                                udp_address: None
                            });
                        }
                        Event::UserJoinedVoice { id } => {
                            if let Some(mut user) = self.users.get_mut(&id) {
                                user.in_voice = true;
                            }
                        }
                        Event::UserLeftVoice { id } => {
                            if let Some(mut user) = self.users.get_mut(&id) {
                                user.in_voice = false;
                            }
                        }
                        Event::UserLeftServer { id } => {
                            let address = self.ids_by_addresses
                                .iter()
                                .find(|e| *e.value() == id)
                                .map(|e| *e.key());

                            if let Some(address) = address {
                                self.ids_by_addresses.remove(&address);
                            }

                            self.users.remove(&id);
                        }
                    }
                }
            }
        }
    }

    /// Handle incoming UDP packet - either forward voice data or authenticate
    async fn handle_packet(
        &self,
        src_addr: SocketAddr,
        packet_data: &[u8],
        udp_socket: &Arc<UdpSocket>,
    ) {
        match Packet::decode(packet_data) {
            Ok((packet, _)) => match packet {
                Packet::VoiceAuthRequest { request_id, voice_token } => {
                    let user_data = self
                        .users
                        .iter()
                        .find(|e| e.value().token == voice_token)
                        .map(|e| (*e.key(), *e.value())); // Copy values, drop reference

                    if let Some((user_id, user)) = user_data {
                        self.authenticate(src_addr, user_id, user, request_id, voice_token, udp_socket).await;
                    } else {
                        error!("VoiceAuthRequest received but no user token found");
                    }
                }
                Packet::VoiceData { user_id: _, sequence, timestamp, data } => {
                    let user_id = self.ids_by_addresses.get(&src_addr).map(|e| *e.value());
                    if let Some(user_id) = user_id {
                        self.forward_voice_packet(user_id, sequence, timestamp, data, udp_socket).await;
                    } else {
                        error!("VoiceData received but no user found");
                    }
                }
                _ => { error!("Received invalid packet type. Address: {:?}",src_addr); }
            },
            Err(e) => { error!("Failed to parse packet from {}: {}", src_addr, e); }
        }
    }

    /// Forward voice packet to authenticated addresses of users in voice channel
    /// Replaces user_id with sender's user_id to prevent spoofing
    async fn forward_voice_packet(
        &self,
        user_id: u64,
        sequence: u32,
        timestamp: u32,
        data: Vec<u8>,
        udp_socket: &Arc<UdpSocket>,
    ) {
        let packet = Packet::VoiceData { user_id, sequence, timestamp, data };
        let encoded_packet = packet.encode();

        let recipients: Vec<SocketAddr> = self.users.iter()
            .filter(|e| e.value().in_voice && *e.key() != user_id)
            .map(|e| e.value().udp_address.unwrap())
            .collect();

        for addr in recipients {
            if let Err(e) = udp_socket.send_to(&encoded_packet, addr).await {
                error!("Failed to forward voice packet to {}: {}", addr, e);
            }
        }
    }

    /// Authenticate incoming UDP connection from token in auth packet
    async fn authenticate(
        &self,
        src_addr: SocketAddr,
        user_id: u64,
        mut user: User,
        request_id: u64,
        voice_token: u64,
        udp_socket: &Arc<UdpSocket>,
    ) {
        info!("Received auth packet from {}", src_addr);

        let token_valid = user.token == voice_token;
        if token_valid {
            user.udp_address = Some(src_addr);
            self.users.insert(user_id, user);
            self.ids_by_addresses.insert(src_addr, user_id);
        } else {
            error!("Invalid token from {}", src_addr);
        }

        // Send response back to client
        let response_packet = Packet::VoiceAuthResponse { request_id, success: token_valid };
        let response_data = response_packet.encode();
        if let Err(e) = udp_socket.send_to(&response_data, src_addr).await {
            error!("Failed to send auth response to {}: {}", src_addr, e);
        } else {
            debug!(
                "Sent auth response (success={}) to {}",
                token_valid, src_addr
            );
        }
    }
}
