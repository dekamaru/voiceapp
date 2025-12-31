use std::net::SocketAddr;

/// Represents an authenticated voice session for UDP communication.
#[derive(Clone, Copy, Debug)]
pub struct VoiceSession {
    pub token: u64,
    pub in_voice: bool,
    pub udp_address: Option<SocketAddr>,
}