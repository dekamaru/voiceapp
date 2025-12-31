/// Events emitted by ManagementServer for VoiceRelayServer synchronization.
#[derive(Debug, Clone)]
pub enum Event {
    /// User connected and received authentication token.
    UserConnected { id: u64, token: u64 },
    /// User joined voice channel.
    VoiceJoined { id: u64 },
    /// User left voice channel.
    VoiceLeft { id: u64 },
    /// User disconnected from server.
    UserDisconnected { id: u64 },
}