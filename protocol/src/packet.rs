use crate::io::{Reader, Writer};
use crate::packet_id::PacketId;
use crate::error::ProtocolError;

/// User information in voice channel.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParticipantInfo {
    pub user_id: u64,
    pub username: String,
    pub in_voice: bool,
}

/// Protocol packet types for client-server communication.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Packet {
    // Requests
    LoginRequest { username: String },
    VoiceAuthRequest { voice_token: u64 },
    JoinVoiceChannelRequest,
    LeaveVoiceChannelRequest,
    ChatMessageRequest { message: String },

    // Responses
    LoginResponse { id: u64, voice_token: u64, participants: Vec<ParticipantInfo> },
    VoiceAuthResponse { success: bool },
    JoinVoiceChannelResponse { success: bool },
    LeaveVoiceChannelResponse { success: bool },
    ChatMessageResponse { success: bool },

    // Events
    UserJoinedServer { participant: ParticipantInfo },
    UserJoinedVoice { user_id: u64 },
    UserLeftVoice { user_id: u64 },
    UserLeftServer { user_id: u64 },
    UserSentMessage { user_id: u64, username: String, message: String },

    // UDP
    VoiceData { user_id: u64, sequence: u32, timestamp: u32, data: Vec<u8> },
}

impl Packet {
    /// Encode packet to wire format.
    ///
    /// Format: `[packet_id: u8][payload_len: u16][payload...]`
    pub fn encode(&self) -> Vec<u8> {
        let mut writer = Writer::new();

        // Write packet_id
        writer.write_u8(self.packet_id().as_u8());

        // Reserve space for payload_len, we'll fill it in later
        let len_pos = writer.reserve_u16();
        let payload_start = writer.position();

        // Write payload
        match self {
            Packet::LoginRequest { username } => {
                writer.write_string(username);
            }
            Packet::VoiceAuthRequest { voice_token } => {
                writer.write_u64(*voice_token);
            }
            Packet::JoinVoiceChannelRequest | Packet::LeaveVoiceChannelRequest => {
                // No payload
            }
            Packet::ChatMessageRequest { message } => {
                writer.write_string(message);
            }
            Packet::LoginResponse { id, voice_token, participants } => {
                writer.write_u64(*id);
                writer.write_u64(*voice_token);
                writer.write_u16(participants.len() as u16);
                
                for participant in participants {
                    writer.write_u64(participant.user_id);
                    writer.write_string(&participant.username);
                    writer.write_bool(participant.in_voice);
                }
            }
            Packet::VoiceAuthResponse { success }
            | Packet::JoinVoiceChannelResponse { success }
            | Packet::LeaveVoiceChannelResponse { success }
            | Packet::ChatMessageResponse { success } => {
                writer.write_bool(*success);
            }
            Packet::UserJoinedServer { participant } => {
                writer.write_u64(participant.user_id);
                writer.write_string(&participant.username);
                writer.write_bool(participant.in_voice);
            }
            Packet::UserJoinedVoice { user_id }
            | Packet::UserLeftVoice { user_id }
            | Packet::UserLeftServer { user_id } => {
                writer.write_u64(*user_id);
            }
            Packet::UserSentMessage { user_id, username, message } => {
                writer.write_u64(*user_id);
                writer.write_string(username);
                writer.write_string(message);
            }
            Packet::VoiceData { user_id, sequence, timestamp, data } => {
                writer.write_u64(*user_id);
                writer.write_u32(*sequence);
                writer.write_u32(*timestamp);
                writer.write_bytes(data);
            }
        }

        // Calculate and write the actual payload length
        let payload_len = writer.position() - payload_start;
        writer.write_u16_at(len_pos, payload_len as u16);

        writer.into_vec()
    }

