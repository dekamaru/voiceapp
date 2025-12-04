use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{Device, Stream, StreamConfig, SampleRate, SampleFormat};
use tokio::sync::mpsc;
use tracing::{debug, error, info, warn};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

const TARGET_SAMPLE_RATE: u32 = 48000;

/// Audio frame: mono F32 samples
pub type AudioFrame = Vec<f32>;

/// Find and validate input device
fn find_input_device() -> Result<Device, Box<dyn std::error::Error>> {
    let host = cpal::default_host();
    let device = host
        .default_input_device()
        .ok_or("No input device found")?;

    info!("Selected input device: {}", device.name()?);

    Ok(device)
}

/// Get input stream config: prefer F32, fall back to any format that supports 48kHz
/// Returns (StreamConfig, SampleFormat)
fn get_stream_config(device: &Device) -> Result<(StreamConfig, SampleFormat), Box<dyn std::error::Error>> {
    let configs: Vec<_> = device.supported_input_configs()?.collect();

    // Try to find F32 config at 48kHz first
    for config in &configs {
        if config.sample_format() == SampleFormat::F32 && config.max_sample_rate() >= SampleRate(TARGET_SAMPLE_RATE) {
            info!("Selected F32 format");
            let stream_config: StreamConfig = config.with_sample_rate(SampleRate(TARGET_SAMPLE_RATE)).into();
            return Ok((stream_config, SampleFormat::F32));
        }
    }

    // Fall back to any other format at 48kHz
    for config in &configs {
        if config.max_sample_rate() >= SampleRate(TARGET_SAMPLE_RATE) {
            let format = config.sample_format();
            info!("F32 not available, falling back to {:?} format", format);
            let stream_config: StreamConfig = config.with_sample_rate(SampleRate(TARGET_SAMPLE_RATE)).into();
            return Ok((stream_config, format));
        }
    }

    // Fall back to maximum sample rate with best available format
    if let Some(config) = configs.into_iter().max_by_key(|c| c.max_sample_rate().0) {
        let format = config.sample_format();
        let max_rate = config.max_sample_rate();
        warn!("48kHz not available, using maximum sample rate {:?} with {:?} format", max_rate, format);
        let stream_config: StreamConfig = config.with_sample_rate(max_rate).into();
        return Ok((stream_config, format));
    }

    Err("No suitable input configuration found".into())
}

/// Convert stereo to mono by averaging channels (F32)
fn stereo_to_mono(stereo: &[f32], channels: u16) -> Vec<f32> {
    if channels == 1 {
        return stereo.to_vec();
    }

    let frame_count = stereo.len() / channels as usize;
    let mut mono = Vec::with_capacity(frame_count);

    for frame_idx in 0..frame_count {
        let mut sum = 0.0f32;
        for ch in 0..channels as usize {
            sum += stereo[frame_idx * channels as usize + ch];
        }
        mono.push(sum / channels as f32);
    }

    mono
}

/// Create input stream that captures audio and sends frames through channel
/// Returns (stream, actual_sample_rate, receiver)
pub fn create_input_stream() -> Result<(Stream, u32, mpsc::Receiver<AudioFrame>), Box<dyn std::error::Error>> {
    let device = find_input_device()?;
    let (config, format) = get_stream_config(&device)?;

    let sample_rate = config.sample_rate.0;

    info!(
        "Input stream config: {} channels, {} Hz, format: {:?}",
        config.channels, config.sample_rate.0, format
    );

    let (tx, rx) = mpsc::channel::<AudioFrame>(100);

    let channels = config.channels;

    // Match on sample format to build the appropriate stream
    let stream = match format {
        SampleFormat::F32 => {
            device.build_input_stream(
                &config,
                move |data: &[f32], _: &cpal::InputCallbackInfo| {
                    let mono_samples = stereo_to_mono(data, channels);
                    let _ = tx.try_send(mono_samples);
                },
                move |err| {
                    error!("Input stream error: {}", err);
                },
                None,
            )?
        }
        SampleFormat::I16 => {
            device.build_input_stream(
                &config,
                move |data: &[i16], _: &cpal::InputCallbackInfo| {
                    // Convert i16 to f32 in [-1.0, 1.0] range
                    let f32_data: Vec<f32> = data.iter().map(|&s| s as f32 / 32768.0).collect();
                    let mono_samples = stereo_to_mono(&f32_data, channels);
                    // Use try_send() to NEVER block the audio callback
                    let _ = tx.try_send(mono_samples);
                },
                move |err| {
                    error!("Input stream error: {}", err);
                },
                None,
            )?
        }
        SampleFormat::U16 => {
            device.build_input_stream(
                &config,
                move |data: &[u16], _: &cpal::InputCallbackInfo| {
                    // Convert u16 to f32 in [0.0, 1.0] range, then to [-1.0, 1.0]
                    let f32_data: Vec<f32> = data
                        .iter()
                        .map(|&s| (s as f32 / 32768.0) - 1.0)
                        .collect();
                    let mono_samples = stereo_to_mono(&f32_data, channels);
                    // Use try_send() to NEVER block the audio callback
                    let _ = tx.try_send(mono_samples);
                },
                move |err| {
                    error!("Input stream error: {}", err);
                },
                None,
            )?
        }
        other => {
            return Err(format!("Unsupported sample format: {:?}", other).into());
        }
    };

    // Start the stream
    stream.play()?;
    debug!("Input stream started");

    Ok((stream, sample_rate, rx))
}
