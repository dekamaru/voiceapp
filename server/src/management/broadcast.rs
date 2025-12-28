use std::net::SocketAddr;

/// Broadcast message sent to all connected clients
#[derive(Clone, Debug)]
pub struct BroadcastMessage {
    pub sender_addr: Option<SocketAddr>, // None means server broadcast (all receive)
    pub for_all: bool,                   // If true, include sender; if false, exclude sender
    pub packet_data: Vec<u8>,            // Complete encoded packet to forward
}