use std::fmt;

/// Protocol decoding errors.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProtocolError {
    PacketTooShort { expected: usize, got: usize },
    UnknownPacketId(u8),
    InvalidUtf8,
    IncompletePayload { expected: usize, got: usize },
}

impl fmt::Display for ProtocolError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ProtocolError::PacketTooShort { expected, got } => {
                write!(f, "packet too short: expected at least {} bytes, got {}", expected, got)
            }
            ProtocolError::UnknownPacketId(id) => {
                write!(f, "unknown packet id: 0x{:02x}", id)
            }
            ProtocolError::InvalidUtf8 => {
                write!(f, "invalid UTF-8 encoding")
            }
            ProtocolError::IncompletePayload { expected, got } => {
                write!(f, "incomplete payload: expected {} bytes, got {}", expected, got)
            }
        }
    }
}

impl std::error::Error for ProtocolError {}