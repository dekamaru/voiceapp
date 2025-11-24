//! Response packet payload definitions

use std::io;

use crate::packet_id::{serialize_packet, PacketId};
use crate::events::ParticipantInfo;

/// Response to login request, contains user id, voice token, and current participants
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoginResponse {
    pub id: u64,
    pub voice_token: u64,
    pub participants: Vec<ParticipantInfo>,
}

// Encode functions

/// Encode login response packet
/// Format: [packet_id: u8][payload_len: u16][id: u64 BE][voice_token: u64 BE][count: u16]
///         [user_id: u64 BE][username_len: u16 BE][username: bytes][in_voice: u8]...
pub fn encode_login_response(id: u64, voice_token: u64, participants: &[ParticipantInfo]) -> Vec<u8> {
    let mut payload = Vec::new();
    payload.extend_from_slice(&id.to_be_bytes());
    payload.extend_from_slice(&voice_token.to_be_bytes());

    // Encode participant list
    payload.extend_from_slice(&(participants.len() as u16).to_be_bytes());
    for participant in participants {
        payload.extend_from_slice(&participant.user_id.to_be_bytes());

        let username_bytes = participant.username.as_bytes();
        payload.extend_from_slice(&(username_bytes.len() as u16).to_be_bytes());
        payload.extend_from_slice(username_bytes);

        payload.push(if participant.in_voice { 1 } else { 0 });
    }

    serialize_packet(PacketId::LoginResponse, &payload)
}

/// Encode UDP auth response packet
/// Format: [packet_id: u8][payload_len: u16][success: u8]
pub fn encode_voice_auth_response(success: bool) -> Vec<u8> {
    let payload = vec![if success { 1u8 } else { 0u8 }];
    serialize_packet(PacketId::VoiceAuthResponse, &payload)
}

/// Encode join voice channel response packet
/// Format: [packet_id: u8][payload_len: u16][success: u8]
pub fn encode_join_voice_channel_response(success: bool) -> Vec<u8> {
    let payload = vec![if success { 1u8 } else { 0u8 }];
    serialize_packet(PacketId::JoinVoiceChannelResponse, &payload)
}

/// Encode leave voice channel response packet
/// Format: [packet_id: u8][payload_len: u16][success: u8]
pub fn encode_leave_voice_channel_response(success: bool) -> Vec<u8> {
    let payload = vec![if success { 1u8 } else { 0u8 }];
    serialize_packet(PacketId::LeaveVoiceChannelResponse, &payload)
}

// Decode functions

/// Decode login response payload
/// Format: [id: u64 BE][voice_token: u64 BE][count: u16]
///         [user_id: u64 BE][username_len: u16 BE][username: bytes][in_voice: u8]...
pub fn decode_login_response(data: &[u8]) -> io::Result<LoginResponse> {
    if data.len() < 18 {
        // 8 bytes (id) + 8 bytes (voice_token) + 2 bytes (count)
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "login response payload too short",
        ));
    }

    let id = u64::from_be_bytes(data[0..8].try_into().unwrap());
    let voice_token = u64::from_be_bytes(data[8..16].try_into().unwrap());

    // Decode participant list
    let count = u16::from_be_bytes(data[16..18].try_into().unwrap()) as usize;
    let mut participants = Vec::new();
    let mut pos = 18;

    for _ in 0..count {
        if pos + 10 > data.len() {
            // minimum: 8 bytes (user_id) + 2 bytes (username_len) + 0 bytes (username) + 1 byte (in_voice)
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "incomplete participant entry in login response",
            ));
        }

        let user_id = u64::from_be_bytes(data[pos..pos + 8].try_into().unwrap());
        pos += 8;

        let username_len = u16::from_be_bytes(data[pos..pos + 2].try_into().unwrap()) as usize;
        pos += 2;

        if pos + username_len + 1 > data.len() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "incomplete username in participant entry",
            ));
        }

        let username = String::from_utf8(data[pos..pos + username_len].to_vec())
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "invalid UTF-8 in username"))?;
        pos += username_len;

        let in_voice = data[pos] != 0;
        pos += 1;

        participants.push(ParticipantInfo { user_id, username, in_voice });
    }

    Ok(LoginResponse { id, voice_token, participants })
}

/// Decode UDP auth response payload
/// Format: [success: u8]
pub fn decode_voice_auth_response(data: &[u8]) -> io::Result<bool> {
    if data.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "auth response payload too short",
        ));
    }

    Ok(data[0] != 0)
}

/// Decode join voice channel response payload
/// Format: [success: u8]
pub fn decode_join_voice_channel_response(data: &[u8]) -> io::Result<bool> {
    if data.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "join voice response payload too short",
        ));
    }

    Ok(data[0] != 0)
}

/// Decode leave voice channel response payload
/// Format: [success: u8]
pub fn decode_leave_voice_channel_response(data: &[u8]) -> io::Result<bool> {
    if data.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "leave voice response payload too short",
        ));
    }

    Ok(data[0] != 0)
}
