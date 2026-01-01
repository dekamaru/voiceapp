use crate::error::ProtocolError;
use crate::io::{Reader, Writer};
use crate::packet_id::PacketId;

/// User information.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct ParticipantInfo {
    pub user_id: u64,
    pub username: String,
    pub in_voice: bool,
    pub is_muted: bool,
}

impl ParticipantInfo {
    /// Creates a new participant.
    #[must_use]
    pub fn new(user_id: u64, username: String, in_voice: bool, is_muted: bool) -> Self {
        Self {
            user_id,
            username,
            in_voice,
            is_muted,
        }
    }

    fn write(&self, w: &mut Writer) {
        w.write_u64(self.user_id);
        w.write_string(&self.username);
        w.write_bool(self.in_voice);
        w.write_bool(self.is_muted);
    }

    fn read(r: &mut Reader) -> Result<Self, ProtocolError> {
        Ok(Self {
            user_id: r.read_u64()?,
            username: r.read_string()?,
            in_voice: r.read_bool()?,
            is_muted: r.read_bool()?,
        })
    }
}

/// Protocol packet types for client-server communication.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Packet {
    // Requests
    LoginRequest {
        request_id: u64,
        username: String,
    },
    VoiceAuthRequest {
        request_id: u64,
        voice_token: u64,
    },
    JoinVoiceChannelRequest {
        request_id: u64,
    },
    LeaveVoiceChannelRequest {
        request_id: u64,
    },
    ChatMessageRequest {
        request_id: u64,
        message: String,
    },
    PingRequest {
        request_id: u64,
    },

    // Responses
    LoginResponse {
        request_id: u64,
        id: u64,
        voice_token: u64,
        participants: Vec<ParticipantInfo>,
    },
    VoiceAuthResponse {
        request_id: u64,
        success: bool,
    },
    JoinVoiceChannelResponse {
        request_id: u64,
        success: bool,
    },
    LeaveVoiceChannelResponse {
        request_id: u64,
        success: bool,
    },
    ChatMessageResponse {
        request_id: u64,
        success: bool,
    },
    PingResponse {
        request_id: u64,
    },

    // Events
    UserJoinedServer {
        participant: ParticipantInfo,
    },
    UserJoinedVoice {
        user_id: u64,
    },
    UserLeftVoice {
        user_id: u64,
    },
    UserLeftServer {
        user_id: u64,
    },
    UserSentMessage {
        user_id: u64,
        timestamp: u64,
        message: String,
    },
    UserMuteState {
        user_id: u64,
        is_muted: bool,
    },

    // UDP
    VoiceData {
        user_id: u64,
        sequence: u32,
        timestamp: u32,
        data: Vec<u8>,
    },
}

