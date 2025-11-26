//! Packet ID definitions and parsing

use std::io;

/// Unified packet ID space for all packet types (requests, responses, events)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PacketId {
    // Requests (0x01-0x10)
    LoginRequest = 0x01,
    JoinVoiceChannelRequest = 0x02,
    VoiceAuthRequest = 0x03,
    VoiceData = 0x04,
    LeaveVoiceChannelRequest = 0x05,
    ChatMessageRequest = 0x06,

    // Responses (0x11-0x20)
    LoginResponse = 0x11,
    VoiceAuthResponse = 0x12,
    JoinVoiceChannelResponse = 0x13,
    LeaveVoiceChannelResponse = 0x14,
    ChatMessageResponse = 0x15,

    // Events (0x21-0x40)
    UserJoinedServer = 0x21,
    UserJoinedVoice = 0x22,
    UserLeftVoice = 0x23,
    UserLeftServer = 0x24,
    UserSentMessage = 0x25,
}

impl PacketId {
    pub fn from_u8(value: u8) -> Option<Self> {
        match value {
            0x01 => Some(PacketId::LoginRequest),
            0x02 => Some(PacketId::JoinVoiceChannelRequest),
            0x03 => Some(PacketId::VoiceAuthRequest),
            0x04 => Some(PacketId::VoiceData),
            0x05 => Some(PacketId::LeaveVoiceChannelRequest),
            0x06 => Some(PacketId::ChatMessageRequest),
            0x11 => Some(PacketId::LoginResponse),
            0x12 => Some(PacketId::VoiceAuthResponse),
            0x13 => Some(PacketId::JoinVoiceChannelResponse),
            0x14 => Some(PacketId::LeaveVoiceChannelResponse),
            0x15 => Some(PacketId::ChatMessageResponse),
            0x21 => Some(PacketId::UserJoinedServer),
            0x22 => Some(PacketId::UserJoinedVoice),
            0x23 => Some(PacketId::UserLeftVoice),
            0x24 => Some(PacketId::UserLeftServer),
            0x25 => Some(PacketId::UserSentMessage),
            _ => None,
        }
    }

    pub fn as_u8(self) -> u8 {
        self as u8
    }
}

/// Parse packet ID and return remaining payload bytes
/// Format: [packet_id: u8][payload_len: u16][payload...]
pub fn parse_packet(buf: &[u8]) -> io::Result<(PacketId, &[u8])> {
    if buf.len() < 3 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "packet too short",
        ));
    }

    let packet_id_byte = buf[0];
    let packet_id = PacketId::from_u8(packet_id_byte)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "unknown packet id"))?;

    let payload_len = u16::from_be_bytes(buf[1..3].try_into().unwrap()) as usize;

    if buf.len() < 3 + payload_len {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "incomplete payload",
        ));
    }

    let payload = &buf[3..3 + payload_len];
    Ok((packet_id, payload))
}

/// Serialize a packet with given payload
/// Format: [packet_id: u8][payload_len: u16][payload...]
pub fn serialize_packet(id: PacketId, payload: &[u8]) -> Vec<u8> {
    let mut buf = Vec::new();
    buf.push(id.as_u8());
    buf.extend_from_slice(&(payload.len() as u16).to_be_bytes());
    buf.extend_from_slice(payload);
    buf
}