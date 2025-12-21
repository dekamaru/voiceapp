/// Errors that can occur with VoiceClient
#[derive(Debug, Clone)]
pub enum VoiceClientError {
    ConnectionFailed(String),
    Disconnected,
    Timeout(String),
    SystemError(String),
    VoiceInputOutputManagerError(String),
}

impl std::fmt::Display for VoiceClientError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            VoiceClientError::ConnectionFailed(msg) => write!(f, "Connection failed: {}", msg),
            VoiceClientError::Disconnected => write!(f, "Disconnected from server"),
            VoiceClientError::Timeout(msg) => write!(f, "Timeout exceeded {}", msg),
            VoiceClientError::SystemError(msg) => write!(f, "System error: {}", msg),
            VoiceClientError::VoiceInputOutputManagerError(msg) => write!(f, "Voice I/O manager error: {}", msg),
        }
    }
}

impl std::error::Error for VoiceClientError {}