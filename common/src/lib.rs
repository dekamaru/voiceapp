//! VoiceApp common protocol definitions
//!
//! This module defines the binary protocol used for communication between
//! client and server over TCP (control) and UDP (voice).

use std::io::{self, Write};

// Protocol version
pub const PROTOCOL_VERSION: u8 = 1;

/// Compute a deterministic SSRC from a username using FNV-1a hashing
/// This allows lightweight voice packets to contain only SSRC instead of full username
pub fn username_to_ssrc(username: &str) -> u32 {
    const FNV_OFFSET_BASIS: u32 = 2166136261;
    const FNV_PRIME: u32 = 16777619;

    let mut hash = FNV_OFFSET_BASIS;
    for byte in username.as_bytes() {
        hash ^= *byte as u32;
        hash = hash.wrapping_mul(FNV_PRIME);
    }
    hash
}

// Packet type IDs for TCP control messages
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PacketTypeId {
    Login = 0x01,
    LoginResponse = 0x02,
    UserJoinedServer = 0x03,
    JoinVoiceChannel = 0x04,
    UserJoinedVoice = 0x05,
    UserLeftVoice = 0x06,
    UserLeftServer = 0x07,
    ServerParticipantList = 0x08,
}

impl PacketTypeId {
    pub fn from_u8(value: u8) -> Option<Self> {
        match value {
            0x01 => Some(PacketTypeId::Login),
            0x02 => Some(PacketTypeId::LoginResponse),
            0x03 => Some(PacketTypeId::UserJoinedServer),
            0x04 => Some(PacketTypeId::JoinVoiceChannel),
            0x05 => Some(PacketTypeId::UserJoinedVoice),
            0x06 => Some(PacketTypeId::UserLeftVoice),
            0x07 => Some(PacketTypeId::UserLeftServer),
            0x08 => Some(PacketTypeId::ServerParticipantList),
            _ => None,
        }
    }

    pub fn as_u8(self) -> u8 {
        self as u8
    }
}

/// TCP control packet structure
/// Format: [version: u8][packet_type: u8][payload_len: u16][payload...]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TcpPacket {
    pub packet_type: PacketTypeId,
    pub payload: Vec<u8>,
}

impl TcpPacket {
    pub fn new(packet_type: PacketTypeId, payload: Vec<u8>) -> Self {
        TcpPacket {
            packet_type,
            payload,
        }
    }

    /// Encode packet to binary format
    pub fn encode(&self) -> io::Result<Vec<u8>> {
        let mut buf = Vec::new();
        buf.write_all(&[PROTOCOL_VERSION])?;
        buf.write_all(&[self.packet_type.as_u8()])?;
        buf.write_all(&(self.payload.len() as u16).to_be_bytes())?;
        buf.write_all(&self.payload)?;
        Ok(buf)
    }

    /// Decode packet from binary format
    pub fn decode(data: &[u8]) -> io::Result<(Self, usize)> {
        if data.len() < 4 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "packet too short",
            ));
        }

        let mut pos = 0;
        let version = data[pos];
        pos += 1;

        if version != PROTOCOL_VERSION {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "unsupported protocol version",
            ));
        }

        let packet_type_byte = data[pos];
        pos += 1;

        let packet_type = PacketTypeId::from_u8(packet_type_byte)
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "unknown packet type"))?;

        let payload_len = u16::from_be_bytes([data[pos], data[pos + 1]]) as usize;
        pos += 2;

        if data.len() < pos + payload_len {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "incomplete payload",
            ));
        }

        let payload = data[pos..pos + payload_len].to_vec();
        pos += payload_len;

        Ok((
            TcpPacket {
                packet_type,
                payload,
            },
            pos,
        ))
    }
}

/// Helper to encode username into a payload (null-terminated UTF-8)
pub fn encode_username(username: &str) -> Vec<u8> {
    let mut buf = username.as_bytes().to_vec();
    buf.push(0); // null terminator
    buf
}

