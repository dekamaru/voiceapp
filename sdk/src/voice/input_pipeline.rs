use async_channel::{Receiver, Sender};
use tracing::{error, info, warn};
use voiceapp_protocol::Packet;
use crate::voice::encoder::{Encoder};
use crate::voice::opus_consts::{OPUS_FRAME_SIZE, OPUS_SAMPLE_RATE};
use crate::voice::resampler::AudioResampler;

/// Voice input pipeline: resamples, buffers, and encodes audio to Opus
pub struct InputPipeline;

impl InputPipeline {
    /// Create a new VoiceInputPipeline with external channels
    pub fn new(
        target_sample_rate: u32,
        voice_input_rx: Receiver<Vec<f32>>,
        udp_send_tx: Sender<Vec<u8>>,
    ) -> Result<Self, String> { // TODO: error handling
        let encoder = Encoder::new().map_err(|e| format!("Encoder creation error: {}", e))?;

        let resampler = if target_sample_rate != OPUS_SAMPLE_RATE {
            Some(AudioResampler::new(
                target_sample_rate,
                OPUS_SAMPLE_RATE,
                OPUS_FRAME_SIZE
            ).map_err(|e| format!("AudioResampler creation error: {}", e))?)
        } else {
            None
        };

        // Spawn the pipeline processing task
        tokio::spawn(Self::pipeline_task(encoder, resampler, voice_input_rx, udp_send_tx));

        Ok(InputPipeline {})
    }

    /// Resample audio frame and append to encode buffer
    fn resample(
        frame: &[f32],
        resampler: &mut Option<AudioResampler>,
        resample_buffer: &mut Vec<f32>,
        encode_buffer: &mut Vec<f32>,
    ) {
        const RESAMPLER_CHUNK_SIZE: usize = 480;

        match resampler {
            Some(resampler) => {
                resample_buffer.extend_from_slice(frame);
                while resample_buffer.len() >= RESAMPLER_CHUNK_SIZE {
                    let input_chunk = resample_buffer
                        .drain(0..RESAMPLER_CHUNK_SIZE)
                        .collect();

                    match resampler.resample(input_chunk) {
                        Ok(resampled) => { encode_buffer.extend_from_slice(&resampled) }
                        Err(e) => { error!("Resampling error: {}", e); }
                    }
                }
            },
            None => { encode_buffer.extend_from_slice(frame); }
        }
    }

    /// Encode frames from buffer and send to UDP
    async fn encode_and_send(
        encoder: &mut Encoder,
        encode_buffer: &mut Vec<f32>,
        udp_send_tx: &Sender<Vec<u8>>,
    ) -> bool {
        while encode_buffer.len() >= OPUS_FRAME_SIZE as usize {
            let frame: Vec<f32> = encode_buffer.drain(0..OPUS_FRAME_SIZE as usize).collect();

            match encoder.encode(&frame) {
                Ok(voice_data) => {
                    // Encode VoiceData to Packet and send to UDP
                    let packet = Packet::VoiceData {
                        user_id: voice_data.user_id,
                        sequence: voice_data.sequence,
                        timestamp: voice_data.timestamp,
                        data: voice_data.opus_frame,
                    };

                    if udp_send_tx.send(packet.encode()).await.is_err() {
                        error!("UDP send channel closed, stopping pipeline");
                        return false;
                    }
                }
                Err(e) => {
                    error!("Encoding error: {}", e);
                    return false;
                }
            }
        }
        true
    }

    /// Internal pipeline task: processes audio from input_rx and sends encoded data to UDP
    async fn pipeline_task(
        mut encoder: Encoder,
        mut resampler: Option<AudioResampler>,
        input_rx: Receiver<Vec<f32>>,
        udp_send_tx: Sender<Vec<u8>>,
    ) {
        const RESAMPLER_CHUNK_SIZE: usize = 480;

        let mut resample_buffer = Vec::with_capacity(RESAMPLER_CHUNK_SIZE * 2);
        let mut encode_buffer = Vec::with_capacity(OPUS_FRAME_SIZE as usize * 2);

        loop {
            match input_rx.recv().await {
                Ok(frame) => {
                    Self::resample(&frame, &mut resampler, &mut resample_buffer, &mut encode_buffer);
                    if !Self::encode_and_send(&mut encoder, &mut encode_buffer, &udp_send_tx).await {
                        return;
                    }
                }
                Err(_) => { break; }
            }
        }

        info!("Voice input pipeline stopped");
    }
}
