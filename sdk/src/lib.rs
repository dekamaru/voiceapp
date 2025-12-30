//! VoiceApp SDK - Voice communication client library
//!
//! ## Example
//! ```no_run
//! use voiceapp_sdk::{Client, SdkError};
//!
//! async fn example() -> Result<(), SdkError> {
//!     let client = Client::new();
//!     client.connect("mgmt:8080", "voice:9090", "user").await?;
//!     client.join_channel().await?;
//!     Ok(())
//! }
//! ```

mod network;
mod voice;
mod client;
mod error;

pub use client::Client;
pub use error::SdkError;
pub use network::ClientEvent;
pub use voice::decoder::Decoder;
pub use voiceapp_protocol::ParticipantInfo;
