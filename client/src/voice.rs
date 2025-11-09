use opus::{Channels, Encoder};
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::debug;
use voiceapp_common::{VoicePacket, username_to_ssrc};

const SAMPLE_RATE: u32 = 48000;
const OPUS_FRAME_SAMPLES: usize = 960; // 20ms at 48kHz

/// Manages Opus encoding of audio frames
pub struct VoiceEncoder {
    encoder: Encoder,
    pub ssrc: u32, // Deterministically computed from username
    pub sequence: u32,
    pub timestamp: u32,
    sample_buffer: Vec<f32>,
}

impl VoiceEncoder {
    /// Create a new voice encoder for the given username
    pub fn new(username: String) -> Result<Self, opus::Error> {
        let mut encoder = Encoder::new(SAMPLE_RATE, Channels::Mono, opus::Application::Voip)?;

        // Set encoding parameters
        encoder.set_bitrate(opus::Bitrate::Max)?;

        // Compute SSRC from username
        let ssrc = username_to_ssrc(&username);

        Ok(VoiceEncoder {
            encoder,
            ssrc,
            sequence: 0,
            timestamp: 0,
            sample_buffer: Vec::with_capacity(OPUS_FRAME_SAMPLES * 2),
        })
    }

    /// Add audio samples and return any complete Opus frames
    pub fn encode_frame(&mut self, samples: &[f32]) -> Result<Vec<VoicePacket>, String> {
        self.sample_buffer.extend_from_slice(samples);

        let mut packets = Vec::new();

        // Encode all complete frames
        while self.sample_buffer.len() >= OPUS_FRAME_SAMPLES {
            let frame_samples: Vec<f32> = self.sample_buffer.drain(0..OPUS_FRAME_SAMPLES).collect();

            let mut opus_frame = vec![0u8; 4000]; // Max Opus frame size
            let encoded_size = self.encoder.encode_float(&frame_samples, &mut opus_frame)
                .map_err(|e| format!("Failed to encode Opus frame: {:?}", e))?;
            opus_frame.truncate(encoded_size);

            debug!("Encoded {} samples to {} bytes", OPUS_FRAME_SAMPLES, encoded_size);

            let packet = VoicePacket::new(self.sequence, self.timestamp, self.ssrc, opus_frame);
            packets.push(packet);

            self.sequence = self.sequence.wrapping_add(1);
            self.timestamp = self.timestamp.wrapping_add(OPUS_FRAME_SAMPLES as u32);
        }

        Ok(packets)
    }

    /// Flush any remaining samples in the buffer
    pub fn flush(&mut self) -> Result<Option<VoicePacket>, String> {
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

        let packet = VoicePacket::new(self.sequence, self.timestamp, self.ssrc, opus_frame);

        Ok(Some(packet))
    }
}

/// Thread-safe wrapper for voice encoding
pub struct VoiceEncoderHandle {
    encoder: Arc<Mutex<VoiceEncoder>>,
}

impl VoiceEncoderHandle {
    /// Create a new encoder handle for the given username
    pub fn new(username: String) -> Result<Self, opus::Error> {
        let encoder = VoiceEncoder::new(username)?;
        Ok(VoiceEncoderHandle {
            encoder: Arc::new(Mutex::new(encoder)),
        })
    }

    /// Encode audio samples asynchronously
    pub async fn encode_frame(&self, samples: Vec<f32>) -> Result<Vec<VoicePacket>, String> {
        let mut encoder = self.encoder.lock().await;
        encoder.encode_frame(&samples)
    }

