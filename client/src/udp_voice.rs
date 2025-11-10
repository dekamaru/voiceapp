use std::net::SocketAddr;
use std::time::Duration;
use tokio::net::UdpSocket;
use tracing::{debug, info};
use voiceapp_common::{VoicePacket, UdpAuthResponse};

/// Sends voice packets over UDP to the server
pub struct UdpVoiceSender {
    socket: UdpSocket,
    server_addr: SocketAddr,
}

impl UdpVoiceSender {
    /// Create a new UDP voice sender
    pub async fn new(local_addr: &str, server_addr: &str) -> Result<Self, Box<dyn std::error::Error>> {
        let socket = UdpSocket::bind(local_addr).await?;
        let server_addr: SocketAddr = server_addr.parse()?;

        info!("UDP voice sender bound to {} sending to {}", local_addr, server_addr);

        Ok(UdpVoiceSender { socket, server_addr })
    }

    /// Send a voice packet to the server
    pub async fn send_packet(&self, packet: &VoicePacket) -> Result<(), Box<dyn std::error::Error>> {
        let encoded = packet.encode()?;
        self.socket.send_to(&encoded, self.server_addr).await?;

        debug!("Sent voice packet: seq={}, ts={}, size={}", packet.sequence, packet.timestamp, encoded.len());

        Ok(())
    }

    /// Send multiple voice packets
    pub async fn send_packets(&self, packets: &[VoicePacket]) -> Result<(), Box<dyn std::error::Error>> {
        for packet in packets {
            self.send_packet(packet).await?;
        }
        Ok(())
    }

    /// Send raw data to the server (e.g., auth packets)
    pub async fn send_raw(&self, data: &[u8]) -> Result<(), Box<dyn std::error::Error>> {
        self.socket.send_to(data, self.server_addr).await?;
        debug!("Sent raw data: {} bytes", data.len());
        Ok(())
    }

    /// Wait for auth response from server with timeout
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use voiceapp_common::username_to_ssrc;

    #[tokio::test]
    async fn test_udp_sender_creation() {
        // This will fail to bind to privileged port, but that's okay for testing the constructor
        let result = UdpVoiceSender::new("127.0.0.1:0", "127.0.0.1:9002").await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_send_voice_packet() {
        // Create sender
        let sender = UdpVoiceSender::new("127.0.0.1:0", "127.0.0.1:9002")
            .await
            .expect("Failed to create sender");

        // Create a dummy voice packet
        let ssrc = username_to_ssrc("alice");
        let packet = VoicePacket::new(0, 0, ssrc, vec![0x12, 0x34, 0x56, 0x78]);

        // Send should complete without error (even if no receiver is listening)
        let result = sender.send_packet(&packet).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_send_multiple_packets() {
        let sender = UdpVoiceSender::new("127.0.0.1:0", "127.0.0.1:9002")
            .await
            .expect("Failed to create sender");

        let ssrc = username_to_ssrc("alice");
        let packets = vec![
            VoicePacket::new(0, 0, ssrc, vec![0x11, 0x22]),
            VoicePacket::new(1, 960, ssrc, vec![0x33, 0x44]),
            VoicePacket::new(2, 1920, ssrc, vec![0x55, 0x66]),
        ];

        let result = sender.send_packets(&packets).await;
        assert!(result.is_ok());
    }
}
