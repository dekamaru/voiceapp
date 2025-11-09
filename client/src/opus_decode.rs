use opus::Decoder;
use tracing::debug;

const SAMPLE_RATE: u32 = 48000;
const OPUS_FRAME_SAMPLES: usize = 960; // 20ms at 48kHz

/// Manages Opus decoding of voice packets
pub struct OpusDecoder {
    decoder: Decoder,
}

// SAFETY: OpusDecoder can be safely sent between threads because:
// 1. The decoder is only accessed through &mut self (not shared)
// 2. Access is synchronized via RwLock in UserVoiceStreamManager
// 3. The underlying opus library manages its state correctly
unsafe impl Send for OpusDecoder {}

impl OpusDecoder {
    /// Create a new Opus decoder
    pub fn new() -> Result<Self, opus::Error> {
        let decoder = Decoder::new(SAMPLE_RATE, opus::Channels::Mono)?;
        Ok(OpusDecoder { decoder })
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

    /// Decode a frame with packet loss concealment for missing packets
    pub fn decode_frame_with_plc(&mut self) -> Result<Vec<f32>, String> {
        let mut pcm_out = vec![0.0f32; OPUS_FRAME_SAMPLES];

        let samples_decoded = self.decoder
            .decode_float(&[], &mut pcm_out, true)
            .map_err(|e| format!("Failed to decode with PLC: {:?}", e))?;

        pcm_out.truncate(samples_decoded);

        debug!("Decoded with PLC: {} samples", samples_decoded);

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_opus_decoder_creation() {
        let result = OpusDecoder::new();
        assert!(result.is_ok());
    }

    #[test]
    fn test_mono_to_stereo_conversion() {
        let mono = vec![0.1, 0.2, 0.3, 0.4];
        let stereo = mono_to_stereo(&mono);

        assert_eq!(stereo.len(), 8);
        assert_eq!(stereo[0], 0.1);
        assert_eq!(stereo[1], 0.1); // Duplicated
        assert_eq!(stereo[2], 0.2);
        assert_eq!(stereo[3], 0.2); // Duplicated
        assert_eq!(stereo[4], 0.3);
        assert_eq!(stereo[5], 0.3); // Duplicated
        assert_eq!(stereo[6], 0.4);
        assert_eq!(stereo[7], 0.4); // Duplicated
    }

    #[test]
    fn test_mono_to_stereo_empty() {
        let mono: Vec<f32> = vec![];
        let stereo = mono_to_stereo(&mono);
        assert_eq!(stereo.len(), 0);
    }

    #[test]
    fn test_mono_to_stereo_single_sample() {
        let mono = vec![0.5];
        let stereo = mono_to_stereo(&mono);
        assert_eq!(stereo.len(), 2);
        assert_eq!(stereo[0], 0.5);
        assert_eq!(stereo[1], 0.5);
    }
}
