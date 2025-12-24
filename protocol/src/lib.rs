//! Binary protocol for voice application communication.
//!
//! Wire format: `[packet_id: u8][payload_len: u16][payload...]`

mod packet;
mod packet_id;
mod error;
mod io;

pub use error::ProtocolError;
pub use packet::{Packet, ParticipantInfo};