    /// Flush remaining samples
    pub async fn flush(&self) -> Result<Option<VoicePacket>, String> {
        let mut encoder = self.encoder.lock().await;
        encoder.flush()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_voice_encoder_creation() {
        let result = VoiceEncoder::new("test_user".to_string());
        assert!(result.is_ok());
    }

    #[test]
    fn test_encode_single_frame() {
        let mut encoder = VoiceEncoder::new("test_user".to_string()).expect("Failed to create encoder");

        // Create a test frame of 960 samples with low entropy (zeros compress well)
        let samples: Vec<f32> = vec![0.0; 960];

        let packets = encoder.encode_frame(&samples).expect("Encoding failed");

        assert_eq!(packets.len(), 1);
        assert_eq!(packets[0].sequence, 0);
        assert_eq!(packets[0].timestamp, 0);
        assert!(!packets[0].opus_frame.is_empty());
        assert!(packets[0].opus_frame.len() < 100); // Silence compresses very well
    }

    #[test]
    fn test_encode_multiple_frames() {
        let mut encoder = VoiceEncoder::new("test_user".to_string()).expect("Failed to create encoder");

        // Create 2.5 frames worth of samples (2400 samples)
        let samples: Vec<f32> = (0..2400).map(|i| (i as f32 / 1000.0).sin()).collect();

        let packets = encoder.encode_frame(&samples).expect("Encoding failed");

        // Should get 2 complete frames, with 480 samples remaining in buffer
        assert_eq!(packets.len(), 2);
        assert_eq!(packets[0].sequence, 0);
        assert_eq!(packets[1].sequence, 1);
        assert_eq!(packets[0].timestamp, 0);
        assert_eq!(packets[1].timestamp, 960);
    }

    #[test]
    fn test_encode_small_samples() {
        let mut encoder = VoiceEncoder::new("test_user".to_string()).expect("Failed to create encoder");

        // Add 480 samples (half frame)
        let samples: Vec<f32> = (0..480).map(|i| (i as f32 / 1000.0).sin()).collect();

        let packets = encoder.encode_frame(&samples).expect("Encoding failed");

        // Should get no complete frames yet
        assert_eq!(packets.len(), 0);

        // Add another 480 samples to complete the frame
        let samples2: Vec<f32> = (480..960).map(|i| (i as f32 / 1000.0).sin()).collect();

        let packets = encoder.encode_frame(&samples2).expect("Encoding failed");

        // Now we should get 1 frame
        assert_eq!(packets.len(), 1);
        assert_eq!(packets[0].sequence, 0);
    }

    #[test]
    fn test_flush_incomplete_frame() {
        let mut encoder = VoiceEncoder::new("test_user".to_string()).expect("Failed to create encoder");

        // Add incomplete frame
        let samples: Vec<f32> = (0..500).map(|i| (i as f32 / 1000.0).sin()).collect();
        encoder.encode_frame(&samples).expect("Encoding failed");

        let flushed = encoder.flush().expect("Flush failed");

        assert!(flushed.is_some());
        let packet = flushed.unwrap();
        assert_eq!(packet.sequence, 0);
        assert!(!packet.opus_frame.is_empty());
    }

    #[test]
    fn test_flush_empty_buffer() {
        let mut encoder = VoiceEncoder::new("test_user".to_string()).expect("Failed to create encoder");

        let flushed = encoder.flush().expect("Flush failed");

        // Should return None when buffer is empty
        assert!(flushed.is_none());
    }

    #[test]
    fn test_sequence_wrapping() {
        let mut encoder = VoiceEncoder::new("test_user".to_string()).expect("Failed to create encoder");
        encoder.sequence = u32::MAX; // Start at max

        // Create 2 frames worth of samples
        let samples: Vec<f32> = vec![0.0; 1920];

        let packets = encoder.encode_frame(&samples).expect("Encoding failed");

        assert_eq!(packets.len(), 2);
        assert_eq!(packets[0].sequence, u32::MAX); // First packet at max
        assert_eq!(packets[1].sequence, 0); // Second packet wraps to 0
    }

    #[test]
    fn test_timestamp_progression() {
        let mut encoder = VoiceEncoder::new("test_user".to_string()).expect("Failed to create encoder");

        let samples: Vec<f32> = (0..2880).map(|i| (i as f32 / 1000.0).sin()).collect();

        let packets = encoder.encode_frame(&samples).expect("Encoding failed");

        assert_eq!(packets.len(), 3);
        assert_eq!(packets[0].timestamp, 0);
        assert_eq!(packets[1].timestamp, 960);
        assert_eq!(packets[2].timestamp, 1920);
    }

    #[test]
    fn test_ssrc_from_username() {
        use voiceapp_common::username_to_ssrc;

        let encoder1 = VoiceEncoder::new("alice".to_string()).expect("Failed to create encoder 1");
        let encoder2 = VoiceEncoder::new("bob".to_string()).expect("Failed to create encoder 2");

        // SSRCs should be computed from usernames
        assert_ne!(encoder1.ssrc, encoder2.ssrc);
        assert_eq!(encoder1.ssrc, username_to_ssrc("alice"));
        assert_eq!(encoder2.ssrc, username_to_ssrc("bob"));
    }
}
