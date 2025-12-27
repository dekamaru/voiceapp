use std::collections::HashMap;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{BufferSize, Device, SampleFormat, SampleRate, Stream, StreamConfig};
use tokio::sync::mpsc;
use tracing::{error};
use crate::audio::common::{find_best_stream_config, find_device_by_id, list_devices_by_id, stereo_to_mono};
use crate::config::AudioDevice;

pub fn list_input_devices() -> Result<HashMap<String, String>, Box<dyn std::error::Error>> {
    list_devices_by_id(cpal::default_host().input_devices()?)
}

pub fn find_best_input_stream_config(
    device: &Device,
) -> Result<(SampleRate, SampleFormat, u16), Box<dyn std::error::Error>> {
    let configs: Vec<_> = device.supported_input_configs()?.collect();
    find_best_stream_config(configs)
}

/// Create input stream that captures audio and sends frames through channel
/// Returns (stream, receiver)
pub fn create_input_stream(device_config: AudioDevice) -> Result<(Stream, mpsc::UnboundedReceiver<Vec<f32>>), Box<dyn std::error::Error>> {
    let device = find_device_by_id(device_config.device_id.clone())?;

    let stream_config = StreamConfig {
        channels: device_config.channels,
        sample_rate: device_config.sample_rate,
        buffer_size: BufferSize::Default
    };

    let (tx, rx) = mpsc::unbounded_channel();

    // Match on sample format to build the appropriate stream
    let stream = match device_config.sample_format.as_str() {
        "f32" => device.build_input_stream(
            &stream_config,
            move |data: &[f32], _: &cpal::InputCallbackInfo| {
                let mono_samples = stereo_to_mono(data, device_config.channels);
                let _ = tx.send(mono_samples);
            },
            move |err| {
                error!("Input stream error: {}", err);
            },
            None,
        )?,
        "i16" => {
            device.build_input_stream(
                &stream_config,
                move |data: &[i16], _: &cpal::InputCallbackInfo| {
                    // Convert i16 to f32 in [-1.0, 1.0] range
                    let f32_data: Vec<f32> = data.iter().map(|&s| s as f32 / 32768.0).collect();
                    let mono_samples = stereo_to_mono(&f32_data, device_config.channels);
                    let _ = tx.send(mono_samples);
                },
                move |err| {
                    error!("Input stream error: {}", err);
                },
                None,
            )?
        }
        "u16" => {
            device.build_input_stream(
                &stream_config,
                move |data: &[u16], _: &cpal::InputCallbackInfo| {
                    // Convert u16 to f32 in [0.0, 1.0] range, then to [-1.0, 1.0]
                    let f32_data: Vec<f32> =
                        data.iter().map(|&s| (s as f32 / 32768.0) - 1.0).collect();
                    let mono_samples = stereo_to_mono(&f32_data, device_config.channels);
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

    Ok((stream, rx))
}
