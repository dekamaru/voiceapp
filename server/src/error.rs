//! Error types for the voiceapp server.

use std::net::SocketAddr;
use thiserror::Error;

/// Errors that can occur in the server.
#[derive(Error, Debug)]
pub enum ServerError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Protocol error: {0}")]
    Protocol(#[from] voiceapp_protocol::ProtocolError),

    #[error("User not found: {0}")]
    UserNotFound(SocketAddr),
}
