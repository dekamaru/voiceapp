use neteq::codec::{AudioDecoder, OpusDecoder};
use neteq::{AudioPacket, NetEq, NetEqConfig, RtpHeader};
use rubato::{FftFixedIn, Resampler};
use std::sync::{Arc, Mutex};
use voiceapp_protocol::VoiceData;

const OPUS_SAMPLE_RATE: u32 = 48000;
const FRAME_LENGTH_MS: u32 = 20;
const DECODER_PACKET_ID: u8 = 111;
const CHANNELS: u8 = 1;
const OPUS_FRAME_SIZE: usize = 960; // 20ms * 48kHz

/// Custom decoder that wraps Opus decoding with optional resampling
struct OpusResamplingDecoder {
    /// Underlying Opus decoder (always outputs 48kHz mono)
    opus_decoder: OpusDecoder,

    /// Optional resampler (None if target_rate == 48kHz)
    resampler: Option<FftFixedIn<f32>>,

    /// Pre-allocated output buffer for zero-allocation resampling
    resample_out_buffer: Vec<Vec<f32>>,

    /// Target sample rate for output
    target_sample_rate: u32,
}

impl OpusResamplingDecoder {
    fn new(target_sample_rate: u32) -> Result<Self, neteq::NetEqError> {
        // Create Opus decoder at 48kHz
        let opus_decoder = OpusDecoder::new(OPUS_SAMPLE_RATE, CHANNELS)
            .map_err(|e| neteq::NetEqError::DecoderError(
                format!("Failed to create Opus decoder: {}", e)
            ))?;

        // Create resampler if needed
        let mut resampler = if target_sample_rate != OPUS_SAMPLE_RATE {
            let resampler = FftFixedIn::<f32>::new(
                OPUS_SAMPLE_RATE as usize,      // 48000
                target_sample_rate as usize,     // target
                OPUS_FRAME_SIZE,                 // 960 samples
                2,                               // sub_chunks (quality/performance balance)
                1, // mono
            ).map_err(|e| neteq::NetEqError::DecoderError(
                format!("Failed to create resampler: {}", e)
            ))?;

            Some(resampler)
        } else {
            None
        };

        // Pre-allocate output buffer for zero-allocation processing
        let resample_out_buffer = if let Some(ref mut r) = resampler {
            r.output_buffer_allocate(true)
        } else {
            Vec::new()
        };

        Ok(Self {
            opus_decoder,
            resampler,
            resample_out_buffer,
            target_sample_rate,
        })
    }
}

impl AudioDecoder for OpusResamplingDecoder {
    fn sample_rate(&self) -> u32 { self.target_sample_rate }

    fn channels(&self) -> u8 { 1 }

    fn decode(&mut self, encoded: &[u8]) -> neteq::Result<Vec<f32>> {
        // Step 1: Decode Opus to 48kHz
        let decoded_48k = self.opus_decoder.decode(encoded)?;

        // Step 2: Resample if needed
        match &mut self.resampler {
            None => {
                // No resampling needed
                Ok(decoded_48k)
            }
            Some(resampler) => {
                // Use pre-allocated output buffer for zero-allocation processing
                let (_, resampled_size) = resampler
                    .process_into_buffer(&[&decoded_48k], &mut self.resample_out_buffer, None)
                    .map_err(|e| neteq::NetEqError::DecoderError(
                        format!("Resampling failed: {}", e)
                    ))?;

                // Extract resampled samples from mono channel
                Ok(self.resample_out_buffer[0][0..resampled_size].to_vec())
            }
        }
    }
}

// Required for NetEQ registration
unsafe impl Send for OpusResamplingDecoder {}

pub struct VoiceDecoder {
    neteq: Arc<Mutex<NetEq>>,
}

#[derive(Debug, Clone)]
pub enum VoiceDecoderError {
    NetEqError(String),
}

impl std::fmt::Display for VoiceDecoderError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            VoiceDecoderError::NetEqError(e) => write!(f, "NetEq error: {}", e),
        }
    }
}

impl std::error::Error for VoiceDecoderError {}

impl VoiceDecoder {
    /// Create a new voice decoder with the specified target sample rate
    pub fn new(target_sample_rate: u32) -> Result<Self, VoiceDecoderError> {
        let neteq_config = NetEqConfig {
            sample_rate: OPUS_SAMPLE_RATE,  // NetEQ operates at target rate
            channels: CHANNELS,
            ..Default::default()
        };

        let mut neteq =
            NetEq::new(neteq_config).map_err(|e| VoiceDecoderError::NetEqError(e.to_string()))?;

        // Create custom resampling decoder
        let decoder = OpusResamplingDecoder::new(target_sample_rate)
            .map_err(|e| VoiceDecoderError::NetEqError(e.to_string()))?;
        neteq.register_decoder(DECODER_PACKET_ID, Box::new(decoder));

        let neteq = Arc::new(Mutex::new(neteq));

        Ok(VoiceDecoder { neteq })
    }

    /// Insert a received voice packet into NetEQ for buffering and reordering
    pub async fn insert_packet(&self, packet: VoiceData) -> Result<(), VoiceDecoderError> {
        let decoder_header = RtpHeader::new(
            packet.sequence as u16,
            packet.timestamp,
            packet.ssrc as u32,
            DECODER_PACKET_ID,
            false,
        );
        let decoder_packet = AudioPacket::new(
            decoder_header,
            packet.opus_frame,
            OPUS_SAMPLE_RATE,  // Opus operates at 48kHz
            CHANNELS,
            FRAME_LENGTH_MS,
        );

        let mut neteq = self.neteq.lock().unwrap();
        neteq
            .insert_packet(decoder_packet)
            .map_err(|e| VoiceDecoderError::NetEqError(e.to_string()))
    }

    pub fn get_audio(&self) -> Result<Vec<f32>, VoiceDecoderError> {
        let mut neteq = self.neteq.lock().unwrap();
        neteq
            .get_audio()
            .map(|frame| frame.samples)
            .map_err(|e| VoiceDecoderError::NetEqError(e.to_string()))
    }

    pub fn flush(&self) {
        self.neteq.lock().unwrap().flush();
    }
}
