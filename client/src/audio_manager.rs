use cpal::Stream;
use tokio::sync::{mpsc, broadcast};
use tokio::task::JoinHandle;
use tracing::{debug, error, info};
use std::sync::{Arc, Mutex};

use crate::audio::create_input_stream;
use crate::output::{create_output_stream, AudioOutputHandle};

/// Audio manager that handles recording and playback lifecycle
pub struct AudioManager {
    voice_input_tx: mpsc::UnboundedSender<Vec<f32>>,
    voice_output_rx: broadcast::Receiver<Vec<f32>>,
    input_stream: Arc<Mutex<Option<Stream>>>,
    input_receiver_task: Arc<Mutex<Option<JoinHandle<()>>>>,
    output_handle: Arc<Mutex<Option<AudioOutputHandle>>>,
    output_receiver_task: Arc<Mutex<Option<JoinHandle<()>>>>,
}

impl AudioManager {
    /// Create a new AudioManager with voice input sender and output receiver
    pub fn new(voice_input_tx: mpsc::UnboundedSender<Vec<f32>>, voice_output_rx: broadcast::Receiver<Vec<f32>>) -> Self {
        AudioManager {
            voice_input_tx,
            voice_output_rx,
            input_stream: Arc::new(Mutex::new(None)),
            input_receiver_task: Arc::new(Mutex::new(None)),
            output_handle: Arc::new(Mutex::new(None)),
            output_receiver_task: Arc::new(Mutex::new(None)),
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
        *self.input_stream.lock().unwrap() = Some(stream);
        *self.input_receiver_task.lock().unwrap() = Some(task);

        info!("Audio recording started");
        Ok(())
    }

    /// Stop recording: close stream and stop sending voice data
    pub async fn stop_recording(&self) {
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
    pub async fn start_playback(&self) -> Result<(), Box<dyn std::error::Error>> {
        info!("Starting audio playback");

        // Create the output stream
        let output_handle = create_output_stream()?;
        let output_sender = output_handle.sender();

        // Spawn a task to read from voice output and send to playback
        // Note: The output stream will handle mono-to-stereo conversion if the device is 2-channel
        let voice_output_rx = self.voice_output_rx.resubscribe();
        let task = tokio::spawn(async move {
            let mut rx = voice_output_rx;
            loop {
                match rx.recv().await {
                    Ok(pcm_samples) => {
                        if let Err(e) = output_sender.send(pcm_samples) {
                            error!("Failed to send audio to playback: {}", e);
                            break;
                        }
                    }
                    Err(broadcast::error::RecvError::Closed) => {
                        debug!("Voice output stream closed");
                        break;
                    }
                    Err(broadcast::error::RecvError::Lagged(_)) => {
                        debug!("Voice output stream lagged, skipping frames");
                    }
                }
            }
            debug!("Audio playback task ended");
        });

        // Store the output handle to keep the stream alive and the task
        *self.output_handle.lock().unwrap() = Some(output_handle);
        *self.output_receiver_task.lock().unwrap() = Some(task);

        info!("Audio playback started");
        Ok(())
    }

    /// Stop playing audio output
    pub async fn stop_playback(&self) {
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

        info!("Audio playback stopped");
    }
}