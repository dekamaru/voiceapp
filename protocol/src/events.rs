//! Event packet payload definitions

use std::io;

use crate::packet_id::{serialize_packet, PacketId};

/// Participant info with voice channel status
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParticipantInfo {
    pub user_id: u64,
    pub username: String,
    pub in_voice: bool,
}

// Encode functions

/// Encode user joined server event packet
/// Format: [packet_id: u8][payload_len: u16][user_id: u64 BE][username...null]
pub fn encode_user_joined_server(user_id: u64, username: &str) -> Vec<u8> {
    let mut payload = Vec::new();
    payload.extend_from_slice(&user_id.to_be_bytes());
    payload.extend_from_slice(username.as_bytes());
    payload.push(0); // null terminator
    serialize_packet(PacketId::UserJoinedServer, &payload)
}

/// Encode user joined voice event packet
/// Format: [packet_id: u8][payload_len: u16][user_id: u64 BE]
pub fn encode_user_joined_voice(user_id: u64) -> Vec<u8> {
    let payload = user_id.to_be_bytes().to_vec();
    serialize_packet(PacketId::UserJoinedVoice, &payload)
}

/// Encode user left voice event packet
/// Format: [packet_id: u8][payload_len: u16][user_id: u64 BE]
pub fn encode_user_left_voice(user_id: u64) -> Vec<u8> {
    let payload = user_id.to_be_bytes().to_vec();
    serialize_packet(PacketId::UserLeftVoice, &payload)
}

/// Encode user left server event packet
/// Format: [packet_id: u8][payload_len: u16][user_id: u64 BE]
pub fn encode_user_left_server(user_id: u64) -> Vec<u8> {
    let payload = user_id.to_be_bytes().to_vec();
    serialize_packet(PacketId::UserLeftServer, &payload)
}

// Decode functions

/// Decode user joined server event payload
/// Format: [user_id: u64 BE][username...null]
pub fn decode_user_joined_server(data: &[u8]) -> io::Result<(u64, String)> {
    if data.len() < 9 {
        // minimum: 8 bytes (user_id) + 1 byte (null terminator)
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "user joined server payload too short",
        ));
    }

    let user_id = u64::from_be_bytes(data[0..8].try_into().unwrap());

    let username_end = data.len() - 1;
    if data[username_end] != 0 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "username not null-terminated",
        ));
    }

    let username_bytes = &data[8..username_end];
    let username = String::from_utf8(username_bytes.to_vec())
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "invalid UTF-8"))?;

    Ok((user_id, username))
}

/// Decode user joined voice event payload
/// Format: [user_id: u64 BE]
pub fn decode_user_joined_voice(data: &[u8]) -> io::Result<u64> {
    if data.len() < 8 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "user joined voice payload too short",
        ));
    }

    Ok(u64::from_be_bytes(data[0..8].try_into().unwrap()))
}

/// Decode user left voice event payload
/// Format: [user_id: u64 BE]
pub fn decode_user_left_voice(data: &[u8]) -> io::Result<u64> {
    if data.len() < 8 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "user left voice payload too short",
        ));
    }

    Ok(u64::from_be_bytes(data[0..8].try_into().unwrap()))
}

/// Decode user left server event payload
/// Format: [user_id: u64 BE]
pub fn decode_user_left_server(data: &[u8]) -> io::Result<u64> {
    if data.len() < 8 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "user left server payload too short",
        ));
    }

    Ok(u64::from_be_bytes(data[0..8].try_into().unwrap()))
}

/// Encode user sent message event packet
/// Format: [packet_id: u8][payload_len: u16][user_id: u64 BE][timestamp: u64 BE][message_len: u16 BE][message...]
pub fn encode_user_sent_message(user_id: u64, timestamp: u64, message: &str) -> Vec<u8> {
    let mut payload = Vec::new();
    payload.extend_from_slice(&user_id.to_be_bytes());
    payload.extend_from_slice(&timestamp.to_be_bytes());
    let message_bytes = message.as_bytes();
    payload.extend_from_slice(&(message_bytes.len() as u16).to_be_bytes());
    payload.extend_from_slice(message_bytes);
    serialize_packet(PacketId::UserSentMessage, &payload)
}

/// Decode user sent message event payload
/// Format: [user_id: u64 BE][timestamp: u64 BE][message_len: u16 BE][message...]
pub fn decode_user_sent_message(data: &[u8]) -> io::Result<(u64, u64, String)> {
    if data.len() < 18 {
        // minimum: 8 bytes (user_id) + 8 bytes (timestamp) + 2 bytes (message_len)
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "user sent message payload too short",
        ));
    }

    let user_id = u64::from_be_bytes(data[0..8].try_into().unwrap());
    let timestamp = u64::from_be_bytes(data[8..16].try_into().unwrap());
    let message_len = u16::from_be_bytes(data[16..18].try_into().unwrap()) as usize;

    if data.len() < 18 + message_len {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "incomplete message in user sent message event",
        ));
    }

    let message = String::from_utf8(data[18..18 + message_len].to_vec())
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "invalid UTF-8 in message"))?;

    Ok((user_id, timestamp, message))
}
