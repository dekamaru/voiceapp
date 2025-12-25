use opus::{Bitrate, Channels, Encoder};

/// Voice data packet structure for encoder
pub struct VoiceData {
    pub sequence: u32,
    pub timestamp: u32,
    pub ssrc: u64,
    pub opus_frame: Vec<u8>,
}

pub const SAMPLE_RATE: u32 = 48000;
pub const OPUS_FRAME_SAMPLES: usize = 960; // 20ms at 48kHz

// TODO: variable to play with!
const ENCODING_BITRATE: i32 = 96000; // 96 kbps

/// Manages both Opus encoding and decoding of voice packets
pub struct VoiceEncoder {
    encoder: Encoder,
    pub sequence: u32,
    pub timestamp: u32,
}

impl VoiceEncoder {
    /// Create a new voice codec
    pub fn new() -> Result<Self, String> {
        let mut encoder = Encoder::new(SAMPLE_RATE, Channels::Mono, opus::Application::Voip)
            .map_err(|e| format!("opus error: {}", e.to_string()))?;

        encoder
            .set_bitrate(Bitrate::Bits(ENCODING_BITRATE))
            .unwrap();

        Ok(VoiceEncoder {
            encoder,
            sequence: 0,
            timestamp: 0,
        })
    }

    /// Encode audio samples to Opus format
    /// Pads with zeros if needed to reach OPUS_FRAME_SAMPLES
    pub fn encode(&mut self, samples: &[f32]) -> Result<Option<VoiceData>, String> {
        if samples.is_empty() {
            return Ok(None);
        }

        assert!(
            samples.len() <= OPUS_FRAME_SAMPLES,
            "Samples must be <= {} samples",
            OPUS_FRAME_SAMPLES
        );

        // Pad with zeros to reach frame size
        let mut padded_samples = samples.to_vec();
        while padded_samples.len() < OPUS_FRAME_SAMPLES {
            padded_samples.push(0.0);
        }

        let mut opus_frame = vec![0u8; 4000]; // Max Opus frame size
        let encoded_size = self
            .encoder
            .encode_float(&padded_samples, &mut opus_frame)
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
