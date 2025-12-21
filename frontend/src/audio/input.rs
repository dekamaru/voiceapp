use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{Device, SampleFormat, SampleRate, Stream, StreamConfig};
use tokio::sync::mpsc;
use tracing::{error, info};

const TARGET_SAMPLE_RATE: u32 = 48000;

/// Audio frame: mono F32 samples
pub type AudioFrame = Vec<f32>;

/// Find and validate input device
fn find_input_device() -> Result<Device, Box<dyn std::error::Error>> {
    let host = cpal::default_host();
    let device = host.default_input_device().ok_or("No input device found")?;

    Ok(device)
}

/// Find best stream config with prioritization:
/// 1. 48000 Hz + F32
/// 2. 48000 Hz + I16
/// 3. 48000 Hz + U16
/// 4. Any Hz + F32
/// 5. First available config
/// Returns (SampleRate, SampleFormat)
pub fn find_best_input_stream_config(
    device: &Device,
) -> Result<(SampleRate, SampleFormat, u16), Box<dyn std::error::Error>> {
    let configs: Vec<_> = device.supported_input_configs()?.collect();

    if configs.is_empty() {
        return Err("No input configurations found".into());
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

/// Get input stream config: prefer F32, fall back to any format that supports 48kHz
/// Returns (StreamConfig, SampleFormat)
fn get_stream_config(
    device: &Device,
) -> Result<(StreamConfig, SampleFormat), Box<dyn std::error::Error>> {
    let (sample_rate, format, _) = find_best_input_stream_config(device)?;

    info!("Selected input config: {} Hz, {:?}", sample_rate.0, format);

    // Find the matching config and create StreamConfig
    let configs: Vec<_> = device.supported_input_configs()?.collect();
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
pub fn create_input_stream(
) -> Result<(Stream, u32, mpsc::UnboundedReceiver<AudioFrame>), Box<dyn std::error::Error>> {
    let device = find_input_device()?;
    let (config, format) = get_stream_config(&device)?;

    let sample_rate = config.sample_rate.0;

    let (tx, rx) = mpsc::unbounded_channel();

    let channels = config.channels;

    // Match on sample format to build the appropriate stream
    let stream = match format {
        SampleFormat::F32 => device.build_input_stream(
            &config,
            move |data: &[f32], _: &cpal::InputCallbackInfo| {
                let mono_samples = stereo_to_mono(data, channels);
                let _ = tx.send(mono_samples);
            },
            move |err| {
                error!("Input stream error: {}", err);
            },
            None,
        )?,
        SampleFormat::I16 => {
            device.build_input_stream(
                &config,
                move |data: &[i16], _: &cpal::InputCallbackInfo| {
                    // Convert i16 to f32 in [-1.0, 1.0] range
                    let f32_data: Vec<f32> = data.iter().map(|&s| s as f32 / 32768.0).collect();
                    let mono_samples = stereo_to_mono(&f32_data, channels);
                    let _ = tx.send(mono_samples);
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
                    let f32_data: Vec<f32> =
                        data.iter().map(|&s| (s as f32 / 32768.0) - 1.0).collect();
                    let mono_samples = stereo_to_mono(&f32_data, channels);
                    // Use try_send() to NEVER block the audio callback
                    let _ = tx.send(mono_samples);
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

    Ok((stream, sample_rate, rx))
}

pub fn list_input_devices() -> Result<Vec<String>, Box<dyn std::error::Error>> {
    let host = cpal::default_host();
    let devices = host.input_devices()?;

    Ok(devices.map(|d| d.name().unwrap_or("unknown".to_string())).collect())
}

pub fn find_input_device_by_name(name: String) -> Result<Option<Device>, Box<dyn std::error::Error>> {
    let host = cpal::default_host();
    let mut devices = host.input_devices()?;

    Ok(devices.find(|d| d.name().unwrap_or("unknown".to_string()) == name))
}
