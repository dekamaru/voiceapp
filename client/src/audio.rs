use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{Device, Stream, StreamConfig, SampleRate, SampleFormat};
use tokio::sync::mpsc;
use tracing::{debug, error, info};

const TARGET_SAMPLE_RATE: u32 = 48000;
const TARGET_CHANNELS: u16 = 1; // mono
const AUDIO_BUFFER_CAPACITY: usize = 4800; // ~100ms at 48kHz

/// Audio frame: mono F32 samples at 48kHz
pub type AudioFrame = Vec<f32>;

/// Handle to manage audio input stream lifecycle
pub struct AudioInputHandle {
    stream: Stream,
    receiver: Option<mpsc::Receiver<AudioFrame>>,
}

impl AudioInputHandle {
    /// Stop the audio stream and drain any remaining frames
    pub fn stop(self) {
        drop(self.stream); // dropping stream stops it
    }

    /// Get mutable receiver to consume audio frames
    pub fn receiver_mut(&mut self) -> &mut mpsc::Receiver<AudioFrame> {
        self.receiver.as_mut().expect("receiver already taken")
    }

    /// Extract the receiver from this handle (consuming it)
    pub fn take_receiver(&mut self) -> mpsc::Receiver<AudioFrame> {
        self.receiver.take().expect("receiver already taken")
    }
}

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
pub fn create_input_stream() -> Result<AudioInputHandle, Box<dyn std::error::Error>> {
    let device = find_input_device()?;
    let (config, format) = get_stream_config(&device)?;

    info!(
        "Input stream config: {} channels, {} Hz, format: {:?}",
        config.channels, config.sample_rate.0, format
    );

    let (tx, rx) = mpsc::channel::<AudioFrame>(AUDIO_BUFFER_CAPACITY / 480); // ~10 frames buffer

    let channels = config.channels;

    // Match on sample format to build the appropriate stream
    let stream = match format {
        SampleFormat::F32 => {
            device.build_input_stream(
                &config,
                move |data: &[f32], _: &cpal::InputCallbackInfo| {
                    let mono_samples = stereo_to_mono_f32(data, channels);
                    let _ = tx.blocking_send(mono_samples);
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
                    let mono_samples = stereo_to_mono_f32(&f32_data, channels);
                    let _ = tx.blocking_send(mono_samples);
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
                    let mono_samples = stereo_to_mono_f32(&f32_data, channels);
                    let _ = tx.blocking_send(mono_samples);
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

    Ok(AudioInputHandle { stream, receiver: Some(rx) })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_stereo_to_mono_f32_single_channel() {
        let stereo = vec![0.1, 0.2, 0.3, 0.4];
        let mono = stereo_to_mono_f32(&stereo, 1);
        assert_eq!(mono, stereo);
    }

    #[test]
    fn test_stereo_to_mono_f32_two_channels() {
        // 2 samples, 2 channels: [L1, R1, L2, R2]
        let stereo = vec![0.4, 0.2, 0.6, 0.4];
        let mono = stereo_to_mono_f32(&stereo, 2);

        // Average each pair: (0.4+0.2)/2=0.3, (0.6+0.4)/2=0.5
        assert_eq!(mono.len(), 2);
        assert!((mono[0] - 0.3).abs() < 1e-6);
        assert!((mono[1] - 0.5).abs() < 1e-6);
    }

    #[test]
    fn test_stereo_to_mono_f32_three_channels() {
        // 1 sample, 3 channels: [C1, C2, C3]
        let stereo = vec![0.3, 0.6, 0.9];
        let mono = stereo_to_mono_f32(&stereo, 3);

        // Average all three: (0.3+0.6+0.9)/3=0.6
        assert_eq!(mono.len(), 1);
        assert!((mono[0] - 0.6).abs() < 1e-6);
    }

    #[test]
    fn test_stereo_to_mono_f32_empty() {
        let stereo: Vec<f32> = vec![];
        let mono = stereo_to_mono_f32(&stereo, 2);
        assert_eq!(mono.len(), 0);
    }

    #[test]
    fn test_i16_to_f32_conversion() {
        // Test conversion from i16 to f32
        let i16_samples = vec![0i16, 16384, 32767, -16384, -32768];
        let f32_samples: Vec<f32> = i16_samples
            .iter()
            .map(|&s| s as f32 / 32768.0)
            .collect();

        assert!((f32_samples[0] - 0.0).abs() < 1e-6); // 0 -> 0.0
        assert!((f32_samples[1] - 0.5).abs() < 1e-6); // 16384 -> ~0.5
        assert!((f32_samples[2] - 0.99997).abs() < 1e-4); // 32767 -> ~1.0
        assert!((f32_samples[3] - (-0.5)).abs() < 1e-6); // -16384 -> ~-0.5
        assert!((f32_samples[4] - (-1.0)).abs() < 1e-6); // -32768 -> -1.0
    }

    #[test]
    fn test_u16_to_f32_conversion() {
        // Test conversion from u16 to f32
        let u16_samples = vec![0u16, 16384, 32768, 49152, 65535];
        let f32_samples: Vec<f32> = u16_samples
            .iter()
            .map(|&s| (s as f32 / 32768.0) - 1.0)
            .collect();

        assert!((f32_samples[0] - (-1.0)).abs() < 1e-6); // 0 -> -1.0
        assert!((f32_samples[1] - (-0.5)).abs() < 1e-6); // 16384 -> ~-0.5
        assert!((f32_samples[2] - 0.0).abs() < 1e-6); // 32768 -> 0.0
        assert!((f32_samples[3] - 0.5).abs() < 1e-6); // 49152 -> ~0.5
        assert!((f32_samples[4] - 1.0).abs() < 1e-4); // 65535 -> ~1.0
    }

    #[test]
    fn test_stereo_to_mono_after_i16_conversion() {
        // Full pipeline: i16 stereo -> f32 -> mono
        let i16_stereo = vec![0i16, 0, 32767, -32768];
        let f32_data: Vec<f32> = i16_stereo.iter().map(|&s| s as f32 / 32768.0).collect();
        let mono = stereo_to_mono_f32(&f32_data, 2);

        assert_eq!(mono.len(), 2);
        // First: (0.0 + 0.0) / 2 = 0.0
        assert!((mono[0] - 0.0).abs() < 1e-6);
        // Second: (0.99997 + (-1.0)) / 2 = -0.000015
        assert!((mono[1] - (-0.00001)).abs() < 1e-4);
    }

    #[test]
    fn test_stereo_to_mono_after_u16_conversion() {
        // Full pipeline: u16 stereo -> f32 -> mono
        let u16_stereo = vec![32768u16, 32768, 0, 65535];
        let f32_data: Vec<f32> = u16_stereo
            .iter()
            .map(|&s| (s as f32 / 32768.0) - 1.0)
            .collect();
        let mono = stereo_to_mono_f32(&f32_data, 2);

        assert_eq!(mono.len(), 2);
        // First: (0.0 + 0.0) / 2 = 0.0
        assert!((mono[0] - 0.0).abs() < 1e-6);
        // Second: (-1.0 + 1.0) / 2 = 0.0
        assert!((mono[1] - 0.0).abs() < 1e-4);
    }
}
