pub mod events;
pub mod packet_id;
pub mod requests;
pub mod responses;

pub use events::ParticipantInfo;
pub use packet_id::{parse_packet, PacketId};
pub use requests::VoiceData;
pub use responses::LoginResponse;

// Re-export encode/decode functions for convenience
pub use events::{
    decode_user_joined_server, decode_user_joined_voice, decode_user_left_server,
    decode_user_left_voice, decode_user_sent_message, encode_user_joined_server,
    encode_user_joined_voice, encode_user_left_server, encode_user_left_voice,
    encode_user_sent_message,
};
pub use requests::{
    decode_chat_message_request, decode_login_request, decode_voice_auth_request,
    decode_voice_data, encode_chat_message_request, encode_join_voice_channel_request,
    encode_leave_voice_channel_request, encode_login_request, encode_voice_auth_request,
    encode_voice_data,
};
pub use responses::{
    decode_chat_message_response, decode_join_voice_channel_response,
    decode_leave_voice_channel_response, decode_login_response, decode_voice_auth_response,
    encode_chat_message_response, encode_join_voice_channel_response,
    encode_leave_voice_channel_response, encode_login_response, encode_voice_auth_response,
};
