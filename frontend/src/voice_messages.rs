#[derive(Debug, Clone)]
pub enum VoiceCommand {
    Connect {
        management_addr: String,
        voice_addr: String,
        username: String,
    },
    JoinVoiceChannel,
    LeaveVoiceChannel,
}

#[derive(Debug, Clone)]
pub enum VoiceCommandResult {
    Connect(Result<(), String>),
    JoinVoiceChannel(Result<(), String>),
    LeaveVoiceChannel(Result<(), String>),
}