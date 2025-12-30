use opus::{Bitrate, Channels};
use crate::error::SdkError;
use crate::voice::opus_consts::{OPUS_ENCODING_BITRATE, OPUS_FRAME_SIZE, OPUS_SAMPLE_RATE};
use crate::voice::models::VoiceData;

/// Manages Opus encoding of voice packets
pub(crate) struct Encoder {
    encoder: opus::Encoder,
    sequence: u32,
    timestamp: u32,
}

impl Encoder {
    pub fn new() -> Result<Self, SdkError> {
        let mut encoder = opus::Encoder::new(OPUS_SAMPLE_RATE, Channels::Mono, opus::Application::Voip)
            .map_err(|e| SdkError::EncoderError(format!("opus error: {}", e)))?;

        encoder
            .set_bitrate(Bitrate::Bits(OPUS_ENCODING_BITRATE))
            .map_err(|e| SdkError::EncoderError(e.to_string()))?;

        Ok(Encoder {
            encoder,
            sequence: 0,
            timestamp: 0,
        })
    }

    /// Encode audio samples to Opus format
    pub fn encode(&mut self, samples: &[f32]) -> Result<VoiceData, SdkError> {
        if samples.len() > OPUS_FRAME_SIZE as usize {
            return Err(SdkError::InvalidInput(
                format!("samples ({}) exceed frame size ({})", samples.len(), OPUS_FRAME_SIZE)
            ));
        }

        // Pad with zeros to reach frame size
        let mut padded_samples = samples.to_vec();
        while padded_samples.len() < OPUS_FRAME_SIZE as usize {
            padded_samples.push(0.0);
        }

        let mut opus_frame = vec![0u8; 4000]; // Max Opus frame size
        let encoded_size = self
            .encoder
            .encode_float(&padded_samples, &mut opus_frame)
            .map_err(|e| SdkError::EncoderError(format!("Failed to encode Opus frame: {:?}", e)))?;
        opus_frame.truncate(encoded_size);

        let packet = VoiceData {
            sequence: self.sequence,
            user_id: 0,
            timestamp: self.timestamp,
            opus_frame,
        };

        self.sequence = self.sequence.wrapping_add(1);
        self.timestamp = self.timestamp.wrapping_add(OPUS_FRAME_SIZE);

        Ok(packet)
    }
}
