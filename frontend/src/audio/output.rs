use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{Device, SampleFormat, SampleRate, Stream, StreamConfig};
use std::sync::Arc;
use tracing::{debug, error, info, warn};
use voiceapp_sdk::VoiceDecoder;

/// Handle to manage audio output stream for a single user
pub struct AudioOutputHandle {
    _stream: Stream, // kept alive to maintain audio stream
}

/// Find output device
fn find_output_device() -> Result<Device, Box<dyn std::error::Error>> {
    let host = cpal::default_host();
    let device = host
        .default_output_device()
        .ok_or("No output device found")?;

    info!("Selected output device: {}", device.name()?);

    // Log supported configs for debugging
    #[cfg(debug_assertions)]
    {
        let configs = device.supported_output_configs()?;
        for config in configs {
            debug!(
                "Supported: {} ch, {}-{} Hz, {:?}",
                config.channels(),
                config.min_sample_rate().0,
                config.max_sample_rate().0,
                config.sample_format()
            );
        }
    }

    Ok(device)
}

/// Find best stream config with prioritization:
/// 1. 48000 Hz + F32
/// 2. 48000 Hz + I16
/// 3. 48000 Hz + U16
/// 4. Any Hz + F32
/// 5. First available config
/// Returns (SampleRate, SampleFormat, ChannelsCount)
pub fn find_best_output_stream_config(
    device: &Device,
) -> Result<(SampleRate, SampleFormat, u16), Box<dyn std::error::Error>> {
    let configs: Vec<_> = device.supported_output_configs()?.collect();

    if configs.is_empty() {
        return Err("No output configurations found".into());
    }

    const TARGET_SAMPLE_RATE: u32 = 48000;

    // Priority list: (target_rate, format) where None means any rate
    let priorities = [
        (Some(TARGET_SAMPLE_RATE), SampleFormat::F32),
        (Some(TARGET_SAMPLE_RATE), SampleFormat::I16),
        (Some(TARGET_SAMPLE_RATE), SampleFormat::U16),
        (None, SampleFormat::F32),
    ];

    for (target_rate, format) in priorities {
        for config in &configs {
            if config.sample_format() == format {
                if let Some(rate) = target_rate {
                    if config.min_sample_rate() <= SampleRate(rate)
                        && config.max_sample_rate() >= SampleRate(rate)
                    {
                        return Ok((SampleRate(rate), format, config.channels()));
                    }
                } else {
                    return Ok((config.min_sample_rate(), format, config.channels()));
                }
            }
        }
    }

    // Fallback: first available config
    let first_config = &configs[0];
    Ok((first_config.min_sample_rate(), first_config.sample_format(), first_config.channels()))
}

/// Get output stream config using device's native sample rate
/// Returns (StreamConfig, SampleFormat)
fn get_stream_config(
    device: &Device,
) -> Result<(StreamConfig, SampleFormat), Box<dyn std::error::Error>> {
    let (sample_rate, format, _) = find_best_output_stream_config(device)?;

    info!("Selected output config: {} Hz, {:?}", sample_rate.0, format);

    // Find the matching config and create StreamConfig
    let configs: Vec<_> = device.supported_output_configs()?.collect();
    for config in configs {
        if config.sample_format() == format
            && config.min_sample_rate() <= sample_rate
            && config.max_sample_rate() >= sample_rate
        {
            let stream_config: StreamConfig = config.with_sample_rate(sample_rate).into();
            return Ok((stream_config, format));
        }
    }

    Err("Failed to create stream config from selected parameters".into())
}

/// Create output stream for playing back audio from a specific user
/// Returns (AudioOutputHandle, sample_rate)
pub fn create_output_stream(
    decoder: Arc<VoiceDecoder>,
) -> Result<(AudioOutputHandle, u32), Box<dyn std::error::Error>> {
    let device = find_output_device()?;
    let (config, format) = get_stream_config(&device)?;
    let sample_rate = config.sample_rate.0;

    info!(
        "Output stream config: {} channels, {} Hz, format: {:?}",
        config.channels, sample_rate, format
    );

    let volume = 1.0f32;
    let err_fn = |e| error!("Stream error: {}", e);
    let channels = config.channels as usize;

    // Build stream matching the format
    let stream = match format {
        SampleFormat::F32 => {
            let mut leftover: Vec<f32> = Vec::new();
            let decoder_clone = decoder.clone();
            device.build_output_stream(
                &config,
                move |output: &mut [f32], _: &cpal::OutputCallbackInfo| {
                    // Convert mono to stereo/multi-channel
                    let frame_count = output.len() / channels;
                    let mut mono_buffer = vec![0.0f32; frame_count];
                    fill_output(&mut mono_buffer, &decoder_clone, &mut leftover, volume);
                    for i in 0..frame_count {
                        for ch in 0..channels {
                            output[i * channels + ch] = mono_buffer[i];
                        }
                    }
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
                    // Convert mono to stereo/multi-channel
                    let frame_count = output.len() / channels;
                    let mut mono_buffer = vec![0.0f32; frame_count];
                    fill_output(&mut mono_buffer, &decoder_clone, &mut leftover, volume);
                    for i in 0..frame_count {
                        let sample = (mono_buffer[i].clamp(-1.0, 1.0) * 32767.0) as i16;
                        for ch in 0..channels {
                            output[i * channels + ch] = sample;
                        }
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
                    // Convert mono to stereo/multi-channel
                    let frame_count = output.len() / channels;
                    let mut mono_buffer = vec![0.0f32; frame_count];
                    fill_output(&mut mono_buffer, &decoder_clone, &mut leftover, volume);
                    for i in 0..frame_count {
                        let sample = ((mono_buffer[i].clamp(-1.0, 1.0) * 0.5 + 0.5) * u16::MAX as f32) as u16;
                        for ch in 0..channels {
                            output[i * channels + ch] = sample;
                        }
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

    Ok((AudioOutputHandle { _stream: stream }, sample_rate))
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

pub fn list_output_devices() -> Result<Vec<String>, Box<dyn std::error::Error>> {
    let host = cpal::default_host();
    let devices = host.output_devices()?;

    Ok(devices.map(|d| d.name().unwrap_or("unknown".to_string())).collect())
}
