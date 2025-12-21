use cpal::Stream;
use std::sync::Arc;
use tokio::task::JoinHandle;
use tracing::{debug, error, info};
use voiceapp_sdk::VoiceClient;

use crate::audio::input::create_input_stream;
use crate::audio::output::{create_output_stream, AudioOutputHandle};

/// Audio manager that handles recording and playback lifecycle
pub struct AudioManager {
    voice_client: Arc<VoiceClient>,
    input_stream: Option<Stream>,
    input_receiver_task: Option<JoinHandle<()>>,
    output_streams: std::collections::HashMap<u64, AudioOutputHandle>,
}

impl AudioManager {
    /// Create a new AudioManager with UDP send channel and decoder manager
    pub fn new(voice_client: Arc<VoiceClient>) -> Self {
        Self {
            voice_client,
            input_stream: None,
            input_receiver_task: None,
            output_streams: std::collections::HashMap::new(),
        }
    }

    /// Start recording: create audio stream, pipeline, and forward frames
    pub fn start_recording(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        info!("Starting audio recording");

        // Create the input stream and get actual sample rate
        let (stream, sample_rate, mut receiver) = create_input_stream()?;
        let voice_input_tx = self.voice_client.get_voice_input_sender(sample_rate as usize)?;

        // Spawn task to read from CPAL receiver and forward to voice input
        let task = tokio::spawn(async move {
            while let Some(frame) = receiver.recv().await {
                if let Err(e) = voice_input_tx.send(frame).await {
                    error!("Failed to send audio frame to pipeline: {}", e);
                    break;
                }
            }
            debug!("Audio receiver task ended");
        });

        // Store the stream and tasks
        self.input_stream = Some(stream);
        self.input_receiver_task = Some(task);

        info!("Audio recording started");
        Ok(())
    }

    /// Stop recording: close stream and stop sending voice data
    pub fn stop_recording(&mut self) {
        info!("Stopping audio recording");

        // Drop the input stream (this stops the stream and closes the receiver)
        self.input_stream = None;

        // Abort the input receiver task
        if let Some(task) = self.input_receiver_task.take() {
            task.abort();
        }

        info!("Audio recording stopped");
    }

    /// Create an output stream for a specific user
    pub fn create_output_stream_for_user(&mut self, user_id: u64) -> Result<(), Box<dyn std::error::Error>> {
        info!("Creating output stream for user {}", user_id);

        // TODO: fetch before output stream creation
        let decoder = self.voice_client.get_voice_output_for(user_id, 48000)?;
        // Create output stream for this user's decoder
        let (output_handle, detected_rate) = create_output_stream(decoder)?;

        info!("Created output stream for user {} at {} Hz", user_id, detected_rate);

        // Store the output handle
        self.output_streams.insert(user_id, output_handle);

        Ok(())
    }

    /// Remove output stream for a specific user
    pub fn remove_output_stream_for_user(&mut self, user_id: u64) {
        info!("Removing output stream for user {}", user_id);

        if self.output_streams.remove(&user_id).is_some() {
            info!("Removed output stream for user {}", user_id);
        }

        if self.voice_client.remove_voice_output_for(user_id).is_ok() {
            info!("Removed voice decoder for user {}", user_id);
        }
    }

    pub fn remove_all_output_streams(&mut self) {
        info!("Removing all output streams");

        self.output_streams.clear();
        let _ = self.voice_client.remove_all_voice_outputs();
    }
}