impl Packet {
    /// Encode packet to wire format.
    ///
    /// Format: `[packet_id: u8][payload_len: u16][payload...]`
    ///
    /// # Panics
    /// Panics if payload exceeds 65535 bytes or participants exceed 65535.
    #[must_use]
    #[allow(clippy::too_many_lines)]
    pub fn encode(&self) -> Vec<u8> {
        let mut w = Writer::new();
        w.write_u8(self.id());

        let len_pos = w.reserve_u16();
        let payload_start = w.position();

        match self {
            Self::LoginRequest {
                request_id,
                username,
            } => {
                w.write_u64(*request_id);
                w.write_string(username);
            }
            Self::VoiceAuthRequest {
                request_id,
                voice_token,
            } => {
                w.write_u64(*request_id);
                w.write_u64(*voice_token);
            }
            Self::JoinVoiceChannelRequest { request_id }
            | Self::LeaveVoiceChannelRequest { request_id } => {
                w.write_u64(*request_id);
            }
            Self::ChatMessageRequest {
                request_id,
                message,
            } => {
                w.write_u64(*request_id);
                w.write_string(message);
            }
            Self::PingRequest { request_id } => {
                w.write_u64(*request_id);
            }
            Self::LoginResponse {
                request_id,
                id,
                voice_token,
                participants,
            } => {
                w.write_u64(*request_id);
                w.write_u64(*id);
                w.write_u64(*voice_token);
                w.write_u16(
                    participants
                        .len()
                        .try_into()
                        .expect("too many participants"),
                );
                for p in participants {
                    p.write(&mut w);
                }
            }
            Self::VoiceAuthResponse {
                request_id,
                success,
            }
            | Self::JoinVoiceChannelResponse {
                request_id,
                success,
            }
            | Self::LeaveVoiceChannelResponse {
                request_id,
                success,
            }
            | Self::ChatMessageResponse {
                request_id,
                success,
            } => {
                w.write_u64(*request_id);
                w.write_bool(*success);
            }
            Self::PingResponse { request_id } => {
                w.write_u64(*request_id);
            }
            Self::UserJoinedServer { participant } => participant.write(&mut w),
            Self::UserJoinedVoice { user_id }
            | Self::UserLeftVoice { user_id }
            | Self::UserLeftServer { user_id } => {
                w.write_u64(*user_id);
            }
            Self::UserMuteState { user_id, is_muted } => {
                w.write_u64(*user_id);
                w.write_bool(*is_muted);
            }
            Self::UserSentMessage {
                user_id,
                timestamp,
                message,
            } => {
                w.write_u64(*user_id);
                w.write_u64(*timestamp);
                w.write_string(message);
            }
            Self::VoiceData {
                user_id,
                sequence,
                timestamp,
                data,
            } => {
                w.write_u64(*user_id);
                w.write_u32(*sequence);
                w.write_u32(*timestamp);
                w.write_bytes(data);
            }
        }

        w.write_u16_at(
            len_pos,
            (w.position() - payload_start)
                .try_into()
                .expect("payload too large"),
        );
        w.into_vec()
    }

    /// Decode packet from wire format.
    ///
    /// Returns decoded packet and number of bytes consumed from the buffer.
    ///
    /// # Errors
    /// Returns error if buffer is incomplete or contains invalid data.
    pub fn decode(buf: &[u8]) -> Result<(Self, usize), ProtocolError> {
        let mut header = Reader::new(buf);
        let packet_id = PacketId::try_from(header.read_u8()?)?;
        let payload_len = header.read_u16()? as usize;
        let remaining = header.remaining();

        if remaining.len() < payload_len {
            return Err(ProtocolError::IncompletePayload {
                expected: payload_len,
                got: remaining.len(),
            });
        }

        let mut r = Reader::new(&remaining[..payload_len]);

        let packet = match packet_id {
            PacketId::LoginRequest => Self::LoginRequest {
                request_id: r.read_u64()?,
                username: r.read_string()?,
            },
            PacketId::VoiceAuthRequest => Self::VoiceAuthRequest {
                request_id: r.read_u64()?,
                voice_token: r.read_u64()?,
            },
            PacketId::JoinVoiceChannelRequest => Self::JoinVoiceChannelRequest {
                request_id: r.read_u64()?,
            },
            PacketId::LeaveVoiceChannelRequest => Self::LeaveVoiceChannelRequest {
                request_id: r.read_u64()?,
            },
            PacketId::ChatMessageRequest => Self::ChatMessageRequest {
                request_id: r.read_u64()?,
                message: r.read_string()?,
            },
            PacketId::PingRequest => Self::PingRequest {
                request_id: r.read_u64()?,
            },
            PacketId::LoginResponse => {
                let request_id = r.read_u64()?;
                let id = r.read_u64()?;
                let voice_token = r.read_u64()?;
                let count = r.read_u16()? as usize;
                let mut participants = Vec::with_capacity(count);
                for _ in 0..count {
                    participants.push(ParticipantInfo::read(&mut r)?);
                }
                Self::LoginResponse {
                    request_id,
                    id,
                    voice_token,
                    participants,
                }
            }
            PacketId::VoiceAuthResponse => Self::VoiceAuthResponse {
                request_id: r.read_u64()?,
                success: r.read_bool()?,
            },
            PacketId::JoinVoiceChannelResponse => Self::JoinVoiceChannelResponse {
                request_id: r.read_u64()?,
                success: r.read_bool()?,
            },
            PacketId::LeaveVoiceChannelResponse => Self::LeaveVoiceChannelResponse {
                request_id: r.read_u64()?,
                success: r.read_bool()?,
            },
            PacketId::ChatMessageResponse => Self::ChatMessageResponse {
                request_id: r.read_u64()?,
                success: r.read_bool()?,
            },
            PacketId::PingResponse => Self::PingResponse {
                request_id: r.read_u64()?,
            },
            PacketId::UserJoinedServer => Self::UserJoinedServer {
                participant: ParticipantInfo::read(&mut r)?,
            },
            PacketId::UserJoinedVoice => Self::UserJoinedVoice {
                user_id: r.read_u64()?,
            },
            PacketId::UserLeftVoice => Self::UserLeftVoice {
                user_id: r.read_u64()?,
            },
            PacketId::UserLeftServer => Self::UserLeftServer {
                user_id: r.read_u64()?,
            },
            PacketId::UserSentMessage => Self::UserSentMessage {
                user_id: r.read_u64()?,
                timestamp: r.read_u64()?,
                message: r.read_string()?,
            },
            PacketId::UserMuteState => Self::UserMuteState {
                user_id: r.read_u64()?,
                is_muted: r.read_bool()?,
            },
            PacketId::VoiceData => Self::VoiceData {
                user_id: r.read_u64()?,
                sequence: r.read_u32()?,
                timestamp: r.read_u32()?,
                data: r.remaining().to_vec(),
            },
        };

        Ok((packet, header.position() + payload_len))
    }