    /// Decode packet from wire format.
    ///
    /// Returns error if buffer is incomplete or contains invalid data.
    pub fn decode(buf: &[u8]) -> Result<Self, ProtocolError> {
        let mut reader = Reader::new(buf);

        let packet_id = PacketId::try_from(reader.read_u8()?)?;
        let payload_len = reader.read_u16()? as usize;
        let remaining = reader.remaining();

        if remaining.len() < payload_len {
            return Err(ProtocolError::IncompletePayload {
                expected: payload_len,
                got: remaining.len(),
            });
        }

        let mut payload_reader = Reader::new(&remaining[..payload_len]);

        match packet_id {
            PacketId::LoginRequest => {
                let username = payload_reader.read_string()?;
                Ok(Packet::LoginRequest { username })
            }
            PacketId::VoiceAuthRequest => {
                let voice_token = payload_reader.read_u64()?;
                Ok(Packet::VoiceAuthRequest { voice_token })
            }
            PacketId::JoinVoiceChannelRequest => {
                Ok(Packet::JoinVoiceChannelRequest)
            }
            PacketId::LeaveVoiceChannelRequest => {
                Ok(Packet::LeaveVoiceChannelRequest)
            }
            PacketId::ChatMessageRequest => {
                let message = payload_reader.read_string()?;
                Ok(Packet::ChatMessageRequest { message })
            }
            PacketId::LoginResponse => {
                let id = payload_reader.read_u64()?;
                let voice_token = payload_reader.read_u64()?;
                let participant_count = payload_reader.read_u16()? as usize;
                let mut participants = Vec::with_capacity(participant_count);
                for _ in 0..participant_count {
                    let user_id = payload_reader.read_u64()?;
                    let username = payload_reader.read_string()?;
                    let in_voice = payload_reader.read_bool()?;
                    participants.push(ParticipantInfo { user_id, username, in_voice });
                }
                Ok(Packet::LoginResponse { id, voice_token, participants })
            }
            PacketId::VoiceAuthResponse => {
                let success = payload_reader.read_bool()?;
                Ok(Packet::VoiceAuthResponse { success })
            }
            PacketId::JoinVoiceChannelResponse => {
                let success = payload_reader.read_bool()?;
                Ok(Packet::JoinVoiceChannelResponse { success })
            }
            PacketId::LeaveVoiceChannelResponse => {
                let success = payload_reader.read_bool()?;
                Ok(Packet::LeaveVoiceChannelResponse { success })
            }
            PacketId::ChatMessageResponse => {
                let success = payload_reader.read_bool()?;
                Ok(Packet::ChatMessageResponse { success })
            }
            PacketId::UserJoinedServer => {
                let user_id = payload_reader.read_u64()?;
                let username = payload_reader.read_string()?;
                let in_voice = payload_reader.read_bool()?;
                let participant = ParticipantInfo { user_id, username, in_voice };
                Ok(Packet::UserJoinedServer { participant })
            }
            PacketId::UserJoinedVoice => {
                let user_id = payload_reader.read_u64()?;
                Ok(Packet::UserJoinedVoice { user_id })
            }
            PacketId::UserLeftVoice => {
                let user_id = payload_reader.read_u64()?;
                Ok(Packet::UserLeftVoice { user_id })
            }
            PacketId::UserLeftServer => {
                let user_id = payload_reader.read_u64()?;
                Ok(Packet::UserLeftServer { user_id })
            }
            PacketId::UserSentMessage => {
                let user_id = payload_reader.read_u64()?;
                let username = payload_reader.read_string()?;
                let message = payload_reader.read_string()?;
                Ok(Packet::UserSentMessage { user_id, username, message })
            }
            PacketId::VoiceData => {
                let user_id = payload_reader.read_u64()?;
                let sequence = payload_reader.read_u32()?;
                let timestamp = payload_reader.read_u32()?;
                let data = payload_reader.remaining().to_vec();
                Ok(Packet::VoiceData { user_id, sequence, timestamp, data })
            }
        }
    }

    fn packet_id(&self) -> PacketId {
        match self {
            Packet::LoginRequest { .. } => PacketId::LoginRequest,
            Packet::VoiceAuthRequest { .. } => PacketId::VoiceAuthRequest,
            Packet::JoinVoiceChannelRequest => PacketId::JoinVoiceChannelRequest,
            Packet::LeaveVoiceChannelRequest => PacketId::LeaveVoiceChannelRequest,
            Packet::ChatMessageRequest { .. } => PacketId::ChatMessageRequest,
            Packet::LoginResponse { .. } => PacketId::LoginResponse,
            Packet::VoiceAuthResponse { .. } => PacketId::VoiceAuthResponse,
            Packet::JoinVoiceChannelResponse { .. } => PacketId::JoinVoiceChannelResponse,
            Packet::LeaveVoiceChannelResponse { .. } => PacketId::LeaveVoiceChannelResponse,
            Packet::ChatMessageResponse { .. } => PacketId::ChatMessageResponse,
            Packet::UserJoinedServer { .. } => PacketId::UserJoinedServer,
            Packet::UserJoinedVoice { .. } => PacketId::UserJoinedVoice,
            Packet::UserLeftVoice { .. } => PacketId::UserLeftVoice,
            Packet::UserLeftServer { .. } => PacketId::UserLeftServer,
            Packet::UserSentMessage { .. } => PacketId::UserSentMessage,
            Packet::VoiceData { .. } => PacketId::VoiceData,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn roundtrip(packet: Packet) {
        let encoded = packet.encode();
        let decoded = Packet::decode(&encoded).expect("decode failed");
        assert_eq!(packet, decoded);
    }

    #[test]
    fn roundtrip_string_encoding() {
        roundtrip(Packet::LoginRequest {
            username: "alice".to_string(),
        });
    }

    #[test]
    fn roundtrip_u64_encoding() {
        roundtrip(Packet::VoiceAuthRequest {
            voice_token: 0x1234567890ABCDEF,
        });
    }

    #[test]
    fn roundtrip_empty_payload() {
        roundtrip(Packet::JoinVoiceChannelRequest);
    }

    #[test]
    fn roundtrip_bool_encoding() {
        roundtrip(Packet::VoiceAuthResponse { success: true });
        roundtrip(Packet::VoiceAuthResponse { success: false });
    }

    #[test]
    fn roundtrip_struct_with_list() {
        roundtrip(Packet::LoginResponse {
            id: 42,
            voice_token: 0xDEADBEEF,
            participants: vec![
                ParticipantInfo {
                    user_id: 1,
                    username: "alice".to_string(),
                    in_voice: true,
                },
                ParticipantInfo {
                    user_id: 2,
                    username: "bob".to_string(),
                    in_voice: false,
                },
            ],
        });
    }

    #[test]
    fn roundtrip_multiple_strings() {
        roundtrip(Packet::UserSentMessage {
            user_id: 111,
            username: "dave".to_string(),
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
            username: "".to_string(),
        });
    }

    #[test]
    fn roundtrip_unicode_string() {
        roundtrip(Packet::UserSentMessage {
            user_id: 1,
            username: "François".to_string(),
            message: "用户🎉 Привет мир! 🌍".to_string(),
        });
    }

    #[test]
    fn roundtrip_empty_list() {
        roundtrip(Packet::LoginResponse {
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