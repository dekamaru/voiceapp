//! Configuration constants for the voiceapp server.

use std::env;

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

/// Returns the management server port from `MANAGEMENT_PORT` env var or default.
#[must_use]
pub fn management_port() -> u16 {
    env::var("MANAGEMENT_PORT")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(DEFAULT_MANAGEMENT_PORT)
}

/// Returns the voice relay server port from `VOICE_RELAY_PORT` env var or default.
#[must_use]
pub fn voice_port() -> u16 {
    env::var("VOICE_RELAY_PORT")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(DEFAULT_VOICE_PORT)
}