/// Helper to decode username from a payload
pub fn decode_username(data: &[u8]) -> io::Result<String> {
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

/// Helper to encode username with UDP port into a payload
/// Format: [username...null][port: u16 BE]
pub fn encode_username_with_udp_port(username: &str, udp_port: u16) -> io::Result<Vec<u8>> {
    let mut buf = Vec::new();
    buf.write_all(username.as_bytes())?;
    buf.push(0); // null terminator
    buf.write_all(&udp_port.to_be_bytes())?;
    Ok(buf)
}

/// Helper to decode username with UDP port from a payload
pub fn decode_username_with_udp_port(data: &[u8]) -> io::Result<(String, u16)> {
    if data.len() < 3 {
        // minimum: 1 byte (null terminator) + 2 bytes (port)
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "username with UDP port too short",
        ));
    }

    // Find the null terminator
    let mut pos = 0;
    while pos < data.len() && data[pos] != 0 {
        pos += 1;
    }

    if pos >= data.len() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "username not null-terminated",
        ));
    }

    let username_bytes = &data[..pos];
    let username = String::from_utf8(username_bytes.to_vec())
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "invalid UTF-8"))?;
    pos += 1; // skip null terminator

    if data.len() < pos + 2 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "missing UDP port in payload",
        ));
    }

    let udp_port = u16::from_be_bytes([data[pos], data[pos + 1]]);

    Ok((username, udp_port))
}

/// Helper to encode username with SSRC into a payload
/// Format: [username...null][ssrc: u32]
pub fn encode_username_with_ssrc(username: &str, ssrc: u32) -> io::Result<Vec<u8>> {
    let mut buf = Vec::new();
    buf.write_all(username.as_bytes())?;
    buf.push(0); // null terminator
    buf.write_all(&ssrc.to_be_bytes())?;
    Ok(buf)
}

/// Helper to decode username with SSRC from a payload
pub fn decode_username_with_ssrc(data: &[u8]) -> io::Result<(String, u32)> {
    if data.len() < 5 {
        // minimum: 1 byte (null terminator) + 4 bytes (SSRC)
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "username with SSRC too short",
        ));
    }

    // Find the null terminator
    let mut pos = 0;
    while pos < data.len() && data[pos] != 0 {
        pos += 1;
    }

    if pos >= data.len() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "username not null-terminated",
        ));
    }

    let username_bytes = &data[..pos];
    let username = String::from_utf8(username_bytes.to_vec())
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "invalid UTF-8"))?;
    pos += 1; // skip null terminator

    if data.len() < pos + 4 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "missing SSRC in payload",
        ));
    }

    let ssrc = u32::from_be_bytes([data[pos], data[pos + 1], data[pos + 2], data[pos + 3]]);

    Ok((username, ssrc))
}

/// Participant info with voice channel status
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParticipantInfo {
    pub username: String,
    pub in_voice: bool,
}

/// Helper to encode participant list with voice status into a payload
/// Format: [count: u16][username1...null][in_voice: u8][username2...null][in_voice: u8]...
pub fn encode_participant_list_with_voice(participants: &[ParticipantInfo]) -> io::Result<Vec<u8>> {
    let mut buf = Vec::new();
    buf.write_all(&(participants.len() as u16).to_be_bytes())?;
    for participant in participants {
        buf.write_all(participant.username.as_bytes())?;
        buf.push(0); // null terminator for username
        buf.push(if participant.in_voice { 1 } else { 0 }); // voice status
    }
    Ok(buf)
}

/// Helper to decode participant list with voice status from a payload
pub fn decode_participant_list_with_voice(data: &[u8]) -> io::Result<Vec<ParticipantInfo>> {
    if data.len() < 2 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "participant list too short",
        ));
    }

    let count = u16::from_be_bytes([data[0], data[1]]) as usize;
    let mut participants = Vec::new();
    let mut pos = 2;

    for _ in 0..count {
        // Find the null terminator
        let start = pos;
        while pos < data.len() && data[pos] != 0 {
            pos += 1;
        }

        if pos >= data.len() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "incomplete username in participant list",
            ));
        }

        let username_bytes = &data[start..pos];
        let username = String::from_utf8(username_bytes.to_vec())
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "invalid UTF-8"))?;
        pos += 1; // skip null terminator

        if pos >= data.len() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "missing voice status in participant list",
            ));
        }

        let in_voice = data[pos] != 0;
        pos += 1;

        participants.push(ParticipantInfo { username, in_voice });
    }

    Ok(participants)
}

/// Helper to encode voice token into a payload
/// Format: [token: u64 BE]
pub fn encode_voice_token(token: u64) -> io::Result<Vec<u8>> {
    let mut buf = Vec::new();
    buf.write_all(&token.to_be_bytes())?;
    Ok(buf)
}

