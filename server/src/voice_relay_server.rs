use std::net::SocketAddr;
use tokio::net::UdpSocket;
use tokio::sync::RwLock;
use tracing::{debug, error, info};
use voiceapp_protocol::{
    parse_packet, PacketId, decode_voice_data, encode_voice_data,
    encode_voice_auth_response,
    requests::decode_voice_auth_request
};
use std::collections::HashMap;
use std::sync::Arc;
use crate::management_server::ManagementServer;

/// VoiceRelayServer handles UDP voice packet relaying
/// It depends on ManagementServer for user authentication and state
pub struct VoiceRelayServer {
    management: ManagementServer,
    authenticated_addrs: RwLock<HashMap<SocketAddr, u64>>, // Maps socket address to user_id
}

impl VoiceRelayServer {
    pub fn new(management: ManagementServer) -> Self {
        VoiceRelayServer {
            management,
            authenticated_addrs: RwLock::new(HashMap::new()),
        }
    }

    /// Start listening for UDP voice packets and relay them
    pub async fn run(&self, addr: &str) -> Result<(), Box<dyn std::error::Error>> {
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
        let mut disconnect_rx = self.management.get_disconnect_rx();

        loop {
            tokio::select! {
                // Handle incoming UDP voice packets
                udp_result = udp_socket.recv_from(&mut buf) => {
                    match udp_result {
                        Ok((n, src_addr)) => {
                            self.handle_packet(src_addr, &buf[..n], &udp_socket).await;
                        }
                        Err(e) => {
                            error!("UDP receive error: {}", e);
                        }
                    }
                }

                // Handle user disconnect events from management server
                disconnect_result = disconnect_rx.recv() => {
                    match disconnect_result {
                        Ok(user_id) => { self.handle_voice_disconnect(user_id).await; }
                        Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                            return Ok(());
                        }
                        Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {
                            error!("Disconnect channel lagged, skipping messages");
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
        let user_id = self.authenticated_addrs.read().await.get(&src_addr).copied();

        match parse_packet(packet_data) {
            Ok((packet_id, payload)) => {
                if packet_id == PacketId::VoiceAuthRequest && user_id.is_none() {
                    self.authenticate(src_addr, payload, udp_socket).await;
                } else if packet_id == PacketId::VoiceData && user_id.is_some() {
                    self.forward_voice_packet(user_id.unwrap(), payload, udp_socket).await;
                } else {
                    error!("Received invalid packet id {:?}. User: {:?}", packet_id, user_id);
                }
            }
            Err(e) => {
                error!("Failed to parse packet from {}: {}", src_addr, e);
            }
        }
    }

    /// Forward voice packet to all authenticated addresses except sender
    /// Replaces SSRC with sender's user_id to prevent spoofing
    async fn forward_voice_packet(
        &self,
        sender_user_id: u64,
        payload: &[u8],
        udp_socket: &Arc<UdpSocket>,
    ) {
        // Decode the voice data
        match decode_voice_data(payload) {
            Ok(mut voice_data) => {
                // Replace SSRC with user_id to prevent spoofing
                voice_data.ssrc = sender_user_id;

                // Get list of destination addresses
                let authenticated_addrs = self.authenticated_addrs.read().await;
                let dest_addrs: Vec<_> = authenticated_addrs
                    .iter()
                    .filter(|(_, &uid)| uid != sender_user_id)
                    .map(|(addr, _)| *addr)
                    .collect();
                drop(authenticated_addrs);

                // Encode the modified voice data back
                match encode_voice_data(&voice_data) {
                    Ok(modified_packet) => {
                        // Forward to all authenticated addresses except sender
                        for dest_addr in dest_addrs {
                            if let Err(e) = udp_socket.send_to(&modified_packet, dest_addr).await {
                                error!("Failed to forward voice packet to {}: {}", dest_addr, e);
                            }
                        }
                    }
                    Err(e) => {
                        error!("Failed to encode voice packet: {}", e);
                    }
                }
            }
            Err(e) => {
                error!("Failed to decode voice packet: {}", e);
            }
        }
    }

    /// Authenticate incoming UDP connection from token in auth packet
    async fn authenticate(
        &self,
        src_addr: SocketAddr,
        payload: &[u8],
        udp_socket: &Arc<UdpSocket>,
    ) {
        // Decode the token from auth request
        match decode_voice_auth_request(payload) {
            Ok(token) => {
                debug!("Received auth packet from {}", src_addr);

                // Try to get user_id from token
                let user_id_opt = self.management.get_user_id_by_token(token).await;
                let token_valid = user_id_opt.is_some();

                if token_valid {
                    // Token is valid, authenticate this address with user_id
                    if let Some(user_id) = user_id_opt {
                        let mut authenticated_addrs = self.authenticated_addrs.write().await;
                        authenticated_addrs.insert(src_addr, user_id);
                        debug!("Authenticated voice connection from {} (user_id: {})", src_addr, user_id);
                    }
                } else {
                    error!("Invalid token from {}", src_addr);
                }

                // Send response back to client
                match encode_voice_auth_response(token_valid) {
                    Ok(response_data) => {
                        if let Err(e) = udp_socket.send_to(&response_data, src_addr).await {
                            error!("Failed to send auth response to {}: {}", src_addr, e);
                        } else {
                            debug!("Sent auth response (success={}) to {}", token_valid, src_addr);
                        }
                    }
                    Err(e) => {
                        error!("Failed to encode auth response: {}", e);
                    }
                }
            }
            Err(e) => {
                error!("Failed to decode auth packet from {}: {}", src_addr, e);
            }
        }
    }

    /// Handle user disconnect: remove all authenticated addresses for this user
    async fn handle_voice_disconnect(&self, user_id: u64) {
        let mut authenticated_addrs = self.authenticated_addrs.write().await;
        let removed_count = authenticated_addrs.values().filter(|&&uid| uid == user_id).count();
        authenticated_addrs.retain(|_, &mut uid| uid != user_id);

        if removed_count > 0 {
            debug!("Cleaned up {} UDP voice session(s) for user_id: {}", removed_count, user_id);
        }
    }
}
