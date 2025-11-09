use std::net::SocketAddr;
use tokio::net::UdpSocket;
use tokio::sync::RwLock;
use tokio::task::JoinHandle;
use tracing::{debug, error, info};
use voiceapp_common::VoicePacket;
use std::collections::HashMap;
use std::sync::Arc;
use crate::user_voice_stream::UserVoiceStreamManager;
use crate::jitter_buffer::JitterBuffer;

/// Receives voice packets over UDP and routes them to user streams
pub struct UdpVoiceReceiver {
    socket: Arc<UdpSocket>,
    manager: Arc<UserVoiceStreamManager>,
    ssrc_map: Arc<RwLock<HashMap<u32, String>>>, // SSRC -> username mapping
    jitter_buffers: Arc<RwLock<HashMap<String, JitterBuffer>>>, // Per-user jitter buffers
}

impl UdpVoiceReceiver {
    /// Create a new UDP voice receiver
    pub async fn new(
        bind_addr: &str,
        manager: Arc<UserVoiceStreamManager>,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let socket = UdpSocket::bind(bind_addr).await?;
        let local_addr = socket.local_addr()?;

        info!("UDP voice receiver listening on {}", local_addr);

        Ok(UdpVoiceReceiver {
            socket: Arc::new(socket),
            manager,
            ssrc_map: Arc::new(RwLock::new(HashMap::new())),
            jitter_buffers: Arc::new(RwLock::new(HashMap::new())),
        })
    }

    /// Register a user with their SSRC
    pub async fn register_user(&self, ssrc: u32, username: String) {
        let mut map = self.ssrc_map.write().await;
        map.insert(ssrc, username.clone());

        // Create a jitter buffer for this user
        let mut buffers = self.jitter_buffers.write().await;
        buffers.insert(username, JitterBuffer::new(30)); // 30 packet buffer
    }

    /// Unregister a user by SSRC
    pub async fn unregister_user(&self, ssrc: u32) {
        let mut map = self.ssrc_map.write().await;
        if let Some(username) = map.remove(&ssrc) {
            // Also remove their jitter buffer
            let mut buffers = self.jitter_buffers.write().await;
            buffers.remove(&username);
        }
    }

    /// Get the SSRC to username mapping
    pub fn get_ssrc_map(&self) -> Arc<RwLock<HashMap<u32, String>>> {
        self.ssrc_map.clone()
    }

    /// Get the local address the receiver is bound to
    pub fn local_addr(&self) -> Result<SocketAddr, std::io::Error> {
        self.socket.local_addr()
    }

    /// Start receiving voice packets in a background task
    pub fn start_receiving(&self) -> JoinHandle<()> {
        // Clone the Arc types which are Send
        let socket = self.socket.clone();
        let manager = self.manager.clone();
        let ssrc_map = self.ssrc_map.clone();
        let jitter_buffers = self.jitter_buffers.clone();

        tokio::spawn(async move {
            let mut buf = vec![0u8; 4096];

            loop {
                match socket.recv_from(&mut buf).await {
                    Ok((n, peer_addr)) => {
                        debug!("Received {} bytes from {}", n, peer_addr);

                        // Try to decode voice packet
                        match VoicePacket::decode(&buf[..n]) {
                            Ok((packet, _)) => {
                                // Look up username from SSRC
                                let map = ssrc_map.read().await;
                                match map.get(&packet.ssrc) {
                                    Some(username) => {
                                        debug!(
                                            "Voice packet from {}: seq={}, ts={}, ssrc={}, frame_size={}",
                                            username,
                                            packet.sequence,
                                            packet.timestamp,
                                            packet.ssrc,
                                            packet.opus_frame.len()
                                        );

                                        // Route packet through jitter buffer
                                        let mut buffers = jitter_buffers.write().await;
                                        if let Some(jb) = buffers.get_mut(username) {
                                            // Insert packet into jitter buffer, get any ready packets
                                            if let Some(ready_packet) = jb.insert(packet) {
                                                drop(buffers); // Release lock before calling process_packet
                                                // Process the ready packet
                                                if let Err(e) = manager.process_packet(username, &ready_packet).await {
                                                    error!("Failed to process packet for {}: {}", username, e);
                                                }
                                            } else {
                                                debug!("Packet buffered for {} (jitter buffer size: {})", username, jb.buffered_count());
                                            }
                                        } else {
                                            debug!("No jitter buffer for user {}, dropping packet", username);
                                        }
                                    }
                                    None => {
                                        debug!(
                                            "Received packet with unknown SSRC {}: seq={}, ts={}",
                                            packet.ssrc, packet.sequence, packet.timestamp
                                        );
                                        // Packet from user not in our stream list
                                    }
                                }
                            }
                            Err(e) => {
                                error!("Failed to decode voice packet: {}", e);
                            }
                        }
                    }
                    Err(e) => {
                        error!("UDP receive error: {}", e);
                        // Continue receiving on error
                    }
                }
            }
        })
    }

    /// Process a voice packet for a specific user
    pub async fn process_packet(&self, username: &str, packet: &VoicePacket) -> Result<(), String> {
        self.manager.process_packet(username, packet).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_udp_receiver_creation() {
        let manager = Arc::new(UserVoiceStreamManager::new());
        let result = UdpVoiceReceiver::new("127.0.0.1:0", manager).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_udp_receiver_bind_address() {
        let manager = Arc::new(UserVoiceStreamManager::new());
        let receiver = UdpVoiceReceiver::new("127.0.0.1:0", manager)
            .await
            .expect("Failed to create receiver");

        let addr = receiver.local_addr().expect("Failed to get local address");
        assert!(addr.ip().is_loopback());
    }
}
