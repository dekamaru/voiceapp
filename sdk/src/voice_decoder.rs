use neteq::{AudioPacket, NetEq, NetEqConfig, RtpHeader};
use neteq::codec::OpusDecoder;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex as TokioMutex;
use tokio::sync::broadcast;
use voiceapp_protocol::VoiceData;
use tracing::debug;

const SAMPLE_RATE: u32 = 48000;
const OPUS_FRAME_SAMPLES: usize = 960; // 20ms at 48kHz
const FRAME_LENGTH_MS: u32 = 20;
const DECODER_PACKET_ID: u8 = 111;
const CHANNELS: u8 = 1;

pub struct VoiceDecoder {
    neteq: Arc<TokioMutex<NetEq>>,
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
    /// Create a new voice decoder and spawn 10ms audio output loop
    pub fn new(voice_output_tx: broadcast::Sender<Vec<f32>>) -> Result<Self, VoiceDecoderError> {
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

        let neteq = Arc::new(TokioMutex::new(neteq));

        // Spawn 10ms timer loop
        Self::spawn_audio_output_loop(Arc::clone(&neteq), voice_output_tx.clone());

        Ok(VoiceDecoder {
            neteq,
            voice_output_tx,
        })
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

        let mut neteq = self.neteq.lock().await;
        neteq.insert_packet(decoder_packet)
            .map_err(|e| VoiceDecoderError::NetEqError(e.to_string()))
    }

    /// Spawn the 10ms timer task that pulls audio from NetEQ
    fn spawn_audio_output_loop(
        neteq: Arc<TokioMutex<NetEq>>,
        voice_output_tx: broadcast::Sender<Vec<f32>>,
    ) {
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_millis(10));

            loop {
                interval.tick().await;

                let mut neteq = neteq.lock().await;
                match neteq.get_audio() {
                    Ok(audio) => {
                        if let Err(e) = voice_output_tx.send(audio.samples) {
                            debug!("Voice output channel closed: {}", e);
                            break;
                        }
                    }
                    Err(e) => {
                        debug!("NetEQ get_audio error: {}", e);
                    }
                }
            }
        });
    }
}