    /// Returns the packet type ID.
    #[must_use]
    pub fn id(&self) -> u8 {
        match self {
            Self::LoginRequest { .. } => PacketId::LoginRequest,
            Self::VoiceAuthRequest { .. } => PacketId::VoiceAuthRequest,
            Self::JoinVoiceChannelRequest { .. } => PacketId::JoinVoiceChannelRequest,
            Self::LeaveVoiceChannelRequest { .. } => PacketId::LeaveVoiceChannelRequest,
            Self::ChatMessageRequest { .. } => PacketId::ChatMessageRequest,
            Self::PingRequest { .. } => PacketId::PingRequest,
            Self::LoginResponse { .. } => PacketId::LoginResponse,
            Self::VoiceAuthResponse { .. } => PacketId::VoiceAuthResponse,
            Self::JoinVoiceChannelResponse { .. } => PacketId::JoinVoiceChannelResponse,
            Self::LeaveVoiceChannelResponse { .. } => PacketId::LeaveVoiceChannelResponse,
            Self::ChatMessageResponse { .. } => PacketId::ChatMessageResponse,
            Self::PingResponse { .. } => PacketId::PingResponse,
            Self::UserJoinedServer { .. } => PacketId::UserJoinedServer,
            Self::UserJoinedVoice { .. } => PacketId::UserJoinedVoice,
            Self::UserLeftVoice { .. } => PacketId::UserLeftVoice,
            Self::UserLeftServer { .. } => PacketId::UserLeftServer,
            Self::UserSentMessage { .. } => PacketId::UserSentMessage,
            Self::UserMuteState { .. } => PacketId::UserMuteState,
            Self::VoiceData { .. } => PacketId::VoiceData,
        }
        .as_u8()
    }

