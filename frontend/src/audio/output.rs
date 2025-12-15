use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{Device, SampleFormat, SampleRate, Stream, StreamConfig};
use std::sync::Arc;
use tracing::{debug, error, info, warn};
use voiceapp_sdk::VoiceDecoder;

const TARGET_SAMPLE_RATE: u32 = 48000;

/// Handle to manage audio output stream for a single user
pub struct AudioOutputHandle {
    _stream: Stream, // kept alive to maintain audio stream
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
        return Err(format!(
            "Device does not support {} Hz sample rate",
            TARGET_SAMPLE_RATE
        )
        .into());
    }

    Ok(device)
}

/// Get output stream config: prefer F32, fall back to any format that supports 48kHz
/// Returns (StreamConfig, SampleFormat)
fn get_stream_config(
    device: &Device,
) -> Result<(StreamConfig, SampleFormat), Box<dyn std::error::Error>> {
    let configs: Vec<_> = device.supported_output_configs()?.collect();

    // Try to find F32 config at 48kHz first
    for config in &configs {
        if config.sample_format() == SampleFormat::F32
            && config.min_sample_rate() <= SampleRate(TARGET_SAMPLE_RATE)
            && config.max_sample_rate() >= SampleRate(TARGET_SAMPLE_RATE)
        {
            info!("Selected F32 format for output");
            let stream_config: StreamConfig = config
                .with_sample_rate(SampleRate(TARGET_SAMPLE_RATE))
                .into();
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
            let stream_config: StreamConfig = config
                .with_sample_rate(SampleRate(TARGET_SAMPLE_RATE))
                .into();
            return Ok((stream_config, format));
        }
    }

    Err("Device does not support 48kHz sample rate".into())
}

/// Create output stream for playing back audio from a specific user
pub fn create_output_stream(
    decoder: Arc<VoiceDecoder>,
) -> Result<AudioOutputHandle, Box<dyn std::error::Error>> {
    debug!("Creating output stream...");
    let device = find_output_device()?;
    let (mut config, format) = get_stream_config(&device)?;

    // Set buffer size to 10ms (480 samples at 48kHz) to match NetEQ output frame size
    const BUFFER_SIZE_MS: u32 = 10;
    let frames_per_buffer = (TARGET_SAMPLE_RATE / 1000) * BUFFER_SIZE_MS; // 480 samples
    config.buffer_size = cpal::BufferSize::Fixed(frames_per_buffer);
    config.channels = 1;

    info!(
        "Output stream config: {} channels, {} Hz, format: {:?}, buffer: {} samples ({} ms)",
        config.channels, config.sample_rate.0, format, frames_per_buffer, BUFFER_SIZE_MS
    );

    let volume = 1.0f32;
    let err_fn = |e| error!("Stream error: {}", e);

    // Build stream matching the format
    let stream = match format {
        SampleFormat::F32 => {
            let mut leftover: Vec<f32> = Vec::new();
            let decoder_clone = decoder.clone();
            device.build_output_stream(
                &config,
                move |output: &mut [f32], _: &cpal::OutputCallbackInfo| {
                    fill_output(output, &decoder_clone, &mut leftover, volume);
                },
                err_fn,
                None,
            )?
        }
        SampleFormat::I16 => {
            let mut leftover: Vec<f32> = Vec::new();
            let decoder_clone = decoder.clone();
            device.build_output_stream(
                &config,
                move |output: &mut [i16], _: &cpal::OutputCallbackInfo| {
                    let mut tmp = vec![0.0f32; output.len()];
                    fill_output(&mut tmp, &decoder_clone, &mut leftover, volume);
                    for (o, &v) in output.iter_mut().zip(tmp.iter()) {
                        *o = (v.clamp(-1.0, 1.0) * 32767.0) as i16;
                    }
                },
                err_fn,
                None,
            )?
        }
        SampleFormat::U16 => {
            let mut leftover: Vec<f32> = Vec::new();
            let decoder_clone = decoder.clone();
            device.build_output_stream(
                &config,
                move |output: &mut [u16], _: &cpal::OutputCallbackInfo| {
                    let mut tmp = vec![0.0f32; output.len()];
                    fill_output(&mut tmp, &decoder_clone, &mut leftover, volume);
                    for (o, &v) in output.iter_mut().zip(tmp.iter()) {
                        *o = ((v.clamp(-1.0, 1.0) * 0.5 + 0.5) * u16::MAX as f32) as u16;
                    }
                },
                err_fn,
                None,
            )?
        }
        other => {
            return Err(format!("Unsupported output format: {:?}", other).into());
        }
    };

    // Start the stream
    stream.play()?;
    debug!("Output stream started");

    Ok(AudioOutputHandle { _stream: stream })
}

fn fill_output(
    buffer: &mut [f32],
    decoder: &Arc<VoiceDecoder>,
    leftover: &mut Vec<f32>,
    volume: f32,
) {
    let mut idx = 0;

    while idx < buffer.len() {
        if leftover.is_empty() {
            match decoder.get_audio() {
                Ok(frame) => {
                    leftover.extend_from_slice(&frame);
                }
                Err(e) => {
                    warn!("BUFFER UNDERRUN: get_audio error: {e:?}");
                    // fill silence
                    for s in &mut buffer[idx..] {
                        *s = 0.0;
                    }
                    break;
                }
            }
        }

        let n = std::cmp::min(leftover.len(), buffer.len() - idx);
        if n == 0 {
            warn!("BUFFER UNDERRUN: No audio data available, filling with silence");
            for s in &mut buffer[idx..] {
                *s = 0.0;
            }
            break;
        }

        // Copy samples and apply volume scaling
        for i in 0..n {
            buffer[idx + i] = leftover[i] * volume;
        }

        leftover.drain(..n);
        idx += n;
    }
}

/// List all available output devices
/// Returns (device_names, default_device_index)
pub fn list_output_devices() -> Result<(Vec<String>, usize), Box<dyn std::error::Error>> {
    let host = cpal::default_host();

    // Get all output devices
    let devices: Vec<Device> = host.output_devices()?.collect();

    if devices.is_empty() {
        return Err("No output devices found".into());
    }

    // Get device names and log supported configs
    let mut device_names = Vec::new();
    for device in &devices {
        if let Ok(name) = device.name() {
            info!("Output device: {}", name);

            // Log all supported configs for this device
            if let Ok(configs) = device.supported_output_configs() {
                for config in configs {
                    info!(
                        "  Config: {} channels, {}-{} Hz, {:?}",
                        config.channels(),
                        config.min_sample_rate().0,
                        config.max_sample_rate().0,
                        config.sample_format()
                    );
                }
            } else {
                warn!("  Failed to query supported configs");
            }

            device_names.push(name);
        }
    }

    // Find default device index
    let default_device = host.default_output_device();
    let default_index = if let Some(default_dev) = default_device {
        if let Ok(default_name) = default_dev.name() {
            info!("Default output device: {}", default_name);
            device_names
                .iter()
                .position(|name| name == &default_name)
                .unwrap_or(0)
        } else {
            0
        }
    } else {
        0
    };

    Ok((device_names, default_index))
}
