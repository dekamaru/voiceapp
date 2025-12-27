use neteq::{AudioPacket, NetEq, NetEqConfig, RtpHeader};
use std::sync::{Mutex};
use thiserror::Error;
use crate::voice::opus_consts::{OPUS_CHANNELS, OPUS_DECODER_PACKET_ID, OPUS_FRAME_SIZE, OPUS_SAMPLE_RATE, OPUS_FRAME_LENGTH_MS};
pub(crate) use crate::voice::models::VoiceData;
use crate::voice::neteq::opus_resampling_decoder::{OpusResamplingDecoder};

pub struct Decoder {
    neteq: Mutex<NetEq>,
}

#[derive(Debug, Clone, Error)]
pub enum DecoderError {
    #[error("NetEQ error: {0}")]
    NetEqError(String),

    #[error("Resampler initialization error: {0}")]
    ResamplingInitializationError(String),

    #[error("Lock error: {0}")]
    LockError(String),
}

impl Decoder {
    /// Create a new voice decoder with the specified target sample rate
    pub fn new(target_sample_rate: u32) -> Result<Self, DecoderError> {
        let neteq_config = NetEqConfig {
            sample_rate: OPUS_SAMPLE_RATE,
            channels: OPUS_CHANNELS,
            ..Default::default()
        };

        let mut neteq = NetEq::new(neteq_config)
            .map_err(|e| DecoderError::NetEqError(e.to_string()))?;

        let decoder = OpusResamplingDecoder::new(
            OPUS_SAMPLE_RATE,
            target_sample_rate,
            OPUS_CHANNELS,
            OPUS_FRAME_SIZE
        ).map_err(|e| DecoderError::ResamplingInitializationError(e.to_string()))?;

        neteq.register_decoder(OPUS_DECODER_PACKET_ID, Box::new(decoder));

        Ok(Decoder { neteq: Mutex::new(neteq) })
    }

    pub fn consume_voice_data(&self, packet: VoiceData) -> Result<(), DecoderError> {
        let mut neteq = self.neteq.lock().map_err(|e| DecoderError::LockError(e.to_string()))?;

        neteq
            .insert_packet(self.create_neteq_packet(packet))
            .map_err(|e| DecoderError::NetEqError(e.to_string()))
    }

    pub fn get_decoded_audio(&self) -> Result<Vec<f32>, DecoderError> {
        let mut neteq = self.neteq.lock().map_err(|e| DecoderError::LockError(e.to_string()))?;

        neteq
            .get_audio()
            .map(|frame| frame.samples)
            .map_err(|e| DecoderError::NetEqError(e.to_string()))
    }

    fn create_neteq_packet(&self, packet: VoiceData) -> AudioPacket {
        let decoder_header = RtpHeader::new(
            packet.sequence as u16,
            packet.timestamp,
            0,
            OPUS_DECODER_PACKET_ID,
            false,
        );

        AudioPacket::new(
            decoder_header,
            packet.opus_frame,
            OPUS_SAMPLE_RATE,
            OPUS_CHANNELS,
            OPUS_FRAME_LENGTH_MS,
        )
    }
}