    /// Returns the request ID if this packet has one.
    ///
    /// Returns `Some(request_id)` for request/response packets, `None` for events and voice data.
    #[must_use]
    pub fn request_id(&self) -> Option<u64> {
        match self {
            Self::LoginRequest { request_id, .. }
            | Self::VoiceAuthRequest { request_id, .. }
            | Self::JoinVoiceChannelRequest { request_id }
            | Self::LeaveVoiceChannelRequest { request_id }
            | Self::ChatMessageRequest { request_id, .. }
            | Self::PingRequest { request_id }
            | Self::LoginResponse { request_id, .. }
            | Self::VoiceAuthResponse { request_id, .. }
            | Self::JoinVoiceChannelResponse { request_id, .. }
            | Self::LeaveVoiceChannelResponse { request_id, .. }
            | Self::ChatMessageResponse { request_id, .. }
            | Self::PingResponse { request_id } => Some(*request_id),
            _ => None,
        }
    }
}

#[cfg(test)]
#[allow(clippy::unreadable_literal, clippy::needless_pass_by_value)]
mod tests {
    use super::*;

    fn roundtrip(packet: Packet) {
        let encoded = packet.encode();
        let (decoded, size) = Packet::decode(&encoded).expect("decode failed");
        assert_eq!(packet, decoded);
        assert_eq!(encoded.len(), size);
    }

    #[test]
    fn roundtrip_string_encoding() {
        roundtrip(Packet::LoginRequest {
            request_id: 1,
            username: "alice".to_string(),
        });
    }

    #[test]
    fn roundtrip_u64_encoding() {
        roundtrip(Packet::VoiceAuthRequest {
            request_id: 2,
            voice_token: 0x1234567890ABCDEF,
        });
    }

    #[test]
    fn roundtrip_empty_payload() {
        roundtrip(Packet::JoinVoiceChannelRequest { request_id: 3 });
    }

    #[test]
    fn roundtrip_bool_encoding() {
        roundtrip(Packet::VoiceAuthResponse {
            request_id: 4,
            success: true,
        });
        roundtrip(Packet::VoiceAuthResponse {
            request_id: 5,
            success: false,
        });
    }

    #[test]
    fn roundtrip_struct_with_list() {
        roundtrip(Packet::LoginResponse {
            request_id: 6,
            id: 42,
            voice_token: 0xDEADBEEF,
            participants: vec![
                ParticipantInfo {
                    user_id: 1,
                    username: "alice".to_string(),
                    in_voice: true,
                    is_muted: false,
                },
                ParticipantInfo {
                    user_id: 2,
                    username: "bob".to_string(),
                    in_voice: false,
                    is_muted: true,
                },
            ],
        });
    }

    #[test]
    fn roundtrip_multiple_strings() {
        roundtrip(Packet::UserSentMessage {
            user_id: 111,
            timestamp: 0xDEADBEEF,
            message: "Test message".to_string(),
        });
    }

    #[test]
    fn roundtrip_binary_data() {
        roundtrip(Packet::VoiceData {
            user_id: 222,
            sequence: 1000,
            timestamp: 48000,
            data: vec![0x12, 0x34, 0x56, 0x78, 0x90, 0xAB, 0xCD, 0xEF],
        });
    }

    #[test]
    fn roundtrip_empty_string() {
        roundtrip(Packet::LoginRequest {
            request_id: 7,
            username: String::new(),
        });
    }

    #[test]
    fn roundtrip_unicode_string() {
        roundtrip(Packet::UserSentMessage {
            user_id: 1,
            timestamp: 0xDEADBEEF,
            message: "用户🎉 Привет мир! 🌍".to_string(),
        });
    }

    #[test]
    fn roundtrip_empty_list() {
        roundtrip(Packet::LoginResponse {
            request_id: 8,
            id: 1,
            voice_token: 123,
            participants: vec![],
        });
    }

    #[test]
    fn roundtrip_large_binary_data() {
        roundtrip(Packet::VoiceData {
            user_id: 1,
            sequence: 0,
            timestamp: 0,
            data: vec![0xFF; 1024],
        });
    }
}
