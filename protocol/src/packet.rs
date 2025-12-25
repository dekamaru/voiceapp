use crate::io::{Reader, Writer};
use crate::packet_id::PacketId;
use crate::error::ProtocolError;

/// User information in voice channel.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParticipantInfo {
    pub user_id: u64,
    pub username: String,
    pub in_voice: bool,
    pub is_muted: bool,
}

/// Protocol packet types for client-server communication.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Packet {
    // Requests
    LoginRequest { request_id: u64, username: String },
    VoiceAuthRequest { request_id: u64, voice_token: u64 },
    JoinVoiceChannelRequest { request_id: u64 },
    LeaveVoiceChannelRequest { request_id: u64 },
    ChatMessageRequest { request_id: u64, message: String },

    // Responses
    LoginResponse { request_id: u64, id: u64, voice_token: u64, participants: Vec<ParticipantInfo> },
    VoiceAuthResponse { request_id: u64, success: bool },
    JoinVoiceChannelResponse { request_id: u64, success: bool },
    LeaveVoiceChannelResponse { request_id: u64, success: bool },
    ChatMessageResponse { request_id: u64, success: bool },

    // Events
    UserJoinedServer { participant: ParticipantInfo },
    UserJoinedVoice { user_id: u64 },
    UserLeftVoice { user_id: u64 },
    UserLeftServer { user_id: u64 },
    UserSentMessage { user_id: u64, timestamp: u64, message: String },
    UserMuteState { user_id: u64, is_muted: bool },

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
        writer.write_u8(self.id());

        // Reserve space for payload_len, we'll fill it in later
        let len_pos = writer.reserve_u16();
        let payload_start = writer.position();

        // Write payload
        match self {
            Packet::LoginRequest { request_id, username } => {
                writer.write_u64(*request_id);
                writer.write_string(username);
            }
            Packet::VoiceAuthRequest { request_id, voice_token } => {
                writer.write_u64(*request_id);
                writer.write_u64(*voice_token);
            }
            Packet::JoinVoiceChannelRequest { request_id } => {
                writer.write_u64(*request_id);
            }
            Packet::LeaveVoiceChannelRequest { request_id } => {
                writer.write_u64(*request_id);
            }
            Packet::ChatMessageRequest { request_id, message } => {
                writer.write_u64(*request_id);
                writer.write_string(message);
            }
            Packet::LoginResponse { request_id, id, voice_token, participants } => {
                writer.write_u64(*request_id);
                writer.write_u64(*id);
                writer.write_u64(*voice_token);
                writer.write_u16(participants.len() as u16);

                for participant in participants {
                    writer.write_u64(participant.user_id);
                    writer.write_string(&participant.username);
                    writer.write_bool(participant.in_voice);
                    writer.write_bool(participant.is_muted);
                }
            }
            Packet::VoiceAuthResponse { request_id, success } => {
                writer.write_u64(*request_id);
                writer.write_bool(*success);
            }
            Packet::JoinVoiceChannelResponse { request_id, success } => {
                writer.write_u64(*request_id);
                writer.write_bool(*success);
            }
            Packet::LeaveVoiceChannelResponse { request_id, success } => {
                writer.write_u64(*request_id);
                writer.write_bool(*success);
            }
            Packet::ChatMessageResponse { request_id, success } => {
                writer.write_u64(*request_id);
                writer.write_bool(*success);
            }
            Packet::UserJoinedServer { participant } => {
                writer.write_u64(participant.user_id);
                writer.write_string(&participant.username);
                writer.write_bool(participant.in_voice);
                writer.write_bool(participant.is_muted);
            }
            Packet::UserJoinedVoice { user_id }
            | Packet::UserLeftVoice { user_id }
            | Packet::UserLeftServer { user_id } => {
                writer.write_u64(*user_id);
            }
            Packet::UserMuteState { user_id, is_muted } => {
                writer.write_u64(*user_id);
                writer.write_bool(*is_muted);
            }
            Packet::UserSentMessage { user_id, timestamp, message } => {
                writer.write_u64(*user_id);
                writer.write_u64(*timestamp);
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
    /// Returns decoded packet and number of bytes consumed from the buffer.
    /// Returns error if buffer is incomplete or contains invalid data.
    pub fn decode(buf: &[u8]) -> Result<(Self, usize), ProtocolError> {
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

        let packet = match packet_id {
            PacketId::LoginRequest => {
                let request_id = payload_reader.read_u64()?;
                let username = payload_reader.read_string()?;
                Packet::LoginRequest { request_id, username }
            }
            PacketId::VoiceAuthRequest => {
                let request_id = payload_reader.read_u64()?;
                let voice_token = payload_reader.read_u64()?;
                Packet::VoiceAuthRequest { request_id, voice_token }
            }
            PacketId::JoinVoiceChannelRequest => {
                let request_id = payload_reader.read_u64()?;
                Packet::JoinVoiceChannelRequest { request_id }
            }
            PacketId::LeaveVoiceChannelRequest => {
                let request_id = payload_reader.read_u64()?;
                Packet::LeaveVoiceChannelRequest { request_id }
            }
            PacketId::ChatMessageRequest => {
                let request_id = payload_reader.read_u64()?;
                let message = payload_reader.read_string()?;
                Packet::ChatMessageRequest { request_id, message }
            }
            PacketId::LoginResponse => {
                let request_id = payload_reader.read_u64()?;
                let id = payload_reader.read_u64()?;
                let voice_token = payload_reader.read_u64()?;
                let participant_count = payload_reader.read_u16()? as usize;
                let mut participants = Vec::with_capacity(participant_count);
                for _ in 0..participant_count {
                    let user_id = payload_reader.read_u64()?;
                    let username = payload_reader.read_string()?;
                    let in_voice = payload_reader.read_bool()?;
                    let is_muted = payload_reader.read_bool()?;
                    participants.push(ParticipantInfo { user_id, username, in_voice, is_muted });
                }
                Packet::LoginResponse { request_id, id, voice_token, participants }
            }
            PacketId::VoiceAuthResponse => {
                let request_id = payload_reader.read_u64()?;
                let success = payload_reader.read_bool()?;
                Packet::VoiceAuthResponse { request_id, success }
            }
            PacketId::JoinVoiceChannelResponse => {
                let request_id = payload_reader.read_u64()?;
                let success = payload_reader.read_bool()?;
                Packet::JoinVoiceChannelResponse { request_id, success }
            }
            PacketId::LeaveVoiceChannelResponse => {
                let request_id = payload_reader.read_u64()?;
                let success = payload_reader.read_bool()?;
                Packet::LeaveVoiceChannelResponse { request_id, success }
            }
            PacketId::ChatMessageResponse => {
                let request_id = payload_reader.read_u64()?;
                let success = payload_reader.read_bool()?;
                Packet::ChatMessageResponse { request_id, success }
            }
            PacketId::UserJoinedServer => {
                let user_id = payload_reader.read_u64()?;
                let username = payload_reader.read_string()?;
                let in_voice = payload_reader.read_bool()?;
                let is_muted = payload_reader.read_bool()?;
                let participant = ParticipantInfo { user_id, username, in_voice, is_muted };
                Packet::UserJoinedServer { participant }
            }
            PacketId::UserJoinedVoice => {
                let user_id = payload_reader.read_u64()?;
                Packet::UserJoinedVoice { user_id }
            }
            PacketId::UserLeftVoice => {
                let user_id = payload_reader.read_u64()?;
                Packet::UserLeftVoice { user_id }
            }
            PacketId::UserLeftServer => {
                let user_id = payload_reader.read_u64()?;
                Packet::UserLeftServer { user_id }
            }
            PacketId::UserSentMessage => {
                let user_id = payload_reader.read_u64()?;
                let timestamp = payload_reader.read_u64()?;
                let message = payload_reader.read_string()?;
                Packet::UserSentMessage { user_id, timestamp, message }
            }
            PacketId::UserMuteState => {
                let user_id = payload_reader.read_u64()?;
                let is_muted = payload_reader.read_bool()?;
                Packet::UserMuteState { user_id, is_muted }
            }
            PacketId::VoiceData => {
                let user_id = payload_reader.read_u64()?;
                let sequence = payload_reader.read_u32()?;
                let timestamp = payload_reader.read_u32()?;
                let data = payload_reader.remaining().to_vec();
                Packet::VoiceData { user_id, sequence, timestamp, data }
            }
        };

        Ok((packet, reader.position() + payload_len))
    }

    pub fn id(&self) -> u8 {
        match self {
            Packet::LoginRequest { .. } => PacketId::LoginRequest,
            Packet::VoiceAuthRequest { .. } => PacketId::VoiceAuthRequest,
            Packet::JoinVoiceChannelRequest { .. } => PacketId::JoinVoiceChannelRequest,
            Packet::LeaveVoiceChannelRequest { .. } => PacketId::LeaveVoiceChannelRequest,
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
            Packet::UserMuteState { .. } => PacketId::UserMuteState,
            Packet::VoiceData { .. } => PacketId::VoiceData,
        }.as_u8()
    }

    /// Get request_id if this packet has one.
    ///
    /// Returns Some(request_id) for request/response packets, None for events and voice data.
    pub fn request_id(&self) -> Option<u64> {
        match self {
            Packet::LoginRequest { request_id, .. } => Some(*request_id),
            Packet::VoiceAuthRequest { request_id, .. } => Some(*request_id),
            Packet::JoinVoiceChannelRequest { request_id } => Some(*request_id),
            Packet::LeaveVoiceChannelRequest { request_id } => Some(*request_id),
            Packet::ChatMessageRequest { request_id, .. } => Some(*request_id),
            Packet::LoginResponse { request_id, .. } => Some(*request_id),
            Packet::VoiceAuthResponse { request_id, .. } => Some(*request_id),
            Packet::JoinVoiceChannelResponse { request_id, .. } => Some(*request_id),
            Packet::LeaveVoiceChannelResponse { request_id, .. } => Some(*request_id),
            Packet::ChatMessageResponse { request_id, .. } => Some(*request_id),
            Packet::UserJoinedServer { .. } => None,
            Packet::UserJoinedVoice { .. } => None,
            Packet::UserLeftVoice { .. } => None,
            Packet::UserLeftServer { .. } => None,
            Packet::UserSentMessage { .. } => None,
            Packet::UserMuteState { .. } => None,
            Packet::VoiceData { .. } => None,
        }
    }
}

#[cfg(test)]
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
        roundtrip(Packet::VoiceAuthResponse { request_id: 4, success: true });
        roundtrip(Packet::VoiceAuthResponse { request_id: 5, success: false });
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
            username: "".to_string(),
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