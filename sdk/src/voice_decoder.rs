use opus::Decoder;
use tracing::debug;

const SAMPLE_RATE: u32 = 48000;
const OPUS_FRAME_SAMPLES: usize = 960; // 20ms at 48kHz

/// Manages Opus decoding of voice packets
pub struct VoiceDecoder {
    decoder: Decoder,
}

// SAFETY: VoiceDecoder can be safely sent between threads because:
// 1. The decoder is only accessed through &mut self (not shared)
// 2. Access is synchronized via RwLock in UserVoiceStreamManager
// 3. The underlying opus library manages its state correctly
unsafe impl Send for VoiceDecoder {}

impl VoiceDecoder {
    /// Create a new Opus decoder
    pub fn new() -> Result<Self, opus::Error> {
        let decoder = Decoder::new(SAMPLE_RATE, opus::Channels::Mono)?;
        Ok(VoiceDecoder { decoder })
    }

    /// Decode a single Opus frame to PCM samples
    pub fn decode_frame(&mut self, opus_frame: &[u8]) -> Result<Vec<f32>, String> {
        // Allocate output buffer for max frame size
        let mut pcm_out = vec![0.0f32; OPUS_FRAME_SAMPLES];

        let samples_decoded = self.decoder
            .decode_float(opus_frame, &mut pcm_out, false)
            .map_err(|e| format!("Failed to decode Opus frame: {:?}", e))?;

        pcm_out.truncate(samples_decoded);

        debug!("Decoded {} bytes to {} samples", opus_frame.len(), samples_decoded);

        Ok(pcm_out)
    }
}

/// Convert mono audio to stereo by duplicating channels
pub fn mono_to_stereo(mono: &[f32]) -> Vec<f32> {
    let mut stereo = Vec::with_capacity(mono.len() * 2);
    for &sample in mono {
        stereo.push(sample);
        stereo.push(sample); // Duplicate to stereo
    }
    stereo
}
