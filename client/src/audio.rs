use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{Device, Stream, StreamConfig, SampleRate, SampleFormat};
use tokio::sync::mpsc;
use tracing::{debug, error, info, warn};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

const TARGET_SAMPLE_RATE: u32 = 48000;
const AUDIO_BUFFER_CAPACITY: usize = 48000; // ~1000ms at 48kHz - larger buffer to prevent callback blocking

/// Audio frame: mono F32 samples at 48kHz
pub type AudioFrame = Vec<f32>;

/// Find and validate input device
fn find_input_device() -> Result<Device, Box<dyn std::error::Error>> {
    let host = cpal::default_host();
    let device = host
        .default_input_device()
        .ok_or("No input device found")?;

    info!("Selected input device: {}", device.name()?);

    // Check supported configs
    let configs = device.supported_input_configs()?;
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

/// Get input stream config: prefer F32, fall back to any format that supports 48kHz
/// Returns (StreamConfig, SampleFormat)
fn get_stream_config(device: &Device) -> Result<(StreamConfig, SampleFormat), Box<dyn std::error::Error>> {
    let configs: Vec<_> = device.supported_input_configs()?.collect();

    // Try to find F32 config at 48kHz first
    for config in &configs {
        if config.sample_format() == SampleFormat::F32
            && config.min_sample_rate() <= SampleRate(TARGET_SAMPLE_RATE)
            && config.max_sample_rate() >= SampleRate(TARGET_SAMPLE_RATE)
        {
            info!("Selected F32 format");
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

/// Convert stereo to mono by averaging channels (F32)
fn stereo_to_mono_f32(stereo: &[f32], channels: u16) -> Vec<f32> {
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
/// Returns the stream (to keep it alive) and a receiver for audio frames
pub fn create_input_stream() -> Result<(Stream, mpsc::Receiver<AudioFrame>), Box<dyn std::error::Error>> {
    let device = find_input_device()?;
    let (config, format) = get_stream_config(&device)?;

    info!(
        "Input stream config: {} channels, {} Hz, format: {:?}",
        config.channels, config.sample_rate.0, format
    );

    let (tx, rx) = mpsc::channel::<AudioFrame>(AUDIO_BUFFER_CAPACITY / 480); // ~100 frames buffer at 48kHz

    // Track dropped frames to detect callback blockage
    let dropped_frames = Arc::new(AtomicUsize::new(0));

    let channels = config.channels;

    // Match on sample format to build the appropriate stream
    let stream = match format {
        SampleFormat::F32 => {
            let dropped_frames = dropped_frames.clone();
            device.build_input_stream(
                &config,
                move |data: &[f32], _: &cpal::InputCallbackInfo| {
                    let mono_samples = stereo_to_mono_f32(data, channels);
                    // Use try_send() to NEVER block the audio callback
                    // Dropping frames is better than blocking the audio system
                    if let Err(_) = tx.try_send(mono_samples) {
                        let dropped = dropped_frames.fetch_add(1, Ordering::Relaxed);
                        if dropped % 100 == 0 {
                            warn!("Input buffer full, dropped audio frames (count={})", dropped + 1);
                        }
                    }
                },
                move |err| {
                    error!("Input stream error: {}", err);
                },
                None,
            )?
        }
        SampleFormat::I16 => {
            let dropped_frames = dropped_frames.clone();
            device.build_input_stream(
                &config,
                move |data: &[i16], _: &cpal::InputCallbackInfo| {
                    // Convert i16 to f32 in [-1.0, 1.0] range
                    let f32_data: Vec<f32> = data.iter().map(|&s| s as f32 / 32768.0).collect();
                    let mono_samples = stereo_to_mono_f32(&f32_data, channels);
                    // Use try_send() to NEVER block the audio callback
                    if let Err(_) = tx.try_send(mono_samples) {
                        let dropped = dropped_frames.fetch_add(1, Ordering::Relaxed);
                        if dropped % 100 == 0 {
                            warn!("Input buffer full, dropped audio frames (count={})", dropped + 1);
                        }
                    }
                },
                move |err| {
                    error!("Input stream error: {}", err);
                },
                None,
            )?
        }
        SampleFormat::U16 => {
            let dropped_frames = dropped_frames.clone();
            device.build_input_stream(
                &config,
                move |data: &[u16], _: &cpal::InputCallbackInfo| {
                    // Convert u16 to f32 in [0.0, 1.0] range, then to [-1.0, 1.0]
                    let f32_data: Vec<f32> = data
                        .iter()
                        .map(|&s| (s as f32 / 32768.0) - 1.0)
                        .collect();
                    let mono_samples = stereo_to_mono_f32(&f32_data, channels);
                    // Use try_send() to NEVER block the audio callback
                    if let Err(_) = tx.try_send(mono_samples) {
                        let dropped = dropped_frames.fetch_add(1, Ordering::Relaxed);
                        if dropped % 100 == 0 {
                            warn!("Input buffer full, dropped audio frames (count={})", dropped + 1);
                        }
                    }
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

    Ok((stream, rx))
}
