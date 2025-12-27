use std::fmt;

/// Protocol decoding errors.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum ProtocolError {
    PacketTooShort { expected: usize, got: usize },
    UnknownPacketId(u8),
    InvalidUtf8,
    IncompletePayload { expected: usize, got: usize },
}

impl fmt::Display for ProtocolError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::PacketTooShort { expected, got } => {
                write!(
                    f,
                    "packet too short: expected at least {expected} bytes, got {got}"
                )
            }
            Self::UnknownPacketId(id) => write!(f, "unknown packet id: 0x{id:02x}"),
            Self::InvalidUtf8 => write!(f, "invalid UTF-8 encoding"),
            Self::IncompletePayload { expected, got } => {
                write!(
                    f,
                    "incomplete payload: expected {expected} bytes, got {got}"
                )
            }
        }
    }
}

impl std::error::Error for ProtocolError {}
