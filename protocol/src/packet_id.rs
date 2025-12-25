use crate::error::ProtocolError;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum PacketId {
    // Requests (0x01-0x20)
    LoginRequest = 0x01,
    JoinVoiceChannelRequest = 0x02,
    VoiceAuthRequest = 0x03,
    LeaveVoiceChannelRequest = 0x04,
    ChatMessageRequest = 0x05,

    // Responses (0x21-0x40)
    LoginResponse = 0x21,
    VoiceAuthResponse = 0x22,
    JoinVoiceChannelResponse = 0x23,
    LeaveVoiceChannelResponse = 0x24,
    ChatMessageResponse = 0x25,

    // Events (0x41-0x60)
    UserJoinedServer = 0x41,
    UserJoinedVoice = 0x42,
    UserLeftVoice = 0x43,
    UserLeftServer = 0x44,
    UserSentMessage = 0x45,
    UserMuteState = 0x46,

    // UDP (0x61-...)
    VoiceData = 0x61,
}

impl PacketId {
    pub(crate) fn as_u8(self) -> u8 {
        self as u8
    }
}

impl TryFrom<u8> for PacketId {
    type Error = ProtocolError;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0x01 => Ok(PacketId::LoginRequest),
            0x02 => Ok(PacketId::JoinVoiceChannelRequest),
            0x03 => Ok(PacketId::VoiceAuthRequest),
            0x04 => Ok(PacketId::LeaveVoiceChannelRequest),
            0x05 => Ok(PacketId::ChatMessageRequest),
            0x21 => Ok(PacketId::LoginResponse),
            0x22 => Ok(PacketId::VoiceAuthResponse),
            0x23 => Ok(PacketId::JoinVoiceChannelResponse),
            0x24 => Ok(PacketId::LeaveVoiceChannelResponse),
            0x25 => Ok(PacketId::ChatMessageResponse),
            0x41 => Ok(PacketId::UserJoinedServer),
            0x42 => Ok(PacketId::UserJoinedVoice),
            0x43 => Ok(PacketId::UserLeftVoice),
            0x44 => Ok(PacketId::UserLeftServer),
            0x45 => Ok(PacketId::UserSentMessage),
            0x46 => Ok(PacketId::UserMuteState),
            0x61 => Ok(PacketId::VoiceData),
            _ => Err(ProtocolError::UnknownPacketId(value)),
        }
    }
}