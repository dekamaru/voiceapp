//! Request packet payload definitions

use std::io;

use crate::packet_id::{serialize_packet, PacketId};

/// Voice audio frame transmission
#[derive(Debug, Clone)]
pub struct VoiceData {
    pub sequence: u32,
    pub timestamp: u32,
    pub ssrc: u64,
    pub opus_frame: Vec<u8>,
}

// Encode functions

/// Encode login request packet
/// Format: [packet_id: u8][payload_len: u16][username...null]
pub fn encode_login_request(username: &str) -> io::Result<Vec<u8>> {
    let mut payload = username.as_bytes().to_vec();
    payload.push(0); // null terminator
    serialize_packet(PacketId::LoginRequest, &payload)
}

/// Encode join voice channel request packet
/// Format: [packet_id: u8][payload_len: u16]
pub fn encode_join_voice_channel_request() -> io::Result<Vec<u8>> {
    serialize_packet(PacketId::JoinVoiceChannelRequest, &[])
}

/// Encode UDP auth request packet
/// Format: [packet_id: u8][payload_len: u16][token: u64 BE]
pub fn encode_voice_auth_request(voice_token: u64) -> io::Result<Vec<u8>> {
    let payload = voice_token.to_be_bytes().to_vec();
    serialize_packet(PacketId::VoiceAuthRequest, &payload)
}

/// Encode voice frame request packet
/// Format: [packet_id: u8][payload_len: u16][sequence: u32 BE][timestamp: u32 BE][ssrc: u64 BE][opus_frame...]
pub fn encode_voice_data(payload: &VoiceData) -> io::Result<Vec<u8>> {
    let mut data = Vec::new();
    data.extend_from_slice(&payload.sequence.to_be_bytes());
    data.extend_from_slice(&payload.timestamp.to_be_bytes());
    data.extend_from_slice(&payload.ssrc.to_be_bytes());
    data.extend_from_slice(&payload.opus_frame);
    serialize_packet(PacketId::VoiceData, &data)
}

/// Encode leave voice channel request packet
/// Format: [packet_id: u8][payload_len: u16]
pub fn encode_leave_voice_channel_request() -> io::Result<Vec<u8>> {
    serialize_packet(PacketId::LeaveVoiceChannelRequest, &[])
}

// Decode functions

/// Decode login request payload
/// Format: [username...null]
pub fn decode_login_request(data: &[u8]) -> io::Result<String> {
    if data.is_empty() || data[data.len() - 1] != 0 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "username not null-terminated",
        ));
    }
    let username_bytes = &data[..data.len() - 1];
    String::from_utf8(username_bytes.to_vec())
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "invalid UTF-8"))
}

/// Decode UDP auth request payload
/// Format: [token: u64 BE]
pub fn decode_voice_auth_request(data: &[u8]) -> io::Result<u64> {
    if data.len() < 8 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "token payload too short",
        ));
    }
    Ok(u64::from_be_bytes(data[0..8].try_into().unwrap()))
}

/// Decode voice frame request payload
/// Format: [sequence: u32 BE][timestamp: u32 BE][ssrc: u64 BE][opus_frame...]
pub fn decode_voice_data(data: &[u8]) -> io::Result<VoiceData> {
    if data.len() < 16 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "voice frame too short",
        ));
    }

    let sequence = u32::from_be_bytes(data[0..4].try_into().unwrap());
    let timestamp = u32::from_be_bytes(data[4..8].try_into().unwrap());
    let ssrc = u64::from_be_bytes(data[8..16].try_into().unwrap());
    let opus_frame = data[16..].to_vec();

    Ok(VoiceData {
        sequence,
        timestamp,
        ssrc,
        opus_frame,
    })
}
