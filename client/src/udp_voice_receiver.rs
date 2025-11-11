use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use tokio::net::UdpSocket;
use tokio::sync::RwLock;
use tokio::task::JoinHandle;
use tracing::{debug, error, info};
use voiceapp_common::{VoicePacket, UdpAuthResponse};
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
    auth_success: Arc<AtomicBool>, // Auth result status
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
            auth_success: Arc::new(AtomicBool::new(false)),
        })
    }

    /// Register a user with their SSRC
    pub async fn register_user(&self, ssrc: u32, username: String) {
        let mut map = self.ssrc_map.write().await;
        map.insert(ssrc, username.clone());
        debug!("Registered user '{}' with SSRC {}", username, ssrc);

        // Create a jitter buffer for this user
        let mut buffers = self.jitter_buffers.write().await;
        buffers.insert(username.clone(), JitterBuffer::new(20));
        debug!("Created jitter buffer for user '{}'", username);
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

    /// Get the local address the receiver is bound to
    pub fn local_addr(&self) -> Result<SocketAddr, std::io::Error> {
        self.socket.local_addr()
    }

    /// Send raw data to the server from the receiver's socket
    /// This ensures packets come from the listening port
    pub async fn send_to(&self, data: &[u8], server_addr: &str) -> Result<(), Box<dyn std::error::Error>> {
        let server_addr: SocketAddr = server_addr.parse()?;
        self.socket.send_to(data, server_addr).await?;
        Ok(())
    }

    /// Send a voice packet to the server from the receiver's socket
    pub async fn send_voice_packet(&self, packet: &VoicePacket, server_addr: &str) -> Result<(), Box<dyn std::error::Error>> {
        let data = packet.encode()?;
        self.send_to(&data, server_addr).await
    }

    /// Wait for auth response with timeout (must be called before start_receiving)
    pub async fn wait_auth_response(&self, timeout_secs: u64) -> Result<bool, Box<dyn std::error::Error>> {
        let mut buf = [0u8; 256];
        match tokio::time::timeout(
            Duration::from_secs(timeout_secs),
            self.socket.recv(&mut buf),
        ).await {
            Ok(Ok(n)) => {
                match UdpAuthResponse::decode(&buf[..n]) {
                    Ok(response) => {
                        if response.success {
                            debug!("Auth response received: SUCCESS");
                            self.auth_success.store(true, Ordering::Relaxed);
                            Ok(true)
                        } else {
                            debug!("Auth response received: FAILURE");
                            Ok(false)
                        }
                    }
                    Err(e) => Err(format!("Failed to decode auth response: {}", e).into()),
                }
            }
            Ok(Err(e)) => Err(format!("Failed to receive auth response: {}", e).into()),
            Err(_) => Err("Auth response timeout".into()),
        }
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
                    Ok((n, _peer_addr)) => {
                        // Try to decode voice packet
                        match VoicePacket::decode(&buf[..n]) {
                            Ok((packet, _)) => {
                                // Look up username from SSRC
                                let map = ssrc_map.read().await;
                                match map.get(&packet.ssrc) {
                                    Some(username) => {
                                        // Route packet through jitter buffer
                                        let mut buffers = jitter_buffers.write().await;
                                        if let Some(jb) = buffers.get_mut(username) {
                                            // Insert packet into jitter buffer
                                            if let Some(mut ready_packet) = jb.insert(packet) {
                                                // Process all ready packets from the jitter buffer
                                                debug!("Jitter buffer returned packet seq={} for {}", ready_packet.sequence, username);
                                                loop {
                                                    drop(buffers); // Release lock before calling process_packet
                                                    if let Err(e) = manager.process_packet(username, &ready_packet).await {
                                                        error!("Failed to process packet for {}: {}", username, e);
                                                    } else {
                                                        debug!("Successfully processed packet seq={} for {}", ready_packet.sequence, username);
                                                    }
                                                    // Reacquire lock and try to get next available packet
                                                    buffers = jitter_buffers.write().await;
                                                    if let Some(jb) = buffers.get_mut(username) {
                                                        if let Some(next_packet) = jb.next_available() {
                                                            ready_packet = next_packet;
                                                            // Continue loop to process this packet
                                                        } else {
                                                            // No more ready packets, break loop
                                                            break;
                                                        }
                                                    } else {
                                                        break;
                                                    }
                                                }
                                            }
                                        } else {
                                            let available_users: Vec<String> = buffers.keys().cloned().collect();
                                            error!("No jitter buffer for user '{}', dropping packet. Available users: {:?}", username, available_users);
                                        }
                                    }
                                    None => {
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
