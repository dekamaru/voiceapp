use crate::error::ProtocolError;

macro_rules! packet_ids {
    ($($name:ident = $val:expr),* $(,)?) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
        pub(crate) enum PacketId { $($name = $val,)* }

        impl PacketId {
            pub(crate) const fn as_u8(self) -> u8 { self as u8 }
        }

        impl TryFrom<u8> for PacketId {
            type Error = ProtocolError;

            fn try_from(value: u8) -> Result<Self, Self::Error> {
                match value {
                    $($val => Ok(Self::$name),)*
                    _ => Err(ProtocolError::UnknownPacketId(value)),
                }
            }
        }
    };
}

packet_ids! {
    // Requests (0x01-0x1F)
    LoginRequest = 0x01,
    JoinVoiceChannelRequest = 0x02,
    VoiceAuthRequest = 0x03,
    LeaveVoiceChannelRequest = 0x04,
    ChatMessageRequest = 0x05,
    PingRequest = 0x06,

    // Responses (0x20-0x3F)
    LoginResponse = 0x21,
    VoiceAuthResponse = 0x22,
    JoinVoiceChannelResponse = 0x23,
    LeaveVoiceChannelResponse = 0x24,
    ChatMessageResponse = 0x25,
    PingResponse = 0x26,

    // Events (0x40-0x5F)
    UserJoinedServer = 0x41,
    UserJoinedVoice = 0x42,
    UserLeftVoice = 0x43,
    UserLeftServer = 0x44,
    UserSentMessage = 0x45,
    UserMuteState = 0x46,

    // UDP (0x60+)
    VoiceData = 0x61,
}
