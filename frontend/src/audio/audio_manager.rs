use cpal::Stream;
use async_channel::Sender;
use tokio::task::JoinHandle;
use tracing::{debug, error, info};
use std::sync::{Arc, Mutex};
use voiceapp_sdk::voice_client::VoiceFrame;
use voiceapp_sdk::VoiceDecoder;

use crate::audio::input::create_input_stream;
use crate::audio::output::{create_output_stream, AudioOutputHandle};

/// Audio manager that handles recording and playback lifecycle
pub struct AudioManager {
    voice_input_tx: Sender<VoiceFrame>,
    decoder: Arc<VoiceDecoder>,
    input_stream: Arc<Mutex<Option<Stream>>>,
    input_receiver_task: Arc<Mutex<Option<JoinHandle<()>>>>,
    output_handle: Arc<Mutex<Option<AudioOutputHandle>>>,
    output_receiver_task: Arc<Mutex<Option<JoinHandle<()>>>>,
}

impl AudioManager {
    /// Create a new AudioManager with voice input sender and voice decoder
    pub fn new(voice_input_tx: Sender<VoiceFrame>, decoder: Arc<VoiceDecoder>) -> Self {
        AudioManager {
            voice_input_tx,
            decoder,
            input_stream: Arc::new(Mutex::new(None)),
            input_receiver_task: Arc::new(Mutex::new(None)),
            output_handle: Arc::new(Mutex::new(None)),
            output_receiver_task: Arc::new(Mutex::new(None)),
        }
    }

    /// Start recording: create/resume audio stream and forward frames to SDK
    pub fn start_recording(&self) -> Result<(), Box<dyn std::error::Error>> {
        info!("Starting audio recording");

        // Create the input stream
        let (stream, sample_rate, receiver) = create_input_stream()?;

        // Spawn a task to read from the receiver and forward to SDK
        let voice_input_tx = self.voice_input_tx.clone();
        let task = tokio::spawn(async move {
            let mut rx = receiver;
            while let Some(frame) = rx.recv().await {
                if let Err(e) = voice_input_tx.send(VoiceFrame::new(sample_rate.0 as usize, frame)).await {
                    error!("Failed to send audio frame to SDK: {}", e);
                    break;
                }
            }
            debug!("Audio receiver task ended");
        });

        // Store the stream and task
        *self.input_stream.lock().unwrap() = Some(stream);
        *self.input_receiver_task.lock().unwrap() = Some(task);

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