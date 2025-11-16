use opus::{Channels, Encoder, Decoder};
use voiceapp_protocol::VoiceData;
use tracing::debug;

pub const SAMPLE_RATE: u32 = 48000;
pub const OPUS_FRAME_SAMPLES: usize = 960; // 20ms at 48kHz

/// Manages both Opus encoding and decoding of voice packets
pub struct VoiceCodec {
    encoder: Encoder,
    decoder: Decoder,
    pub sequence: u32,
    pub timestamp: u32,
}

impl VoiceCodec {
    /// Create a new voice codec
    pub fn new() -> Result<Self, opus::Error> {
        let encoder = Encoder::new(SAMPLE_RATE, Channels::Mono, opus::Application::Voip)?;
        let decoder = Decoder::new(SAMPLE_RATE, Channels::Mono)?;

        // Set encoding parameters
        let mut codec = VoiceCodec {
            encoder,
            decoder,
            sequence: 0,
            timestamp: 0,
        };

        // Configure encoder
        codec.encoder.set_bitrate(opus::Bitrate::Max)?;

        Ok(codec)
    }

    /// Encode audio samples to Opus format
    /// Pads with zeros if needed to reach OPUS_FRAME_SAMPLES
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

    /// Decode Opus frame to PCM samples
    pub fn decode(&mut self, opus_frame: &[u8]) -> Result<Vec<f32>, String> {
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
