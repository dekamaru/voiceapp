/// Errors that can occur with VoiceClient
#[derive(Debug, Clone)]
pub enum ClientError {
    ConnectionFailed(String),
    Disconnected,
    Timeout(String),
    SystemError(String),
    VoiceInputOutputManagerError(String),
}

impl std::fmt::Display for ClientError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ClientError::ConnectionFailed(msg) => write!(f, "Connection failed: {}", msg),
            ClientError::Disconnected => write!(f, "Disconnected from server"),
            ClientError::Timeout(msg) => write!(f, "Timeout exceeded {}", msg),
            ClientError::SystemError(msg) => write!(f, "System error: {}", msg),
            ClientError::VoiceInputOutputManagerError(msg) => write!(f, "Voice I/O manager error: {}", msg),
        }
    }
}

impl std::error::Error for ClientError {}