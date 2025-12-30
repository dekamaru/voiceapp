use thiserror::Error;

/// SDK error type
#[derive(Debug, Clone, Error)]
pub enum SdkError {
    #[error("connection failed: {0}")]
    ConnectionFailed(String),
    #[error("disconnected from server")]
    Disconnected,
    #[error("timeout: {0}")]
    Timeout(String),
    #[error("encoder error: {0}")]
    EncoderError(String),
    #[error("decoder error: {0}")]
    DecoderError(String),
    #[error("resampler error: {0}")]
    ResamplerError(String),
    #[error("lock error")]
    LockError,
    #[error("channel closed")]
    ChannelClosed,
    #[error("invalid input: {0}")]
    InvalidInput(String),
}