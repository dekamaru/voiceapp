use async_channel::{Receiver, Sender};
use rubato::{FftFixedIn, Resampler};
use tracing::{error, info, warn};
use voiceapp_protocol::Packet;
use crate::voice_encoder::{VoiceEncoder, OPUS_FRAME_SAMPLES};

/// Configuration for VoiceInputPipeline
pub struct VoiceInputPipelineConfig {
    pub sample_rate: usize,
}

/// Voice input pipeline: resamples, buffers, and encodes audio to Opus
pub struct VoiceInputPipeline {
    _handle: tokio::task::JoinHandle<()>,
}

impl VoiceInputPipeline {
    /// Create a new VoiceInputPipeline with external channels
    pub fn new(
        config: VoiceInputPipelineConfig,
        voice_input_rx: Receiver<Vec<f32>>,
        udp_send_tx: Sender<Vec<u8>>,
    ) -> Result<Self, String> {
        // Spawn the pipeline processing task
        let handle = tokio::spawn(Self::pipeline_task(config, voice_input_rx, udp_send_tx));

        Ok(VoiceInputPipeline {
            _handle: handle,
        })
    }

    /// Internal pipeline task: processes audio from input_rx and sends encoded data to UDP
    async fn pipeline_task(
        config: VoiceInputPipelineConfig,
        input_rx: Receiver<Vec<f32>>,
        udp_send_tx: Sender<Vec<u8>>,
    ) {
        const TARGET_SAMPLE_RATE: usize = 48000;
        const RESAMPLER_CHUNK_SIZE: usize = 480;  // Optimized for Opus frame alignment (480 × 2 = 960)
        const RESAMPLER_SUB_CHUNKS: usize = 2;    // FFT overlap-add quality/performance balance
        const RESAMPLE_BUFFER_CAPACITY: usize = RESAMPLER_CHUNK_SIZE * 3;  // 1440 samples
        const ENCODE_BUFFER_CAPACITY: usize = OPUS_FRAME_SAMPLES * 2;      // 1920 samples

        // Create resampler if needed
        let mut resampler = if config.sample_rate != TARGET_SAMPLE_RATE {
            match FftFixedIn::<f32>::new(
                config.sample_rate,
                TARGET_SAMPLE_RATE,
                RESAMPLER_CHUNK_SIZE,
                RESAMPLER_SUB_CHUNKS, // sub-chunks for overlap-add
                1, // mono
            ) {
                Ok(r) => Some(r),
                Err(e) => {
                    error!("Failed to create resampler: {}", e);
                    return;
                }
            }
        } else {
            None
        };

        // Initialize encoder
        let mut codec = match VoiceEncoder::new() {
            Ok(c) => c,
            Err(e) => {
                error!("Failed to create voice encoder: {}", e);
                return;
            }
        };

        // Buffers
        let mut resample_buffer = Vec::with_capacity(RESAMPLE_BUFFER_CAPACITY);
        let mut encode_buffer = Vec::with_capacity(ENCODE_BUFFER_CAPACITY);

        // Pre-allocate resampler output buffer for zero-allocation processing
        let mut resample_out_buffer = if let Some(ref mut r) = resampler {
            r.output_buffer_allocate(true)
        } else {
            Vec::new()
        };

        info!(
            "Voice input pipeline started with sample_rate={}",
            config.sample_rate
        );

        loop {
            match input_rx.recv().await {
                Ok(frame) => {
                    // Handle resampling if input rate differs from 48kHz
                    if config.sample_rate != TARGET_SAMPLE_RATE {
                        resample_buffer.extend_from_slice(&frame);

                        while resample_buffer.len() >= RESAMPLER_CHUNK_SIZE {
                            if let Some(ref mut resampler_inst) = resampler {
                                let input_chunk: Vec<f32> =
                                    resample_buffer.drain(0..RESAMPLER_CHUNK_SIZE).collect();

                                // Use pre-allocated output buffer for zero-allocation processing
                                match resampler_inst.process_into_buffer(
                                    &[&input_chunk],
                                    &mut resample_out_buffer,
                                    None,
                                ) {
                                    Ok((_, resampled_size)) => {
                                        encode_buffer.extend_from_slice(
                                            &resample_out_buffer[0][0..resampled_size],
                                        );

                                        // Monitor buffer growth in debug builds
                                        #[cfg(debug_assertions)]
                                        if encode_buffer.len() > OPUS_FRAME_SAMPLES * 3 {
                                            warn!("Encode buffer growing large: {} samples", encode_buffer.len());
                                        }
                                    }
                                    Err(e) => {
                                        error!("Resampling error: {}", e);
                                    }
                                }
                            }
                        }
                    } else {
                        // No resampling needed
                        encode_buffer.extend_from_slice(&frame);
                    }

                    // Encode complete frames
                    while encode_buffer.len() >= OPUS_FRAME_SAMPLES {
                        let frame: Vec<f32> = encode_buffer.drain(0..OPUS_FRAME_SAMPLES).collect();

                        match codec.encode(&frame) {
                            Ok(Some(voice_data)) => {
                                // Encode VoiceData to Packet and send to UDP
                                let packet = Packet::VoiceData {
                                    user_id: voice_data.ssrc,
                                    sequence: voice_data.sequence,
                                    timestamp: voice_data.timestamp,
                                    data: voice_data.opus_frame,
                                };

                                if udp_send_tx.send(packet.encode()).await.is_err() {
                                    error!("UDP send channel closed, stopping pipeline");
                                    return;
                                }
                            }
                            Ok(None) => {
                                error!("Encoder returned None for full frame");
                                return;
                            }
                            Err(e) => {
                                error!("Encoding error: {}", e);
                                return;
                            }
                        }
                    }
                }
                Err(_) => {
                    // Input channel closed, encode any remaining samples
                    if !encode_buffer.is_empty() {
                        match codec.encode(&encode_buffer) {
                            Ok(Some(voice_data)) => {
                                let packet = Packet::VoiceData {
                                    user_id: voice_data.ssrc,
                                    sequence: voice_data.sequence,
                                    timestamp: voice_data.timestamp,
                                    data: voice_data.opus_frame,
                                };
                                let _ = udp_send_tx.send(packet.encode()).await;
                            }
                            Ok(None) => {
                                // No final frame to send
                            }
                            Err(e) => {
                                error!("Error encoding remaining samples: {}", e);
                            }
                        }
                    }
                    break;
                }
            }
        }

        info!("Voice input pipeline stopped");
    }
}