/// Helper to decode voice token from a payload
pub fn decode_voice_token(data: &[u8]) -> io::Result<u64> {
    if data.len() < 8 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "token payload too short",
        ));
    }
    Ok(u64::from_be_bytes([
        data[0], data[1], data[2], data[3], data[4], data[5], data[6], data[7],
    ]))
}

/// Helper to encode participant list into a payload (legacy, no voice status)
/// Format: [count: u16][username1...null][username2...null]...[usernameN...null]
pub fn encode_participant_list(usernames: &[&str]) -> io::Result<Vec<u8>> {
    let mut buf = Vec::new();
    buf.write_all(&(usernames.len() as u16).to_be_bytes())?;
    for username in usernames {
        buf.write_all(username.as_bytes())?;
        buf.push(0); // null terminator for each username
    }
    Ok(buf)
}

/// Helper to decode participant list from a payload (legacy, no voice status)
pub fn decode_participant_list(data: &[u8]) -> io::Result<Vec<String>> {
    if data.len() < 2 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "participant list too short",
        ));
    }

    let count = u16::from_be_bytes([data[0], data[1]]) as usize;
    let mut usernames = Vec::new();
    let mut pos = 2;

    for _ in 0..count {
        // Find the null terminator
        let start = pos;
        while pos < data.len() && data[pos] != 0 {
            pos += 1;
        }

        if pos >= data.len() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "incomplete username in participant list",
            ));
        }

        let username_bytes = &data[start..pos];
        let username = String::from_utf8(username_bytes.to_vec())
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "invalid UTF-8"))?;
        usernames.push(username);
        pos += 1; // skip null terminator
    }

    Ok(usernames)
}

/// UDP authentication packet structure
/// Format: [version: u8][token: u64][username...null]
/// Sent once when client joins voice channel to authenticate
#[derive(Debug, Clone)]
pub struct UdpAuthPacket {
    pub token: u64,
    pub username: String,
}

/// UDP authentication response packet structure
/// Format: [version: u8][success: u8]
/// Server responds with success (1) or failure (0)
#[derive(Debug, Clone)]
pub struct UdpAuthResponse {
    pub success: bool,
}

impl UdpAuthResponse {
    pub fn new(success: bool) -> Self {
        UdpAuthResponse { success }
    }

    /// Encode response to binary format
    pub fn encode(&self) -> io::Result<Vec<u8>> {
        let mut buf = Vec::new();
        buf.write_all(&[PROTOCOL_VERSION])?;
        buf.write_all(&[if self.success { 1u8 } else { 0u8 }])?;
        Ok(buf)
    }

    /// Decode response from binary format
    pub fn decode(data: &[u8]) -> io::Result<Self> {
        if data.len() < 2 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "auth response too short",
            ));
        }

        let version = data[0];
        if version != PROTOCOL_VERSION {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "unsupported protocol version",
            ));
        }

        let success = data[1] != 0;
        Ok(UdpAuthResponse { success })
    }
}

impl UdpAuthPacket {
    pub fn new(token: u64, username: String) -> Self {
        UdpAuthPacket { token, username }
    }

    /// Encode auth packet to binary format
    pub fn encode(&self) -> io::Result<Vec<u8>> {
        let mut buf = Vec::new();
        buf.write_all(&[PROTOCOL_VERSION])?;
        buf.write_all(&self.token.to_be_bytes())?;
        buf.write_all(self.username.as_bytes())?;
        buf.push(0); // null terminator
        Ok(buf)
    }

    /// Decode auth packet from binary format
    pub fn decode(data: &[u8]) -> io::Result<(Self, usize)> {
        if data.len() < 10 {
            // 1 (version) + 8 (token) + 1 (null terminator minimum)
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "auth packet too short",
            ));
        }

        let mut pos = 0;
        let version = data[pos];
        pos += 1;

        if version != PROTOCOL_VERSION {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "unsupported protocol version",
            ));
        }

        let token = u64::from_be_bytes([
            data[pos],
            data[pos + 1],
            data[pos + 2],
            data[pos + 3],
            data[pos + 4],
            data[pos + 5],
            data[pos + 6],
            data[pos + 7],
        ]);
        pos += 8;

        // Find null terminator
        let start = pos;
        while pos < data.len() && data[pos] != 0 {
            pos += 1;
        }

        if pos >= data.len() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "username not null-terminated in auth packet",
            ));
        }

        let username_bytes = &data[start..pos];
        let username = String::from_utf8(username_bytes.to_vec())
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "invalid UTF-8 in username"))?;
        pos += 1; // skip null terminator

        Ok((UdpAuthPacket { token, username }, pos))
    }
}

