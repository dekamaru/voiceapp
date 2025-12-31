use std::net::SocketAddr;
use voiceapp_protocol::Packet;

/// Broadcast message sent to all connected clients.
#[derive(Clone, Debug)]
pub struct BroadcastMessage {
    exclude: Option<SocketAddr>,
    data: Vec<u8>,
}

impl BroadcastMessage {
    /// Create a broadcast message that will be sent to all clients.
    pub fn to_all(packet: &Packet) -> Self {
        Self {
            exclude: None,
            data: packet.encode(),
        }
    }

    /// Create a broadcast message that excludes the sender.
    pub fn excluding(sender: SocketAddr, packet: &Packet) -> Self {
        Self {
            exclude: Some(sender),
            data: packet.encode(),
        }
    }

    /// Check if this message should be sent to the given address.
    pub fn should_send_to(&self, addr: SocketAddr) -> bool {
        self.exclude != Some(addr)
    }

    /// Get the encoded packet data.
    pub fn data(&self) -> &[u8] {
        &self.data
    }
}