use cpal::Stream;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tracing::{debug, error, info};
use std::sync::{Arc, Mutex};

use crate::audio::create_input_stream;

/// Audio manager that handles recording lifecycle and voice data transmission
pub struct AudioManager {
    voice_input_tx: mpsc::UnboundedSender<Vec<f32>>,
    stream: Arc<Mutex<Option<Stream>>>,
    receiver_task: Arc<Mutex<Option<JoinHandle<()>>>>,
}

impl AudioManager {
    /// Create a new AudioManager with a voice input sender
    pub fn new(voice_input_tx: mpsc::UnboundedSender<Vec<f32>>) -> Self {
        AudioManager {
            voice_input_tx,
            stream: Arc::new(Mutex::new(None)),
            receiver_task: Arc::new(Mutex::new(None)),
        }
    }

    /// Start recording: create/resume audio stream and forward frames to SDK
    pub async fn start_recording(&self) -> Result<(), Box<dyn std::error::Error>> {
        info!("Starting audio recording");

        // Create the input stream
        let (stream, receiver) = create_input_stream()?;

        // Spawn a task to read from the receiver and forward to SDK
        let voice_input_tx = self.voice_input_tx.clone();
        let task = tokio::spawn(async move {
            let mut rx = receiver;
            while let Some(frame) = rx.recv().await {
                if let Err(e) = voice_input_tx.send(frame) {
                    error!("Failed to send audio frame to SDK: {}", e);
                    break;
                }
            }
            debug!("Audio receiver task ended");
        });

        // Store the stream and task
        *self.stream.lock().unwrap() = Some(stream);
        *self.receiver_task.lock().unwrap() = Some(task);

        info!("Audio recording started");
        Ok(())
    }

    /// Stop recording: close stream and stop sending voice data
    pub async fn stop_recording(&self) {
        info!("Stopping audio recording");

        // Drop the stream (this stops the stream and closes the receiver)
        if let Ok(mut stream_opt) = self.stream.lock() {
            *stream_opt = None;
        }

        // Abort the receiver task
        if let Ok(mut task_opt) = self.receiver_task.lock() {
            if let Some(task) = task_opt.take() {
                task.abort();
            }
        }

        info!("Audio recording stopped");
    }
}