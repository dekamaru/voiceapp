//! VoiceApp common protocol definitions
//!
//! This module defines the binary protocol used for communication between
//! client and server over TCP (control) and UDP (voice).

use std::io::{self, Write};

// Protocol version
pub const PROTOCOL_VERSION: u8 = 1;

// Packet type IDs for TCP control messages
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PacketTypeId {
    Login = 0x01,
    UserJoinedServer = 0x02,
    JoinVoiceChannel = 0x03,
    UserJoinedVoice = 0x04,
    UserLeftVoice = 0x05,
    UserLeftServer = 0x06,
    ServerParticipantList = 0x07,
}

impl PacketTypeId {
    pub fn from_u8(value: u8) -> Option<Self> {
        match value {
            0x01 => Some(PacketTypeId::Login),
            0x02 => Some(PacketTypeId::UserJoinedServer),
            0x03 => Some(PacketTypeId::JoinVoiceChannel),
            0x04 => Some(PacketTypeId::UserJoinedVoice),
            0x05 => Some(PacketTypeId::UserLeftVoice),
            0x06 => Some(PacketTypeId::UserLeftServer),
            0x07 => Some(PacketTypeId::ServerParticipantList),
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

/// UDP voice packet structure
/// Format: [version: u8][sequence: u32][timestamp: u32][ssrc: u32][opus_frame...]
#[derive(Debug, Clone)]
pub struct VoicePacket {
    pub sequence: u32,
    pub timestamp: u32,
    pub ssrc: u32, // Synchronization source (identifies the sender)
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
        pos = data.len();

        Ok((
            VoicePacket {
                sequence,
                timestamp,
                ssrc,
                opus_frame,
            },
            pos,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_packet_type_id_conversions() {
        assert_eq!(PacketTypeId::Login.as_u8(), 0x01);
        assert_eq!(PacketTypeId::UserJoinedServer.as_u8(), 0x02);
        assert_eq!(PacketTypeId::JoinVoiceChannel.as_u8(), 0x03);
        assert_eq!(PacketTypeId::UserJoinedVoice.as_u8(), 0x04);
        assert_eq!(PacketTypeId::UserLeftVoice.as_u8(), 0x05);
        assert_eq!(PacketTypeId::UserLeftServer.as_u8(), 0x06);
        assert_eq!(PacketTypeId::ServerParticipantList.as_u8(), 0x07);

        assert_eq!(PacketTypeId::from_u8(0x01), Some(PacketTypeId::Login));
        assert_eq!(PacketTypeId::from_u8(0x05), Some(PacketTypeId::UserLeftVoice));
        assert_eq!(PacketTypeId::from_u8(0x06), Some(PacketTypeId::UserLeftServer));
        assert_eq!(PacketTypeId::from_u8(0x07), Some(PacketTypeId::ServerParticipantList));
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
        let packet = VoicePacket::new(42, 1000, 54321, opus_frame.clone());

        let encoded = packet.encode().expect("encode failed");
        let (decoded, bytes_read) = VoicePacket::decode(&encoded).expect("decode failed");

        assert_eq!(decoded.sequence, 42);
        assert_eq!(decoded.timestamp, 1000);
        assert_eq!(decoded.ssrc, 54321);
        assert_eq!(decoded.opus_frame, opus_frame);
        assert_eq!(bytes_read, encoded.len());
    }

    #[test]
    fn test_encode_decode_voice_packet_empty_opus() {
        let packet = VoicePacket::new(1, 2, 3, vec![]);

        let encoded = packet.encode().expect("encode failed");
        let (decoded, bytes_read) = VoicePacket::decode(&encoded).expect("decode failed");

        assert_eq!(decoded.sequence, 1);
        assert_eq!(decoded.timestamp, 2);
        assert_eq!(decoded.ssrc, 3);
        assert_eq!(decoded.opus_frame.len(), 0);
        assert_eq!(bytes_read, encoded.len());
    }

    #[test]
    fn test_encode_decode_voice_packet_large_opus() {
        let opus_frame = vec![0x42; 1200]; // 1200 bytes of data
        let packet = VoicePacket::new(999, 88888, 77777, opus_frame.clone());

        let encoded = packet.encode().expect("encode failed");
        let (decoded, bytes_read) = VoicePacket::decode(&encoded).expect("decode failed");

        assert_eq!(decoded.sequence, 999);
        assert_eq!(decoded.timestamp, 88888);
        assert_eq!(decoded.ssrc, 77777);
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
        let packet = VoicePacket::new(0x11223344, 0x55667788, 0x99AABBCC, opus);
        let encoded = packet.encode().expect("encode failed");

        // Format: [version(1)][sequence(4)][timestamp(4)][ssrc(4)][opus_frame(5)]
        // = 1 + 4 + 4 + 4 + 5 = 18 bytes
        assert_eq!(encoded.len(), 18);

        // Check header bytes
        assert_eq!(encoded[0], PROTOCOL_VERSION); // version
        assert_eq!(&encoded[1..5], &0x11223344u32.to_be_bytes()); // sequence
        assert_eq!(&encoded[5..9], &0x55667788u32.to_be_bytes()); // timestamp
        assert_eq!(&encoded[9..13], &0x99AABBCCu32.to_be_bytes()); // ssrc
        assert_eq!(&encoded[13..], b"frame"); // opus_frame
    }
}