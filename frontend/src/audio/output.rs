use std::collections::HashMap;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{BufferSize, Device, DeviceId, SampleFormat, SampleRate, Stream, StreamConfig};
use std::sync::Arc;
use tracing::{error, warn};
use crate::audio::audio_source::AudioSource;
use crate::audio::common::{find_best_stream_config, find_device_by_id, list_devices_by_id};
use crate::audio::{mono_to_multichannel_f32, mono_to_multichannel_i16, mono_to_multichannel_u16};
use crate::config::AudioDevice;

pub fn list_output_devices() -> Result<HashMap<String, String>, Box<dyn std::error::Error>> {
    list_devices_by_id(cpal::default_host().output_devices()?)
}

pub fn find_best_output_stream_config(
    device: &Device,
) -> Result<(SampleRate, SampleFormat, u16), Box<dyn std::error::Error>> {
    let configs: Vec<_> = device.supported_output_configs()?.collect();
    find_best_stream_config(configs)
}

/// Create output stream for playing back audio from an audio source
pub fn create_output_stream(
    device_config: AudioDevice,
    audio_source: Arc<dyn AudioSource>,
) -> Result<Stream, Box<dyn std::error::Error>> {
    let device = find_device_by_id(device_config.device_id.clone())?;

    let stream_config = StreamConfig {
        channels: device_config.channels,
        sample_rate: device_config.sample_rate,
        buffer_size: BufferSize::Default
    };

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
                    let frame_count = output.len() / channels;
                    let mut mono_buffer = vec![0.0f32; frame_count];
                    fill_output(&mut mono_buffer, &audio_source_clone, &mut leftover);
                    mono_to_multichannel_f32(&mono_buffer, output, channels);
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
                    let frame_count = output.len() / channels;
                    let mut mono_buffer = vec![0.0f32; frame_count];
                    fill_output(&mut mono_buffer, &audio_source_clone, &mut leftover);
                    mono_to_multichannel_i16(&mono_buffer, output, channels);
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
                    let frame_count = output.len() / channels;
                    let mut mono_buffer = vec![0.0f32; frame_count];
                    fill_output(&mut mono_buffer, &audio_source_clone, &mut leftover);
                    mono_to_multichannel_u16(&mono_buffer, output, channels);
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

    Ok(stream)
}

fn fill_output(buffer: &mut [f32], audio_source: &Arc<dyn AudioSource>, leftover: &mut Vec<f32>) {
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

        // Copy samples from leftover to buffer
        for i in 0..n {
            buffer[idx + i] = leftover[i];
        }

        leftover.drain(..n);
        idx += n;
    }
}
