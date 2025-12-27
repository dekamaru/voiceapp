use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::io::Cursor;
use std::time::Instant;
use rubato::{FftFixedInOut, Resampler};
use tracing::{error, info, warn};
use crate::audio::adjust_volume;
use crate::audio::audio_source::AudioSource;

/// Stores a preloaded notification sound
struct NotificationSound {
    samples: Vec<f32>,  // Mono f32 samples, pre-resampled to target rate
}

/// Current notification being played
struct CurrentNotification {
    samples: Vec<f32>,
    position: usize,
}

/// Internal playback state (protected by mutex)
struct PlaybackState {
    current: Option<CurrentNotification>,       // Currently playing sound
}

/// Plays notification sounds through a dedicated output stream
/// Uses "last wins" behavior: calling play() cancels current sound and starts new one
pub struct NotificationPlayer {
    sounds: HashMap<String, NotificationSound>,  // Preloaded WAVs
    state: Arc<Mutex<PlaybackState>>,           // Current playback position
    chunk_size: usize,                          // Samples per get_audio() call
}

impl NotificationPlayer {
    /// Create a new notification player with given target sample rate
    /// Loads all WAV files embedded at compile time
    pub fn new(target_sample_rate: u32) -> Self {
        let chunk_size = (target_sample_rate as f32 * 0.01) as usize; // 10ms chunks

        // Define embedded sound files (compile-time inclusion)
        // Add your WAV files to frontend/resources/sounds/ directory
        let embedded_sounds: Vec<(&str, &[u8])> = vec![
            ("join_voice", include_bytes!("../../resources/sounds/join_voice.wav")),
            ("leave_voice", include_bytes!("../../resources/sounds/leave_voice.wav")),
            ("mute", include_bytes!("../../resources/sounds/mute.wav")),
            ("unmute", include_bytes!("../../resources/sounds/unmute.wav")),
        ];

        let mut sounds = HashMap::new();

        let start = Instant::now();
        for (name, wav_bytes) in embedded_sounds {
            match Self::load_and_resample_wav(wav_bytes, target_sample_rate) {
                Ok(samples) => {
                    sounds.insert(name.to_string(), NotificationSound { samples });
                }
                Err(e) => {
                    error!("Failed to load notification sound '{}': {}", name, e);
                }
            }
        }
        let elapsed_ms = start.elapsed().as_millis();

        if !sounds.is_empty() {
            info!("Loaded and resampled {} notification sounds in {} ms", sounds.len(), elapsed_ms);
        }

        Self {
            sounds,
            state: Arc::new(Mutex::new(PlaybackState {
                current: None,
            })),
            chunk_size,
        }
    }

    /// Load WAV from bytes and resample to target sample rate
    fn load_and_resample_wav(wav_bytes: &[u8], target_sample_rate: u32) -> Result<Vec<f32>, Box<dyn std::error::Error>> {
        // Decode WAV file
        let cursor = Cursor::new(wav_bytes);
        let mut reader = hound::WavReader::new(cursor)?;

        let spec = reader.spec();
        let source_sample_rate = spec.sample_rate;

        // Read all samples and convert to mono f32
        let samples: Vec<f32> = match spec.sample_format {
            hound::SampleFormat::Int => {
                match spec.bits_per_sample {
                    16 => {
                        reader.samples::<i16>()
                            .map(|s| s.unwrap() as f32 / i16::MAX as f32)
                            .collect()
                    }
                    24 => {
                        reader.samples::<i32>()
                            .map(|s| s.unwrap() as f32 / 8388608.0)  // 2^23
                            .collect()
                    }
                    32 => {
                        reader.samples::<i32>()
                            .map(|s| s.unwrap() as f32 / i32::MAX as f32)
                            .collect()
                    }
                    _ => return Err(format!("Unsupported bit depth: {}", spec.bits_per_sample).into()),
                }
            }
            hound::SampleFormat::Float => {
                reader.samples::<f32>()
                    .map(|s| s.unwrap())
                    .collect()
            }
        };

        // Convert stereo to mono if needed
        let mono_samples: Vec<f32> = if spec.channels == 1 {
            samples
        } else if spec.channels == 2 {
            samples.chunks_exact(2)
                .map(|chunk| (chunk[0] + chunk[1]) / 2.0)
                .collect()
        } else {
            return Err(format!("Unsupported channel count: {}", spec.channels).into());
        };

        // Resample if needed
        if source_sample_rate == target_sample_rate {
            Ok(mono_samples)
        } else {
            Self::resample_audio(&mono_samples, source_sample_rate as usize, target_sample_rate as usize)
        }
    }

    /// Resample audio using rubato (similar to voice_input_pipeline.rs)
    fn resample_audio(input: &[f32], from_rate: usize, to_rate: usize) -> Result<Vec<f32>, Box<dyn std::error::Error>> {
        // Use FftFixedInOut for variable-length input
        let chunk_size = 1024;  // Process in 1024-sample chunks
        let mut resampler = FftFixedInOut::<f32>::new(from_rate, to_rate, chunk_size, 2)?;

        let mut output = Vec::new();
        let mut pos = 0;

        while pos < input.len() {
            let chunk_end = (pos + chunk_size).min(input.len());
            let chunk = &input[pos..chunk_end];

            // Pad last chunk if needed
            let mut padded_chunk = chunk.to_vec();
            if padded_chunk.len() < chunk_size {
                padded_chunk.resize(chunk_size, 0.0);
            }

            // Resample chunk
            let resampled = resampler.process(&[padded_chunk], None)?;

            // Calculate how many output samples correspond to actual input
            let valid_output_samples = if chunk_end - pos < chunk_size {
                // Last chunk - scale output proportionally
                let ratio = to_rate as f32 / from_rate as f32;
                ((chunk_end - pos) as f32 * ratio) as usize
            } else {
                resampled[0].len()
            };

            output.extend_from_slice(&resampled[0][..valid_output_samples]);
            pos = chunk_end;
        }

        Ok(output)
    }

    /// Play a notification sound (cancels any currently playing sound)
    /// If sound_id not found, logs warning and does nothing
    pub fn play(&self, sound_id: &str, volume: u8) {
        if let Some(sound) = self.sounds.get(sound_id) {
            let mut samples = sound.samples.clone();
            adjust_volume(&mut samples, volume as f32 / 100.0);

            let mut state = self.state.lock().unwrap();
            state.current = Some(CurrentNotification {
                samples,
                position: 0,
            });
        } else {
            warn!("Notification sound '{}' not found", sound_id);
        }
    }
}

impl AudioSource for NotificationPlayer {
    fn get_audio(&self) -> Result<Vec<f32>, Box<dyn std::error::Error>> {
        let mut state = self.state.lock().unwrap();

        if let Some(current) = &mut state.current {
            let remaining = current.samples.len() - current.position;
            if remaining > 0 {
                let to_copy = remaining.min(self.chunk_size);
                let chunk = current.samples[current.position..current.position + to_copy].to_vec();
                current.position += to_copy;

                // If finished, clear current
                if current.position >= current.samples.len() {
                    state.current = None;
                }

                return Ok(chunk);
            }
        }

        // No notification playing - return silence
        Ok(vec![0.0; self.chunk_size])
    }
}
