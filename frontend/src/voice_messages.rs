#[derive(Debug, Clone)]
pub enum VoiceCommand {
    Connect {
        management_addr: String,
        voice_addr: String,
        username: String,
    },
}

#[derive(Debug, Clone)]
pub enum VoiceCommandResult {
    Connect(Result<(), String>),
}