/// UDP voice packet structure
/// Format: [version: u8][sequence: u32][timestamp: u32][ssrc: u32][opus_frame...]
/// Lightweight format: SSRC is computed from username, not transmitted
#[derive(Debug, Clone)]
pub struct VoicePacket {
    pub sequence: u32,
    pub timestamp: u32,
    pub ssrc: u32, // SSRC computed from sender's username
    pub opus_frame: Vec<u8>,
}

impl VoicePacket {
    pub fn new(sequence: u32, timestamp: u32, ssrc: u32, opus_frame: Vec<u8>) -> Self {
        VoicePacket {
            sequence,
            timestamp,
            ssrc,
            opus_frame,
        }
    }

    /// Encode voice packet to binary format
    pub fn encode(&self) -> io::Result<Vec<u8>> {
        let mut buf = Vec::new();
        buf.write_all(&[PROTOCOL_VERSION])?;
        buf.write_all(&self.sequence.to_be_bytes())?;
        buf.write_all(&self.timestamp.to_be_bytes())?;
        buf.write_all(&self.ssrc.to_be_bytes())?;
        buf.write_all(&self.opus_frame)?;
        Ok(buf)
    }

    /// Decode voice packet from binary format
    pub fn decode(data: &[u8]) -> io::Result<(Self, usize)> {
        if data.len() < 13 {
            // 1 (version) + 4 (seq) + 4 (ts) + 4 (ssrc)
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "voice packet too short",
            ));
        }

        let mut pos = 0;
        let version = data[pos];
        pos += 1;

        if version != PROTOCOL_VERSION {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "unsupported protocol version",
            ));
        }

        let sequence = u32::from_be_bytes([data[pos], data[pos + 1], data[pos + 2], data[pos + 3]]);
        pos += 4;

        let timestamp = u32::from_be_bytes([data[pos], data[pos + 1], data[pos + 2], data[pos + 3]]);
        pos += 4;

        let ssrc = u32::from_be_bytes([data[pos], data[pos + 1], data[pos + 2], data[pos + 3]]);
        pos += 4;

        let opus_frame = data[pos..].to_vec();
        let final_pos = data.len();

        Ok((
            VoicePacket {
                sequence,
                timestamp,
                ssrc,
                opus_frame,
            },
            final_pos,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_packet_type_id_conversions() {
        assert_eq!(PacketTypeId::Login.as_u8(), 0x01);
        assert_eq!(PacketTypeId::LoginResponse.as_u8(), 0x02);
        assert_eq!(PacketTypeId::UserJoinedServer.as_u8(), 0x03);
        assert_eq!(PacketTypeId::JoinVoiceChannel.as_u8(), 0x04);
        assert_eq!(PacketTypeId::UserJoinedVoice.as_u8(), 0x05);
        assert_eq!(PacketTypeId::UserLeftVoice.as_u8(), 0x06);
        assert_eq!(PacketTypeId::UserLeftServer.as_u8(), 0x07);
        assert_eq!(PacketTypeId::ServerParticipantList.as_u8(), 0x08);

        assert_eq!(PacketTypeId::from_u8(0x01), Some(PacketTypeId::Login));
        assert_eq!(PacketTypeId::from_u8(0x02), Some(PacketTypeId::LoginResponse));
        assert_eq!(PacketTypeId::from_u8(0x06), Some(PacketTypeId::UserLeftVoice));
        assert_eq!(PacketTypeId::from_u8(0x07), Some(PacketTypeId::UserLeftServer));
        assert_eq!(PacketTypeId::from_u8(0x08), Some(PacketTypeId::ServerParticipantList));
        assert_eq!(PacketTypeId::from_u8(0xFF), None);
    }

    #[test]
    fn test_encode_decode_tcp_packet_login() {
        let username = "alice";
        let payload = encode_username(username);
        let packet = TcpPacket::new(PacketTypeId::Login, payload);

        let encoded = packet.encode().expect("encode failed");
        let (decoded, bytes_read) = TcpPacket::decode(&encoded).expect("decode failed");

        assert_eq!(packet, decoded);
        assert_eq!(bytes_read, encoded.len());
    }

    #[test]
    fn test_encode_decode_tcp_packet_user_joined() {
        let username = "bob";
        let payload = encode_username(username);
        let packet = TcpPacket::new(PacketTypeId::UserJoinedServer, payload);

        let encoded = packet.encode().expect("encode failed");
        let (decoded, bytes_read) = TcpPacket::decode(&encoded).expect("decode failed");

        assert_eq!(packet, decoded);
        assert_eq!(bytes_read, encoded.len());
    }

    #[test]
    fn test_encode_decode_tcp_packet_join_voice() {
        let username = "charlie";
        let payload = encode_username(username);
        let packet = TcpPacket::new(PacketTypeId::JoinVoiceChannel, payload);

        let encoded = packet.encode().expect("encode failed");
        let (decoded, bytes_read) = TcpPacket::decode(&encoded).expect("decode failed");

        assert_eq!(packet, decoded);
        assert_eq!(bytes_read, encoded.len());
    }

    #[test]
    fn test_encode_decode_tcp_packet_user_left_voice() {
        let username = "charlie";
        let payload = encode_username(username);
        let packet = TcpPacket::new(PacketTypeId::UserLeftVoice, payload);

        let encoded = packet.encode().expect("encode failed");
        let (decoded, bytes_read) = TcpPacket::decode(&encoded).expect("decode failed");

        assert_eq!(packet, decoded);
        assert_eq!(bytes_read, encoded.len());
    }

    #[test]
    fn test_encode_decode_tcp_packet_user_left_server() {
        let username = "dave";
        let payload = encode_username(username);
        let packet = TcpPacket::new(PacketTypeId::UserLeftServer, payload);

        let encoded = packet.encode().expect("encode failed");
        let (decoded, bytes_read) = TcpPacket::decode(&encoded).expect("decode failed");

        assert_eq!(packet, decoded);
        assert_eq!(bytes_read, encoded.len());
    }

    #[test]
    fn test_encode_decode_tcp_packet_empty_payload() {
        let packet = TcpPacket::new(PacketTypeId::UserLeftServer, vec![]);

        let encoded = packet.encode().expect("encode failed");
        let (decoded, bytes_read) = TcpPacket::decode(&encoded).expect("decode failed");

        assert_eq!(packet, decoded);
        assert_eq!(bytes_read, encoded.len());
    }

    #[test]
    fn test_decode_tcp_packet_too_short() {
        let data = vec![PROTOCOL_VERSION, 0x01];
        let result = TcpPacket::decode(&data);
        assert!(result.is_err());
    }

    #[test]
    fn test_decode_tcp_packet_bad_version() {
        let data = vec![PROTOCOL_VERSION + 1, 0, 0, 0, 0, 0x01, 0, 0];
        let result = TcpPacket::decode(&data);
        assert!(result.is_err());
    }

    #[test]
    fn test_decode_tcp_packet_unknown_type() {
        let mut data = vec![PROTOCOL_VERSION];
        data.extend_from_slice(&42u32.to_be_bytes()); // packet_id
        data.push(0xFF); // unknown packet type
        data.extend_from_slice(&0u16.to_be_bytes()); // payload_len
        let result = TcpPacket::decode(&data);
        assert!(result.is_err());
    }

    #[test]
    fn test_decode_tcp_packet_incomplete_payload() {
        let mut data = vec![PROTOCOL_VERSION];
        data.extend_from_slice(&42u32.to_be_bytes()); // packet_id
        data.push(0x01); // packet type
        data.extend_from_slice(&100u16.to_be_bytes()); // payload_len claims 100 bytes
        data.extend_from_slice(b"only_10"); // but only 7 bytes provided
        let result = TcpPacket::decode(&data);
        assert!(result.is_err());
    }

    #[test]
    fn test_encode_decode_username() {
        let username = "test_user";
        let encoded = encode_username(username);
        let decoded = decode_username(&encoded).expect("decode failed");
        assert_eq!(decoded, username);
    }

    #[test]
    fn test_decode_username_not_null_terminated() {
        let data = b"no_null".to_vec();
        let result = decode_username(&data);
        assert!(result.is_err());
    }

    #[test]
    fn test_decode_username_empty() {
        let result = decode_username(&[]);
        assert!(result.is_err());
    }

    #[test]
    fn test_decode_username_invalid_utf8() {
        let mut data = vec![0xFF, 0xFE];
        data.push(0); // null terminator
        let result = decode_username(&data);
        assert!(result.is_err());
    }

    #[test]
    fn test_encode_decode_username_with_ssrc() {
        let username = "alice";
        let ssrc = 12345u32;
        let encoded = encode_username_with_ssrc(username, ssrc).expect("encode failed");
        let (decoded_username, decoded_ssrc) = decode_username_with_ssrc(&encoded).expect("decode failed");
        assert_eq!(decoded_username, username);
        assert_eq!(decoded_ssrc, ssrc);
    }

    #[test]
    fn test_encode_decode_username_with_ssrc_max_values() {
        let username = "very_long_username_with_special_chars_こんにちは";
        let ssrc = u32::MAX;
        let encoded = encode_username_with_ssrc(username, ssrc).expect("encode failed");
        let (decoded_username, decoded_ssrc) = decode_username_with_ssrc(&encoded).expect("decode failed");
        assert_eq!(decoded_username, username);
        assert_eq!(decoded_ssrc, ssrc);
    }

    #[test]
    fn test_decode_username_with_ssrc_too_short() {
        let data = vec![0x61, 0x62, 0x63]; // "abc" without SSRC
        let result = decode_username_with_ssrc(&data);
        assert!(result.is_err());
    }

    #[test]
    fn test_decode_username_with_ssrc_not_null_terminated() {
        let mut data = vec![0x61, 0x62, 0x63]; // "abc" without null terminator
        data.extend_from_slice(&123u32.to_be_bytes());
        let result = decode_username_with_ssrc(&data);
        assert!(result.is_err());
    }

    #[test]
    fn test_encode_decode_participant_list_single() {
        let usernames = vec!["alice"];
        let encoded = encode_participant_list(&usernames).expect("encode failed");
        let decoded = decode_participant_list(&encoded).expect("decode failed");
        assert_eq!(decoded, usernames);
    }

    #[test]
    fn test_encode_decode_participant_list_multiple() {
        let usernames = vec!["alice", "bob", "charlie"];
        let encoded = encode_participant_list(&usernames).expect("encode failed");
        let decoded = decode_participant_list(&encoded).expect("decode failed");
        assert_eq!(decoded, usernames);
    }

    #[test]
    fn test_encode_decode_participant_list_empty() {
        let usernames: Vec<&str> = vec![];
        let encoded = encode_participant_list(&usernames).expect("encode failed");
        let decoded = decode_participant_list(&encoded).expect("decode failed");
        assert_eq!(decoded, usernames);
    }

    #[test]
    fn test_decode_participant_list_too_short() {
        let data = vec![0x01]; // says 1 participant but no data
        let result = decode_participant_list(&data);
        assert!(result.is_err());
    }

    #[test]
    fn test_decode_participant_list_incomplete() {
        let mut data = vec![0x00, 0x02]; // 2 participants
        data.extend_from_slice(b"alice\0"); // first participant
        // missing second participant
        let result = decode_participant_list(&data);
        assert!(result.is_err());
    }

    #[test]
    fn test_encode_decode_voice_packet() {
        let opus_frame = vec![0xAB, 0xCD, 0xEF, 0x12, 0x34];
        let ssrc = username_to_ssrc("alice");
        let packet = VoicePacket::new(42, 1000, ssrc, opus_frame.clone());

        let encoded = packet.encode().expect("encode failed");
        let (decoded, bytes_read) = VoicePacket::decode(&encoded).expect("decode failed");

        assert_eq!(decoded.sequence, 42);
        assert_eq!(decoded.timestamp, 1000);
        assert_eq!(decoded.ssrc, ssrc);
        assert_eq!(decoded.opus_frame, opus_frame);
        assert_eq!(bytes_read, encoded.len());
    }

    #[test]
    fn test_encode_decode_voice_packet_empty_opus() {
        let ssrc = username_to_ssrc("bob");
        let packet = VoicePacket::new(1, 2, ssrc, vec![]);

        let encoded = packet.encode().expect("encode failed");
        let (decoded, bytes_read) = VoicePacket::decode(&encoded).expect("decode failed");

        assert_eq!(decoded.sequence, 1);
        assert_eq!(decoded.timestamp, 2);
        assert_eq!(decoded.ssrc, ssrc);
        assert_eq!(decoded.opus_frame.len(), 0);
        assert_eq!(bytes_read, encoded.len());
    }

    #[test]
    fn test_encode_decode_voice_packet_large_opus() {
        let opus_frame = vec![0x42; 1200]; // 1200 bytes of data
        let ssrc = username_to_ssrc("charlie");
        let packet = VoicePacket::new(999, 88888, ssrc, opus_frame.clone());

        let encoded = packet.encode().expect("encode failed");
        let (decoded, bytes_read) = VoicePacket::decode(&encoded).expect("decode failed");

        assert_eq!(decoded.sequence, 999);
        assert_eq!(decoded.timestamp, 88888);
        assert_eq!(decoded.ssrc, ssrc);
        assert_eq!(decoded.opus_frame, opus_frame);
        assert_eq!(bytes_read, encoded.len());
    }

    #[test]
    fn test_decode_voice_packet_too_short() {
        let data = vec![PROTOCOL_VERSION, 0, 0, 0];
        let result = VoicePacket::decode(&data);
        assert!(result.is_err());
    }

    #[test]
    fn test_decode_voice_packet_bad_version() {
        let mut data = vec![PROTOCOL_VERSION + 1];
        data.extend_from_slice(&0u32.to_be_bytes()); // sequence
        data.extend_from_slice(&0u32.to_be_bytes()); // timestamp
        data.extend_from_slice(&0u32.to_be_bytes()); // ssrc
        let result = VoicePacket::decode(&data);
        assert!(result.is_err());
    }

    #[test]
    fn test_tcp_packet_format_example() {
        // Demonstrate the binary format with a known example
        let packet = TcpPacket::new(PacketTypeId::Login, b"user".to_vec());
        let encoded = packet.encode().expect("encode failed");

        // Format: [version(1)][packet_type(1)][payload_len(2)][payload(4)]
        // = 1 + 1 + 2 + 4 = 8 bytes
        assert_eq!(encoded.len(), 8);

        // Check header bytes
        assert_eq!(encoded[0], PROTOCOL_VERSION); // version
        assert_eq!(encoded[1], 0x01); // packet_type (Login)
        assert_eq!(&encoded[2..4], &4u16.to_be_bytes()); // payload_len
        assert_eq!(&encoded[4..], b"user"); // payload
    }

    #[test]
    fn test_voice_packet_format_example() {
        // Demonstrate the binary format with a known example
        let opus = b"frame".to_vec();
        let ssrc = 0xAABBCCDD;
        let packet = VoicePacket::new(0x11223344, 0x55667788, ssrc, opus);
        let encoded = packet.encode().expect("encode failed");

        // Format: [version(1)][sequence(4)][timestamp(4)][ssrc(4)][opus_frame(5)]
        // = 1 + 4 + 4 + 4 + 5 = 18 bytes
        assert_eq!(encoded.len(), 18);

        // Check header bytes
        assert_eq!(encoded[0], PROTOCOL_VERSION); // version
        assert_eq!(&encoded[1..5], &0x11223344u32.to_be_bytes()); // sequence
        assert_eq!(&encoded[5..9], &0x55667788u32.to_be_bytes()); // timestamp
        assert_eq!(&encoded[9..13], &0xAABBCCDDu32.to_be_bytes()); // ssrc
        assert_eq!(&encoded[13..], b"frame"); // opus_frame
    }

    #[test]
    fn test_encode_decode_username_with_udp_port() {
        let username = "alice";
        let port = 12345u16;

        let encoded = encode_username_with_udp_port(username, port).expect("encode failed");
        let (decoded_username, decoded_port) = decode_username_with_udp_port(&encoded).expect("decode failed");

        assert_eq!(decoded_username, username);
        assert_eq!(decoded_port, port);
    }

    #[test]
    fn test_encode_decode_username_with_udp_port_various() {
        // Test with various port numbers
        let test_cases = vec![
            ("alice", 0u16),
            ("bob", 65535u16),
            ("charlie", 9002u16),
            ("diana", 54321u16),
        ];

        for (username, port) in test_cases {
            let encoded = encode_username_with_udp_port(username, port).expect("encode failed");
            let (decoded_username, decoded_port) = decode_username_with_udp_port(&encoded).expect("decode failed");

            assert_eq!(decoded_username, username);
            assert_eq!(decoded_port, port);
        }
    }

    #[test]
    fn test_decode_username_with_udp_port_too_short() {
        let data = vec![0x61, 0x62, 0x63]; // "abc" with null term but missing port
        let result = decode_username_with_udp_port(&data);
        assert!(result.is_err());
    }

    #[test]
    fn test_decode_username_with_udp_port_not_null_terminated() {
        let mut data = vec![0x61, 0x62, 0x63]; // "abc" without null terminator
        data.extend_from_slice(&12345u16.to_be_bytes());
        let result = decode_username_with_udp_port(&data);
        assert!(result.is_err());
    }

    #[test]
    fn test_encode_decode_udp_auth_packet() {
        let token = 0x0123456789ABCDEFu64;
        let username = "alice";
        let packet = UdpAuthPacket::new(token, username.to_string());

        let encoded = packet.encode().expect("encode failed");
        let (decoded, bytes_read) = UdpAuthPacket::decode(&encoded).expect("decode failed");

        assert_eq!(decoded.token, token);
        assert_eq!(decoded.username, username);
        assert_eq!(bytes_read, encoded.len());
    }

    #[test]
    fn test_encode_decode_udp_auth_packet_various() {
        let test_cases = vec![
            (0u64, "alice"),
            (u64::MAX, "bob"),
            (12345u64, "charlie"),
            (0x0123456789ABCDEFu64, "diana"),
        ];

        for (token, username) in test_cases {
            let packet = UdpAuthPacket::new(token, username.to_string());
            let encoded = packet.encode().expect("encode failed");
            let (decoded, _) = UdpAuthPacket::decode(&encoded).expect("decode failed");

            assert_eq!(decoded.token, token);
            assert_eq!(decoded.username, username);
        }
    }

    #[test]
    fn test_decode_udp_auth_packet_too_short() {
        let data = vec![PROTOCOL_VERSION, 0, 0, 0, 0];
        let result = UdpAuthPacket::decode(&data);
        assert!(result.is_err());
    }

    #[test]
    fn test_decode_udp_auth_packet_not_null_terminated() {
        let mut data = vec![PROTOCOL_VERSION];
        data.extend_from_slice(&0u64.to_be_bytes());
        data.extend_from_slice(b"alice"); // no null terminator
        let result = UdpAuthPacket::decode(&data);
        assert!(result.is_err());
    }

    #[test]
    fn test_decode_udp_auth_packet_bad_version() {
        let mut data = vec![PROTOCOL_VERSION + 1];
        data.extend_from_slice(&0u64.to_be_bytes());
        data.extend_from_slice(b"alice\0");
        let result = UdpAuthPacket::decode(&data);
        assert!(result.is_err());
    }

    #[test]
    fn test_encode_decode_udp_auth_response_success() {
        let response = UdpAuthResponse::new(true);
        let encoded = response.encode().expect("encode failed");
        let decoded = UdpAuthResponse::decode(&encoded).expect("decode failed");
        assert!(decoded.success);
    }

    #[test]
    fn test_encode_decode_udp_auth_response_failure() {
        let response = UdpAuthResponse::new(false);
        let encoded = response.encode().expect("encode failed");
        let decoded = UdpAuthResponse::decode(&encoded).expect("decode failed");
        assert!(!decoded.success);
    }

    #[test]
    fn test_decode_udp_auth_response_too_short() {
        let data = vec![PROTOCOL_VERSION];
        let result = UdpAuthResponse::decode(&data);
        assert!(result.is_err());
    }

    #[test]
    fn test_decode_udp_auth_response_bad_version() {
        let data = vec![PROTOCOL_VERSION + 1, 1];
        let result = UdpAuthResponse::decode(&data);
        assert!(result.is_err());
    }

    #[test]
    fn test_encode_decode_voice_token() {
        let token = 0x0123456789ABCDEFu64;
        let encoded = encode_voice_token(token).expect("encode failed");
        let decoded = decode_voice_token(&encoded).expect("decode failed");
        assert_eq!(decoded, token);
    }

    #[test]
    fn test_encode_decode_voice_token_various() {
        let test_cases = vec![0u64, u64::MAX, 12345u64, 0x0123456789ABCDEFu64];

        for token in test_cases {
            let encoded = encode_voice_token(token).expect("encode failed");
            let decoded = decode_voice_token(&encoded).expect("decode failed");
            assert_eq!(decoded, token);
        }
    }

    #[test]
    fn test_decode_voice_token_too_short() {
        let data = vec![0x01, 0x02, 0x03];
        let result = decode_voice_token(&data);
        assert!(result.is_err());
    }

    #[test]
    fn test_decode_voice_token_empty() {
        let data = vec![];
        let result = decode_voice_token(&data);
        assert!(result.is_err());
    }
}