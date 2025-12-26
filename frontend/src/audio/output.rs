use std::collections::HashMap;
use std::str::FromStr;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{BufferSize, Device, DeviceId, SampleFormat, SampleRate, Stream, StreamConfig};
use std::sync::Arc;
use tracing::{error, info, warn};
use crate::audio::audio_source::AudioSource;
use crate::config::AudioDevice;

/// Handle to manage audio output stream for a single user
pub struct AudioOutputHandle {
    _stream: Stream, // kept alive to maintain audio stream
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
                    if config.min_sample_rate() <= rate
                        && config.max_sample_rate() >= rate
                    {
                        return Ok((rate, format, config.channels()));
                    }
                } else {
                    return Ok((config.max_sample_rate(), format, config.channels()));
                }
            }
        }
    }

    // Fallback: first available config
    let first_config = &configs[0];
    Ok((first_config.max_sample_rate(), first_config.sample_format(), first_config.channels()))
}

/// Create output stream for playing back audio from an audio source
/// Returns (AudioOutputHandle, sample_rate)
pub fn create_output_stream(
    device_config: AudioDevice,
    audio_source: Arc<dyn AudioSource>,
) -> Result<AudioOutputHandle, Box<dyn std::error::Error>> {
    let device = match find_output_device_by_id(device_config.device_id.clone())? {
        Some(dev) => dev,
        None => {
            return Err(format!("Output device '{}' not found", device_config.device_id).into());
        }
    };

    let stream_config = StreamConfig {
        channels: device_config.channels,
        sample_rate: device_config.sample_rate,
        buffer_size: BufferSize::Default
    };

    let volume = 1.0f32;
    let err_fn = |e| error!("Stream error: {}", e);
    let channels = device_config.channels as usize;

    // Build stream matching the format
    let stream = match device_config.sample_format.as_str() {
        "f32" => {
            let mut leftover: Vec<f32> = Vec::new();
            let audio_source_clone = audio_source.clone();
            device.build_output_stream(
                &stream_config,
                move |output: &mut [f32], _: &cpal::OutputCallbackInfo| {
                    // Convert mono to stereo/multi-channel
                    let frame_count = output.len() / channels;
                    let mut mono_buffer = vec![0.0f32; frame_count];
                    fill_output(&mut mono_buffer, &audio_source_clone, &mut leftover, volume);
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
        "i16" => {
            let mut leftover: Vec<f32> = Vec::new();
            let audio_source_clone = audio_source.clone();
            device.build_output_stream(
                &stream_config,
                move |output: &mut [i16], _: &cpal::OutputCallbackInfo| {
                    // Convert mono to stereo/multi-channel
                    let frame_count = output.len() / channels;
                    let mut mono_buffer = vec![0.0f32; frame_count];
                    fill_output(&mut mono_buffer, &audio_source_clone, &mut leftover, volume);
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
        "u16" => {
            let mut leftover: Vec<f32> = Vec::new();
            let audio_source_clone = audio_source.clone();
            device.build_output_stream(
                &stream_config,
                move |output: &mut [u16], _: &cpal::OutputCallbackInfo| {
                    // Convert mono to stereo/multi-channel
                    let frame_count = output.len() / channels;
                    let mut mono_buffer = vec![0.0f32; frame_count];
                    fill_output(&mut mono_buffer, &audio_source_clone, &mut leftover, volume);
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

    Ok(AudioOutputHandle { _stream: stream })
}

fn fill_output(
    buffer: &mut [f32],
    audio_source: &Arc<dyn AudioSource>,
    leftover: &mut Vec<f32>,
    volume: f32,
) {
    let mut idx = 0;

    while idx < buffer.len() {
        if leftover.is_empty() {
            match audio_source.get_audio() {
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

pub fn list_output_devices() -> Result<HashMap<String, String>, Box<dyn std::error::Error>> {
    let host = cpal::default_host();
    let devices = host.output_devices()?;

    let mut result = HashMap::new();
    for device in devices {
        let id = device.id()?.to_string();
        let name = device.description()?.name().to_string();
        result.insert(id, name);
    }

    Ok(result)
}

pub fn find_output_device_by_id(id: String) -> Result<Option<Device>, Box<dyn std::error::Error>> {
    let parsed = DeviceId::from_str(&id)?;
    let host = cpal::default_host();

    if let Some(device) = host.device_by_id(&parsed) {
        Ok(Some(device))
    } else {
        Err(format!("Output device '{}' not found", &id).into())
    }
}
