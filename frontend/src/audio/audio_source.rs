use std::sync::Arc;
use voiceapp_sdk::Decoder;

/// Trait for audio sources that can provide audio samples
/// This abstraction allows both VoiceDecoder and NotificationPlayer
/// to work with the same output stream infrastructure
pub trait AudioSource: Send + Sync {
    /// Get next chunk of audio samples (mono f32)
    /// Returns empty vec or error if no audio available
    fn get_audio(&self) -> Result<Vec<f32>, Box<dyn std::error::Error>>;
}

/// Wrapper to make VoiceDecoder implement AudioSource trait
pub struct VoiceDecoderSource {
    decoder: Arc<Decoder>,
}

impl VoiceDecoderSource {
    pub fn new(decoder: Arc<Decoder>) -> Self {
        Self { decoder }
    }
}

impl AudioSource for VoiceDecoderSource {
    fn get_audio(&self) -> Result<Vec<f32>, Box<dyn std::error::Error>> {
        self.decoder.get_decoded_audio().map_err(|e| e.into())
    }
}
