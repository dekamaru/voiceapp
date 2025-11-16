use opus::{Channels, Encoder};
use tracing::debug;
use voiceapp_protocol::VoiceData;

pub const SAMPLE_RATE: u32 = 48000;
pub const OPUS_FRAME_SAMPLES: usize = 960; // 20ms at 48kHz

/// Manages Opus encoding of audio frames
pub struct VoiceEncoder {
    encoder: Encoder,
    pub sequence: u32,
    pub timestamp: u32,
    sample_buffer: Vec<f32>,
}

impl VoiceEncoder {
    /// Create a new voice encoder for the given user ID
    pub fn new() -> Result<Self, opus::Error> {
        let mut encoder = Encoder::new(SAMPLE_RATE, Channels::Mono, opus::Application::Voip)?;

        // Set encoding parameters
        encoder.set_bitrate(opus::Bitrate::Max)?;

        Ok(VoiceEncoder {
            encoder,
            sequence: 0,
            timestamp: 0,
            sample_buffer: Vec::with_capacity(OPUS_FRAME_SAMPLES * 2),
        })
    }

    /// Add audio samples and return any complete Opus frames
    pub fn encode_frame(&mut self, samples: &[f32]) -> Result<Vec<VoiceData>, String> {
        self.sample_buffer.extend_from_slice(samples);

        let mut packets = Vec::new();

        // Encode all complete frames
        while self.sample_buffer.len() >= OPUS_FRAME_SAMPLES {
            let frame_samples: Vec<f32> = self.sample_buffer.drain(0..OPUS_FRAME_SAMPLES).collect();

            let mut opus_frame = vec![0u8; 4000]; // Max Opus frame size
            let encoded_size = self.encoder.encode_float(&frame_samples, &mut opus_frame)
                .map_err(|e| format!("Failed to encode Opus frame: {:?}", e))?;
            opus_frame.truncate(encoded_size);

            let packet = VoiceData {
                sequence: self.sequence,
                timestamp: self.timestamp,
                ssrc: 0,
                opus_frame,
            };
            packets.push(packet);

            self.sequence = self.sequence.wrapping_add(1);
            self.timestamp = self.timestamp.wrapping_add(OPUS_FRAME_SAMPLES as u32);
        }

        Ok(packets)
    }

    /// Flush any remaining samples in the buffer
    pub fn flush(&mut self) -> Result<Option<VoiceData>, String> {
        if self.sample_buffer.is_empty() {
            return Ok(None);
        }

        // Pad with zeros to reach frame size
        while self.sample_buffer.len() < OPUS_FRAME_SAMPLES {
            self.sample_buffer.push(0.0);
        }

        let mut opus_frame = vec![0u8; 4000];
        let encoded_size = self.encoder.encode_float(&self.sample_buffer, &mut opus_frame)
            .map_err(|e| format!("Failed to encode Opus frame: {:?}", e))?;
        opus_frame.truncate(encoded_size);

        self.sample_buffer.clear();

        debug!("Flushed {} bytes from remaining samples", encoded_size);

        let packet = VoiceData {
            sequence: self.sequence,
            timestamp: self.timestamp,
            ssrc: 0,
            opus_frame,
        };

        Ok(Some(packet))
    }
}
