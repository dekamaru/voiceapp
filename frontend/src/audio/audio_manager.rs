use async_channel::Sender;
use cpal::Stream;
use std::sync::{Arc, Mutex};
use tokio::task::JoinHandle;
use tracing::{debug, error, info};
use voiceapp_sdk::{voiceapp_protocol, VoiceClient, VoiceInputPipeline, VoiceInputPipelineConfig};

use crate::audio::input::create_input_stream;
use crate::audio::output::{create_output_stream, AudioOutputHandle};

/// Audio manager that handles recording and playback lifecycle
#[derive(Clone)]
pub struct AudioManager {
    voice_pipeline: Arc<Mutex<Option<VoiceInputPipeline>>>,
    decoder_manager: Arc<voiceapp_sdk::VoiceDecoderManager>,
    input_stream: Arc<Mutex<Option<Stream>>>,
    input_receiver_task: Arc<Mutex<Option<JoinHandle<()>>>>,
    pipeline_forward_task: Arc<Mutex<Option<JoinHandle<()>>>>,
    output_streams: Arc<Mutex<std::collections::HashMap<u64, AudioOutputHandle>>>,
    udp_send_tx: Sender<Vec<u8>>,
}

impl AudioManager {
    /// Create a new AudioManager with UDP send channel and decoder manager
    pub fn new(voice_client: Arc<VoiceClient>) -> Self {
        Self {
            voice_pipeline: Arc::new(Mutex::new(None)),
            decoder_manager: voice_client.get_decoder_manager(),
            input_stream: Arc::new(Mutex::new(None)),
            input_receiver_task: Arc::new(Mutex::new(None)),
            pipeline_forward_task: Arc::new(Mutex::new(None)),
            output_streams: Arc::new(Mutex::new(std::collections::HashMap::new())),
            udp_send_tx: voice_client.get_udp_send_tx()
        }
    }

    /// Start recording: create audio stream, pipeline, and forward frames
    pub fn start_recording(&self) -> Result<(), Box<dyn std::error::Error>> {
        info!("Starting audio recording");

        // Create the input stream and get actual sample rate
        let (stream, sample_rate, receiver) = create_input_stream()?;

        // Create voice input pipeline with detected sample rate
        let pipeline = VoiceInputPipeline::new(VoiceInputPipelineConfig {
            sample_rate: sample_rate as usize,
        })?;
        let voice_input_tx = pipeline.input_sender();
        let pipeline_output_rx = pipeline.output_receiver();

        // Store pipeline
        *self.voice_pipeline.lock().unwrap() = Some(pipeline);

        // Spawn task to read from CPAL receiver and forward to pipeline
        let task = tokio::spawn(async move {
            let mut rx = receiver;
            while let Some(frame) = rx.recv().await {
                if let Err(e) = voice_input_tx.send(frame).await {
                    error!("Failed to send audio frame to pipeline: {}", e);
                    break;
                }
            }
            debug!("Audio receiver task ended");
        });

        // Spawn task to forward encoded voice data to UDP
        let udp_send_tx = self.udp_send_tx.clone();
        let forward_task = tokio::spawn(async move {
            loop {
                match pipeline_output_rx.recv().await {
                    Ok(voice_data) => {
                        let encoded = voiceapp_protocol::encode_voice_data(&voice_data);
                        if let Err(e) = udp_send_tx.send(encoded).await {
                            error!("Failed to send voice data to UDP: {}", e);
                            break;
                        }
                    }
                    Err(_) => {
                        debug!("Pipeline output channel closed");
                        break;
                    }
                }
            }
        });

        // Store the stream and tasks
        *self.input_stream.lock().unwrap() = Some(stream);
        *self.input_receiver_task.lock().unwrap() = Some(task);
        *self.pipeline_forward_task.lock().unwrap() = Some(forward_task);

        info!("Audio recording started");
        Ok(())
    }

    /// Stop recording: close stream and stop sending voice data
    pub fn stop_recording(&self) {
        info!("Stopping audio recording");

        // Drop the input stream (this stops the stream and closes the receiver)
        if let Ok(mut stream_opt) = self.input_stream.lock() {
            *stream_opt = None;
        }

        // Abort the input receiver task
        if let Ok(mut task_opt) = self.input_receiver_task.lock() {
            if let Some(task) = task_opt.take() {
                task.abort();
            }
        }

        info!("Audio recording stopped");
    }

    /// Create an output stream for a specific user
    pub fn create_stream_for_user(&self, user_id: u64) -> Result<(), Box<dyn std::error::Error>> {
        info!("Creating output stream for user {}", user_id);

        // Get decoder for this user
        let decoder = self.decoder_manager.get_decoder(user_id)
            .ok_or_else(|| format!("No decoder found for user {}", user_id))?;

        // Create output stream for this user's decoder
        let (output_handle, detected_rate) = create_output_stream(decoder)?;

        info!("Created output stream for user {} at {} Hz", user_id, detected_rate);

        // Store the output handle
        let mut streams = self.output_streams.lock().unwrap();
        streams.insert(user_id, output_handle);

        Ok(())
    }

    /// Remove output stream for a specific user
    pub fn remove_stream_for_user(&self, user_id: u64) {
        info!("Removing output stream for user {}", user_id);

        let mut streams = self.output_streams.lock().unwrap();
        if streams.remove(&user_id).is_some() {
            info!("Removed output stream for user {}", user_id);
        }
    }

    /// Stop playing audio output for all users
    pub fn stop_playback(&self) {
        info!("Stopping audio playback");

        // Drop all output streams
        if let Ok(mut streams) = self.output_streams.lock() {
            let count = streams.len();
            streams.clear();
            info!("Stopped {} output streams", count);
        }

        // Flush all decoders
        let decoder_manager = self.decoder_manager.clone();
        decoder_manager.flush_all();

        info!("Audio playback stopped");
    }
}
