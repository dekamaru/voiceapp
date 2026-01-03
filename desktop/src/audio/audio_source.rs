use std::sync::Arc;
use arc_swap::ArcSwap;
use voiceapp_sdk::Decoder;
use crate::audio::adjust_volume;
use crate::config::AppConfig;

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

/// Wrapper that applies dynamic per-user volume adjustment
pub struct VolumeAdjustedSource {
    inner: Arc<dyn AudioSource>,
    app_config: Arc<ArcSwap<AppConfig>>,
    user_id: u64,
}

impl VolumeAdjustedSource {
    pub fn new(inner: Arc<dyn AudioSource>, app_config: Arc<ArcSwap<AppConfig>>, user_id: u64) -> Self {
        Self {
            inner,
            app_config,
            user_id,
        }
    }
}

impl AudioSource for VolumeAdjustedSource {
    fn get_audio(&self) -> Result<Vec<f32>, Box<dyn std::error::Error>> {
        let mut samples = self.inner.get_audio()?;
        let config = self.app_config.load();
        let master_volume = config.audio.output_device.volume as f32 / 100.0;
        let user_volume = config.audio.users_volumes
            .get(&self.user_id)
            .copied()
            .unwrap_or(100) as f32 / 100.0;

        adjust_volume(&mut samples, master_volume * user_volume);

        Ok(samples)
    }
}
