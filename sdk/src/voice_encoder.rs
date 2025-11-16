use opus::{Channels, Encoder};
use voiceapp_protocol::VoiceData;

pub const SAMPLE_RATE: u32 = 48000;
pub const OPUS_FRAME_SAMPLES: usize = 960; // 20ms at 48kHz

/// Manages Opus encoding of audio frames
pub struct VoiceEncoder {
    encoder: Encoder,
    pub sequence: u32,
    pub timestamp: u32,
}

impl VoiceEncoder {
    /// Create a new voice encoder
    pub fn new() -> Result<Self, opus::Error> {
        let mut encoder = Encoder::new(SAMPLE_RATE, Channels::Mono, opus::Application::Voip)?;

        // Set encoding parameters
        // TODO: check later on bitrate
        encoder.set_bitrate(opus::Bitrate::Max)?;

        Ok(VoiceEncoder {
            encoder,
            sequence: 0,
            timestamp: 0,
        })
    }

    /// Encode audio samples (up to OPUS_FRAME_SAMPLES, pads with zeros if needed)
    pub fn encode(&mut self, samples: &[f32]) -> Result<Option<VoiceData>, String> {
        if samples.is_empty() {
            return Ok(None);
        }

        assert!(samples.len() <= OPUS_FRAME_SAMPLES, "Samples must be <= {} samples", OPUS_FRAME_SAMPLES);

        // Pad with zeros to reach frame size
        let mut padded_samples = samples.to_vec();
        while padded_samples.len() < OPUS_FRAME_SAMPLES {
            padded_samples.push(0.0);
        }

        let mut opus_frame = vec![0u8; 4000]; // Max Opus frame size
        let encoded_size = self.encoder.encode_float(&padded_samples, &mut opus_frame)
            .map_err(|e| format!("Failed to encode Opus frame: {:?}", e))?;
        opus_frame.truncate(encoded_size);

        let packet = VoiceData {
            sequence: self.sequence,
            timestamp: self.timestamp,
            ssrc: 0,
            opus_frame,
        };

        self.sequence = self.sequence.wrapping_add(1);
        self.timestamp = self.timestamp.wrapping_add(OPUS_FRAME_SAMPLES as u32);

        Ok(Some(packet))
    }
}
