use std::net::SocketAddr;
use tokio::net::UdpSocket;
use tokio::sync::RwLock;
use tracing::{debug, error, info};
use voiceapp_common::{VoicePacket, UdpAuthPacket, UdpAuthResponse};
use std::collections::HashSet;
use std::sync::Arc;

use crate::management_server::ManagementServer;

/// VoiceRelayServer handles UDP voice packet relaying
/// It depends on ManagementServer for user authentication and state
pub struct VoiceRelayServer {
    management: ManagementServer,
    authenticated_addrs: RwLock<HashSet<SocketAddr>>,
}

impl VoiceRelayServer {
    pub fn new(management: ManagementServer) -> Self {
        VoiceRelayServer {
            management,
            authenticated_addrs: RwLock::new(HashSet::new()),
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

        loop {
            match udp_socket.recv_from(&mut buf).await {
                Ok((n, src_addr)) => {
                    self.handle_packet(src_addr, &buf[..n], &udp_socket).await;
                }
                Err(e) => {
                    error!("UDP receive error: {}", e);
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
        let is_authenticated = {
            let authenticated_addrs = self.authenticated_addrs.read().await;
            authenticated_addrs.contains(&src_addr)
        };

        if is_authenticated {
            // Address is already authenticated - try to forward voice packet
            self.forward_voice_packet(src_addr, packet_data, udp_socket).await;
        } else {
            // Unauthenticated address - try to authenticate
            self.authenticate(src_addr, packet_data, udp_socket).await;
        }
    }

    /// Forward voice packet to all authenticated addresses except sender
    async fn forward_voice_packet(
        &self,
        src_addr: SocketAddr,
        packet_data: &[u8],
        udp_socket: &Arc<UdpSocket>,
    ) {
        match VoicePacket::decode(packet_data) {
            Ok(_) => {
                // Forward to all authenticated addresses except sender
                let authenticated_addrs = self.authenticated_addrs.read().await;
                let dest_addrs: Vec<_> = authenticated_addrs.iter().copied().collect();
                drop(authenticated_addrs);

                for dest_addr in dest_addrs {
                    if dest_addr != src_addr {
                        if let Err(e) = udp_socket.send_to(packet_data, dest_addr).await {
                            error!("Failed to forward voice packet to {}: {}", dest_addr, e);
                        }
                    }
                }
            }
            Err(e) => {
                error!("Failed to decode voice packet from {}: {}", src_addr, e);
            }
        }
    }

    /// Authenticate incoming UDP connection from token in auth packet
    async fn authenticate(
        &self,
        src_addr: SocketAddr,
        packet_data: &[u8],
        udp_socket: &Arc<UdpSocket>,
    ) {
        match UdpAuthPacket::decode(packet_data) {
            Ok((auth_packet, _)) => {
                debug!("Received auth packet from {}", src_addr);

                // Validate token
                let token_valid = self.management.is_token_valid(auth_packet.token).await;

                if token_valid {
                    // Token is valid, authenticate this address
                    let mut authenticated_addrs = self.authenticated_addrs.write().await;
                    authenticated_addrs.insert(src_addr);
                    debug!("Authenticated voice connection from {}", src_addr);
                } else {
                    error!("Invalid token from {}", src_addr);
                }

                // Send response back to client
                let response = UdpAuthResponse::new(token_valid);
                if let Ok(response_data) = response.encode() {
                    if let Err(e) = udp_socket.send_to(&response_data, src_addr).await {
                        error!("Failed to send auth response to {}: {}", src_addr, e);
                    } else {
                        debug!("Sent auth response (success={}) to {}", token_valid, src_addr);
                    }
                }
            }
            Err(_) => {
                debug!("Received packet from {} that is neither auth nor voice", src_addr);
            }
        }
    }
}
