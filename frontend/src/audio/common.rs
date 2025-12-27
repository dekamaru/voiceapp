use std::collections::HashMap;
use std::str::FromStr;
use cpal::traits::{DeviceTrait, HostTrait};
use cpal::{Device, DeviceId, Devices, DevicesFiltered, SampleFormat, SampleRate, SupportedStreamConfigRange};

const TARGET_SAMPLE_RATE: u32 = 48000;

/// Find best stream config with prioritization:
/// 1. 48000 Hz + F32
/// 2. 48000 Hz + I16
/// 3. 48000 Hz + U16
/// 4. Any Hz + F32
/// 5. First available config
/// Returns (SampleRate, SampleFormat, channels)
pub fn find_best_stream_config(
    configs: Vec<SupportedStreamConfigRange>,
) -> Result<(SampleRate, SampleFormat, u16), Box<dyn std::error::Error>> {
    if configs.is_empty() {
        return Err("No configurations found".into());
    }

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

/// Convert stereo to mono by averaging channels (F32)
pub fn stereo_to_mono(stereo: &[f32], channels: u16) -> Vec<f32> {
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

pub fn adjust_volume(buffer: &mut [f32], volume: f32) {
    for sample in buffer.iter_mut() {
        *sample *= volume;
    }
}

pub fn mono_to_multichannel_f32(mono_buffer: &[f32], output: &mut [f32], channels: usize) {
    let frame_count = output.len() / channels;
    for i in 0..frame_count {
        for ch in 0..channels {
            output[i * channels + ch] = mono_buffer[i];
        }
    }
}

pub fn mono_to_multichannel_i16(mono_buffer: &[f32], output: &mut [i16], channels: usize) {
    let frame_count = output.len() / channels;
    for i in 0..frame_count {
        let sample = (mono_buffer[i].clamp(-1.0, 1.0) * 32767.0) as i16;
        for ch in 0..channels {
            output[i * channels + ch] = sample;
        }
    }
}

pub fn mono_to_multichannel_u16(mono_buffer: &[f32], output: &mut [u16], channels: usize) {
    let frame_count = output.len() / channels;
    for i in 0..frame_count {
        let sample = ((mono_buffer[i].clamp(-1.0, 1.0) * 0.5 + 0.5) * u16::MAX as f32) as u16;
        for ch in 0..channels {
            output[i * channels + ch] = sample;
        }
    }
}

pub fn list_devices_by_id(devices: DevicesFiltered<Devices>) -> Result<HashMap<String, String>, Box<dyn std::error::Error>> {
    let mut result = HashMap::new();
    for device in devices {
        let id = device.id()?.to_string();
        let name = device.description()?.name().to_string();
        result.insert(id, name);
    }

    Ok(result)
}

pub fn find_device_by_id(id: String) -> Result<Device, Box<dyn std::error::Error>> {
    let parsed = DeviceId::from_str(&id)?;
    let host = cpal::default_host();

    if let Some(device) = host.device_by_id(&parsed) {
        Ok(device)
    } else {
        Err(format!("Input device '{}' not found", &id).into())
    }
}
