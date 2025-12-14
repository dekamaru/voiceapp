use async_channel::{unbounded, Receiver, Sender};
use rubato::{FftFixedIn, Resampler};
use tracing::{error, info};

use crate::voice_encoder::{VoiceEncoder, OPUS_FRAME_SAMPLES};
use voiceapp_protocol::VoiceData;

/// Configuration for VoiceInputPipeline
pub struct VoiceInputPipelineConfig {
    pub sample_rate: usize,
}

/// Voice input pipeline: resamples, buffers, and encodes audio to Opus
pub struct VoiceInputPipeline {
    input_tx: Sender<Vec<f32>>,
    output_rx: Receiver<VoiceData>,
}

impl VoiceInputPipeline {
    /// Create a new VoiceInputPipeline with fixed sample rate configuration
    pub fn new(config: VoiceInputPipelineConfig) -> Result<Self, String> {
        let (input_tx, input_rx) = unbounded();
        let (output_tx, output_rx) = unbounded();

        // Spawn the pipeline processing task
        tokio::spawn(Self::pipeline_task(config, input_rx, output_tx));

        Ok(VoiceInputPipeline {
            input_tx,
            output_rx,
        })
    }

    /// Get a cloneable sender for audio samples
    pub fn input_sender(&self) -> Sender<Vec<f32>> {
        self.input_tx.clone()
    }

    /// Get a cloneable receiver for encoded voice data
    pub fn output_receiver(&self) -> Receiver<VoiceData> {
        self.output_rx.clone()
    }

    /// Internal pipeline task: processes audio from input_rx and sends VoiceData to output_tx
    async fn pipeline_task(
        config: VoiceInputPipelineConfig,
        input_rx: Receiver<Vec<f32>>,
        output_tx: Sender<VoiceData>,
    ) {
        const TARGET_SAMPLE_RATE: usize = 48000;
        const RESAMPLER_CHUNK_SIZE: usize = 512;

        // Create resampler if needed
        let mut resampler = if config.sample_rate != TARGET_SAMPLE_RATE {
            match FftFixedIn::<f32>::new(
                config.sample_rate,
                TARGET_SAMPLE_RATE,
                RESAMPLER_CHUNK_SIZE,
                2, // sub-chunks for overlap-add
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
        let mut resample_buffer = Vec::with_capacity(RESAMPLER_CHUNK_SIZE * 2);
        let mut encode_buffer = Vec::with_capacity(OPUS_FRAME_SAMPLES * 2);

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
                            Ok(Some(packet)) => {
                                if output_tx.send(packet).await.is_err() {
                                    error!("Output channel closed, stopping pipeline");
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
                            Ok(Some(packet)) => {
                                let _ = output_tx.send(packet).await;
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
