use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{Device, Stream, StreamConfig, SampleRate, SampleFormat};
use tokio::sync::mpsc;
use tracing::{debug, error, info};

const TARGET_SAMPLE_RATE: u32 = 48000;
const OUTPUT_BUFFER_CAPACITY: usize = 9600; // ~100ms at 48kHz stereo

/// Audio frame for playback: stereo F32 samples at 48kHz
pub type PlaybackFrame = Vec<f32>;

/// Handle to manage audio output stream for a single user
pub struct AudioOutputHandle {
    stream: Stream,
    sender: mpsc::Sender<PlaybackFrame>,
}

impl AudioOutputHandle {
    /// Get sender to queue audio frames for playback
    pub fn sender(&self) -> mpsc::Sender<PlaybackFrame> {
        self.sender.clone()
    }

    /// Stop the audio stream
    pub fn stop(self) {
        drop(self.stream);
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
    let device = find_output_device()?;
    let (config, format) = get_stream_config(&device)?;

    info!(
        "Output stream config: {} channels, {} Hz, format: {:?}",
        config.channels, config.sample_rate.0, format
    );

    let (tx, mut rx) = mpsc::channel::<PlaybackFrame>(OUTPUT_BUFFER_CAPACITY / 480);

    // Build stream matching the format
    let stream = match format {
        SampleFormat::F32 => {
            device.build_output_stream(
                &config,
                move |output: &mut [f32], _: &cpal::OutputCallbackInfo| {
                    // Try to get next frame from receiver
                    let frame = match rx.try_recv() {
                        Ok(frame) => frame,
                        Err(_) => {
                            // No data available, output silence
                            for sample in output.iter_mut() {
                                *sample = 0.0;
                            }
                            return;
                        }
                    };

                    // Copy frame to output buffer
                    let copy_len = frame.len().min(output.len());
                    output[..copy_len].copy_from_slice(&frame[..copy_len]);

                    // Pad remaining with silence
                    for sample in output[copy_len..].iter_mut() {
                        *sample = 0.0;
                    }
                },
                move |err| {
                    error!("Output stream error: {}", err);
                },
                None,
            )?
        }
        SampleFormat::I16 => {
            device.build_output_stream(
                &config,
                move |output: &mut [i16], _: &cpal::OutputCallbackInfo| {
                    let frame = match rx.try_recv() {
                        Ok(frame) => frame,
                        Err(_) => {
                            // No data, output silence
                            for sample in output.iter_mut() {
                                *sample = 0;
                            }
                            return;
                        }
                    };

                    // Convert F32 to I16 and copy
                    let copy_len = frame.len().min(output.len());
                    for i in 0..copy_len {
                        output[i] = (frame[i] * 32767.0).clamp(-32768.0, 32767.0) as i16;
                    }

                    // Pad with silence
                    for sample in output[copy_len..].iter_mut() {
                        *sample = 0;
                    }
                },
                move |err| {
                    error!("Output stream error: {}", err);
                },
                None,
            )?
        }
        SampleFormat::U16 => {
            device.build_output_stream(
                &config,
                move |output: &mut [u16], _: &cpal::OutputCallbackInfo| {
                    let frame = match rx.try_recv() {
                        Ok(frame) => frame,
                        Err(_) => {
                            // Output silence (32768 is center for U16)
                            for sample in output.iter_mut() {
                                *sample = 32768;
                            }
                            return;
                        }
                    };

                    // Convert F32 to U16 and copy
                    let copy_len = frame.len().min(output.len());
                    for i in 0..copy_len {
                        let val = ((frame[i] + 1.0) * 32768.0).clamp(0.0, 65535.0) as u16;
                        output[i] = val;
                    }

                    // Pad with silence
                    for sample in output[copy_len..].iter_mut() {
                        *sample = 32768;
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
    debug!("Output stream started");

    Ok(AudioOutputHandle { stream, sender: tx })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_output_stream() {
        let result = create_output_stream();
        // Will succeed if system has audio device
        let _ = result;
    }
}
