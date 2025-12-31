//! Voice application server implementation.
//!
//! This crate provides two server components:
//! - [`ManagementServer`]: TCP server for user management and presence
//! - [`VoiceRelayServer`]: UDP server for voice packet relay
//!
//! # Architecture
//!
//! The server is split into two main components:
//!
//! 1. **ManagementServer** - Handles TCP connections for:
//!    - User authentication and login
//!    - Presence management (join/leave voice channels)
//!    - Chat messaging
//!    - Mute state synchronization
//!
//! 2. **VoiceRelayServer** - Handles UDP packets for:
//!    - Voice authentication (token-based)
//!    - Voice packet forwarding between participants
//!
//! The two servers communicate via an event channel to synchronize user state.

pub mod config;
pub mod error;
pub mod event;
pub mod management;
pub mod voice;

pub use config::*;
pub use error::ServerError;
pub use event::Event;
pub use management::server::ManagementServer;
pub use voice::server::VoiceRelayServer;
