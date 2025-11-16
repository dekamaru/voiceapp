use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{Device, Stream, StreamConfig, SampleRate, SampleFormat};
use std::sync::mpsc;
use tracing::{debug, error, info};
use std::sync::{Arc, Mutex};
use std::collections::VecDeque;

const TARGET_SAMPLE_RATE: u32 = 48000;
const OUTPUT_BUFFER_CAPACITY: usize = 48000; // ~500ms at 48kHz for continuous sample buffer

/// Audio frame for playback: stereo F32 samples at 48kHz
pub type PlaybackFrame = Vec<f32>;

/// Handle to manage audio output stream for a single user
pub struct AudioOutputHandle {
    _stream: Stream, // kept alive to maintain audio stream
    sender: mpsc::Sender<PlaybackFrame>,
}

impl AudioOutputHandle {
    /// Get sender to queue audio frames for playback
    pub fn sender(&self) -> mpsc::Sender<PlaybackFrame> {
        self.sender.clone()
    }
}

/// Find and validate output device
fn find_output_device() -> Result<Device, Box<dyn std::error::Error>> {
    let host = cpal::default_host();
    let device = host
        .default_output_device()
        .ok_or("No output device found")?;

    info!("Selected output device: {}", device.name()?);

    // Check supported configs
    let configs = device.supported_output_configs()?;
    let mut found_valid = false;

    for config in configs {
        if config.min_sample_rate() <= SampleRate(TARGET_SAMPLE_RATE)
            && config.max_sample_rate() >= SampleRate(TARGET_SAMPLE_RATE)
        {
            found_valid = true;
            debug!(
                "Found config: {} channels, {}-{} Hz, {:?}",
                config.channels(),
                config.min_sample_rate().0,
                config.max_sample_rate().0,
                config.sample_format()
            );
        }
    }

    if !found_valid {
        return Err(
            format!(
                "Device does not support {} Hz sample rate",
                TARGET_SAMPLE_RATE
            )
            .into(),
        );
    }

    Ok(device)
}

/// Get output stream config: prefer F32, fall back to any format that supports 48kHz
/// Returns (StreamConfig, SampleFormat)
fn get_stream_config(device: &Device) -> Result<(StreamConfig, SampleFormat), Box<dyn std::error::Error>> {
    let configs: Vec<_> = device.supported_output_configs()?.collect();

    // Try to find F32 config at 48kHz first
    for config in &configs {
        if config.sample_format() == SampleFormat::F32
            && config.min_sample_rate() <= SampleRate(TARGET_SAMPLE_RATE)
            && config.max_sample_rate() >= SampleRate(TARGET_SAMPLE_RATE)
        {
            info!("Selected F32 format for output");
            let stream_config: StreamConfig = config.with_sample_rate(SampleRate(TARGET_SAMPLE_RATE)).into();
            return Ok((stream_config, SampleFormat::F32));
        }
    }

    // Fall back to any other format at 48kHz
    for config in &configs {
        if config.min_sample_rate() <= SampleRate(TARGET_SAMPLE_RATE)
            && config.max_sample_rate() >= SampleRate(TARGET_SAMPLE_RATE)
        {
            let format = config.sample_format();
            info!("F32 not available, falling back to {:?} format", format);
            let stream_config: StreamConfig = config.with_sample_rate(SampleRate(TARGET_SAMPLE_RATE)).into();
            return Ok((stream_config, format));
        }
    }

    Err("Device does not support 48kHz sample rate".into())
}

