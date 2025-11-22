use neteq::{AudioPacket, NetEq, NetEqConfig, RtpHeader};
use neteq::codec::OpusDecoder;
use std::sync::{Arc, Mutex};
use voiceapp_protocol::VoiceData;

const SAMPLE_RATE: u32 = 48000;
const FRAME_LENGTH_MS: u32 = 20;
const DECODER_PACKET_ID: u8 = 111;
const CHANNELS: u8 = 1;

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
    /// Create a new voice decoder
    pub fn new() -> Result<Self, VoiceDecoderError> {
        let neteq_config = NetEqConfig {
            sample_rate: SAMPLE_RATE,
            channels: CHANNELS,
            ..Default::default()
        };

        let mut neteq = NetEq::new(neteq_config)
            .map_err(|e| VoiceDecoderError::NetEqError(e.to_string()))?;

        let decoder = OpusDecoder::new(SAMPLE_RATE, CHANNELS)
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
            SAMPLE_RATE,
            CHANNELS,
            FRAME_LENGTH_MS,
        );

        let mut neteq = self.neteq.lock().unwrap();
        neteq.insert_packet(decoder_packet)
            .map_err(|e| VoiceDecoderError::NetEqError(e.to_string()))
    }

    /// Get decoded audio from NetEQ (called from CPAL callback on-demand)
    pub fn get_audio(&self) -> Result<Vec<f32>, VoiceDecoderError> {
        let mut neteq = self.neteq.lock().unwrap();
        neteq.get_audio()
            .map(|frame| frame.samples)
            .map_err(|e| VoiceDecoderError::NetEqError(e.to_string()))
    }

    pub fn flush(&self) {
        self.neteq.lock().unwrap().flush();
    }
}
