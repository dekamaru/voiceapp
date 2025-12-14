use async_channel::Sender;
use cpal::Stream;
use std::sync::{Arc, Mutex};
use tokio::task::JoinHandle;
use tracing::{debug, error, info};
use voiceapp_sdk::{voiceapp_protocol, VoiceDecoder, VoiceInputPipeline, VoiceInputPipelineConfig};

use crate::audio::input::create_input_stream;
use crate::audio::output::{create_output_stream, AudioOutputHandle};

/// Audio manager that handles recording and playback lifecycle
pub struct AudioManager {
    voice_pipeline: Arc<Mutex<Option<VoiceInputPipeline>>>,
    decoder: Arc<VoiceDecoder>,
    input_stream: Arc<Mutex<Option<Stream>>>,
    input_receiver_task: Arc<Mutex<Option<JoinHandle<()>>>>,
    pipeline_forward_task: Arc<Mutex<Option<JoinHandle<()>>>>,
    output_handle: Arc<Mutex<Option<AudioOutputHandle>>>,
    output_receiver_task: Arc<Mutex<Option<JoinHandle<()>>>>,
    udp_send_tx: Sender<Vec<u8>>,
}

impl AudioManager {
    /// Create a new AudioManager with voice decoder and UDP send channel
    pub fn new(decoder: Arc<VoiceDecoder>, udp_send_tx: Sender<Vec<u8>>) -> Self {
        AudioManager {
            voice_pipeline: Arc::new(Mutex::new(None)),
            decoder,
            input_stream: Arc::new(Mutex::new(None)),
            input_receiver_task: Arc::new(Mutex::new(None)),
            pipeline_forward_task: Arc::new(Mutex::new(None)),
            output_handle: Arc::new(Mutex::new(None)),
            output_receiver_task: Arc::new(Mutex::new(None)),
            udp_send_tx,
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

    /// Start playing audio output from decoded voice stream
    pub fn start_playback(&self) -> Result<(), Box<dyn std::error::Error>> {
        info!("Starting audio playback");

        // Create the output stream with decoder
        let output_handle = create_output_stream(self.decoder.clone())?;

        // Store the output handle to keep the stream alive
        *self.output_handle.lock().unwrap() = Some(output_handle);

        info!("Audio playback started");
        Ok(())
    }

    /// Stop playing audio output
    pub fn stop_playback(&self) {
        info!("Stopping audio playback");

        // Drop the output handle (this stops the stream)
        if let Ok(mut handle_opt) = self.output_handle.lock() {
            *handle_opt = None;
        }

        // Abort the output receiver task
        if let Ok(mut task_opt) = self.output_receiver_task.lock() {
            if let Some(task) = task_opt.take() {
                task.abort();
            }
        }

        self.decoder.flush();

        info!("Audio playback stopped");
    }
}