/// Create output stream for playing back audio from a specific user
pub fn create_output_stream() -> Result<AudioOutputHandle, Box<dyn std::error::Error>> {
    debug!("Creating output stream...");
    let device = find_output_device()?;
    let (config, format) = get_stream_config(&device)?;
    let channels = config.channels;

    info!(
        "Output stream config: {} channels, {} Hz, format: {:?}",
        config.channels, config.sample_rate.0, format
    );

    let (tx, rx) = mpsc::channel::<PlaybackFrame>();

    // Use a continuous sample buffer (VecDeque) instead of frame-based delivery
    // This handles variable cpal callback sizes and prevents timing misalignment
    let sample_buffer = Arc::new(Mutex::new(VecDeque::<f32>::with_capacity(OUTPUT_BUFFER_CAPACITY)));

    // Spawn task to receive frames and feed them into the sample buffer
    // Convert mono to stereo if needed based on device channel count
    let sample_buffer_clone = sample_buffer.clone();
    tokio::spawn(async move {
        while let Ok(frame) = rx.recv() {
            let mut buffer = sample_buffer_clone.lock().unwrap();

            // Convert mono to stereo if device has 2 channels
            let audio_data = if channels == 2 {
                mono_to_stereo(&frame)
            } else {
                frame
            };

            buffer.extend(audio_data.iter().cloned());

            // Log if buffer is getting full
            if buffer.len() > OUTPUT_BUFFER_CAPACITY * 90 / 100 {
                debug!("Output buffer near capacity: {} samples", buffer.len());
            }
        }
    });

    // Build stream matching the format
    let stream = match format {
        SampleFormat::F32 => {
            let sample_buffer = sample_buffer.clone();
            device.build_output_stream(
                &config,
                move |output: &mut [f32], _: &cpal::OutputCallbackInfo| {
                    let mut buffer = sample_buffer.lock().unwrap();

                    // Fill output buffer sample by sample from the VecDeque
                    for sample in output.iter_mut() {
                        if let Some(s) = buffer.pop_front() {
                            *sample = s;
                        } else {
                            // No more samples available, fill with silence
                            *sample = 0.0;
                        }
                    }
                },
                move |err| {
                    error!("Output stream error: {}", err);
                },
                None,
            )?
        }
        SampleFormat::I16 => {
            let sample_buffer = sample_buffer.clone();
            device.build_output_stream(
                &config,
                move |output: &mut [i16], _: &cpal::OutputCallbackInfo| {
                    let mut buffer = sample_buffer.lock().unwrap();

                    // Fill output buffer sample by sample from the VecDeque
                    let mut filled = 0;
                    for sample in output.iter_mut() {
                        if let Some(s) = buffer.pop_front() {
                            *sample = (s * 32767.0).clamp(-32768.0, 32767.0) as i16;
                            filled += 1;
                        } else {
                            *sample = 0;
                        }
                    }

                    if filled < output.len() {
                        debug!("Output callback: buffer underrun, filled {}/{} samples", filled, output.len());
                    }
                },
                move |err| {
                    error!("Output stream error: {}", err);
                },
                None,
            )?
        }
        SampleFormat::U16 => {
            let sample_buffer = sample_buffer.clone();
            device.build_output_stream(
                &config,
                move |output: &mut [u16], _: &cpal::OutputCallbackInfo| {
                    let mut buffer = sample_buffer.lock().unwrap();

                    // Fill output buffer sample by sample from the VecDeque
                    let mut filled = 0;
                    for sample in output.iter_mut() {
                        if let Some(s) = buffer.pop_front() {
                            let val = ((s + 1.0) * 32768.0).clamp(0.0, 65535.0) as u16;
                            *sample = val;
                            filled += 1;
                        } else {
                            *sample = 32768;
                        }
                    }

                    if filled < output.len() {
                        debug!("Output callback: buffer underrun, filled {}/{} samples", filled, output.len());
                    }
                },
                move |err| {
                    error!("Output stream error: {}", err);
                },
                None,
            )?
        }
        other => {
            return Err(format!("Unsupported output format: {:?}", other).into());
        }
    };

    // Start the stream
    stream.play()?;
    debug!("Output stream started with continuous sample buffer");

    Ok(AudioOutputHandle {
        _stream: stream,
        sender: tx,
    })
}

/// Convert mono audio to stereo by duplicating channels
fn mono_to_stereo(mono: &[f32]) -> Vec<f32> {
    let mut stereo = Vec::with_capacity(mono.len() * 2);
    for &sample in mono {
        stereo.push(sample);
        stereo.push(sample); // Duplicate to stereo
    }
    stereo
}
