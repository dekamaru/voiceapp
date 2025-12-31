//! Configuration constants for the voiceapp server.

/// Default port for the management (TCP) server.
pub const DEFAULT_MANAGEMENT_PORT: u16 = 9001;

/// Default port for the voice relay (UDP) server.
pub const DEFAULT_VOICE_PORT: u16 = 9002;

/// Buffer size for reading packets.
pub const PACKET_BUFFER_SIZE: usize = 4096;

/// Capacity of the broadcast channel for client messages.
pub const BROADCAST_CHANNEL_CAPACITY: usize = 1000;

/// Maximum allowed username length.
pub const MAX_USERNAME_LEN: usize = 32;
