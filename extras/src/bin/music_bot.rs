//! Music Bot - A command-line tool for streaming WAV audio files to a voice channel.
//!
//! This bot connects to the voice server and streams audio from a WAV file,
//! making it useful for playing music or sound effects in voice channels.
//!
//! # Requirements
//!
//! The WAV file must be in the following format:
//! - Sample rate: 48000 Hz
//! - Channels: Mono (1 channel)
//! - Bit depth: 16-bit
//!
//! # Usage
//!
//! ```bash
//! music_bot [OPTIONS] <wav_file>
//! ```
//!
//! # Examples
//!
//! ```bash
//! # Use default servers
//! music_bot music.wav
//!
//! # Specify custom servers
//! music_bot --server 192.168.1.100:9001 --voice-server 192.168.1.100:9002 music.wav
//! ```
//!
//! # Options
//!
//! - `--server <addr>` - Management server address (default: `127.0.0.1:9001`)
//! - `--voice-server <addr>` - Voice relay server address (default: `127.0.0.1:9002`)

use std::time::{Duration, Instant};
use tokio::time::sleep;
use tracing::{error, info};
use voiceapp_sdk::{Client, ClientEvent};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize logging
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .init();

    let args: Vec<String> = std::env::args().collect();

    let mut server_addr = "127.0.0.1:9001".to_string();
    let mut voice_server_addr = "127.0.0.1:9002".to_string();
    let mut wav_file = None;

    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--server" => {
                i += 1;
                if i < args.len() {
                    server_addr = args[i].clone();
                } else {
                    eprintln!("Error: --server requires an address");
                    std::process::exit(1);
                }
            }
            "--voice-server" => {
                i += 1;
                if i < args.len() {
                    voice_server_addr = args[i].clone();
                } else {
                    eprintln!("Error: --voice-server requires an address");
                    std::process::exit(1);
                }
            }
            arg if !arg.starts_with("--") => {
                wav_file = Some(arg.to_string());
            }
            arg => {
                eprintln!("Error: Unknown option '{}'", arg);
                std::process::exit(1);
            }
        }
        i += 1;
    }

    let wav_file = match wav_file {
        Some(f) => f,
        None => {
            eprintln!("Usage: music_bot [OPTIONS] <wav_file>");
            eprintln!();
            eprintln!("Options:");
            eprintln!("  --server <addr>        Management server address (default: 127.0.0.1:9001)");
            eprintln!("  --voice-server <addr>  Voice relay server address (default: 127.0.0.1:9002)");
            eprintln!();
            eprintln!("Example: music_bot --server 192.168.1.100:9001 music.wav");
            std::process::exit(1);
        }
    };

    info!("Music bot starting...");
    info!("Management server: {}", server_addr);
    info!("Voice relay server: {}", voice_server_addr);
    info!("WAV file: {}", wav_file);

    // Load WAV file
    info!("Loading WAV file...");
    let reader = hound::WavReader::open(wav_file)?;
    let spec = reader.spec();

    // Validate WAV file format
    if spec.sample_rate != 48000 {
        error!(
            "Invalid sample rate: {}. Expected 48000 Hz",
            spec.sample_rate
        );
        return Err("Invalid sample rate".into());
    }

    if spec.channels != 1 {
        error!(
            "Invalid channels: {}. Expected mono (1 channel)",
            spec.channels
        );
        return Err("Not mono audio".into());
    }

    if spec.bits_per_sample != 16 {
        error!(
            "Invalid bit depth: {}. Expected 16-bit",
            spec.bits_per_sample
        );
        return Err("Invalid bit depth".into());
    }

    // Read all samples
    let samples: Vec<i16> = reader
        .into_samples::<i16>()
        .collect::<Result<Vec<_>, _>>()?;

    info!(
        "WAV file loaded: {} Hz, {} channels, {} bits/sample, {} samples",
        spec.sample_rate,
        spec.channels,
        spec.bits_per_sample,
        samples.len()
    );

    // Connect to voice server
    info!("Connecting to voice servers...");
    let client = Client::new();
    client.connect(&server_addr, &voice_server_addr, "music_bot").await?;
    client.join_channel().await?;
    info!("Connected!");

    let voice_input_tx = client.get_voice_input_sender(48000)?;

    // Stream the WAV file in 20ms frames (960 samples at 48kHz)
    info!("Starting audio stream...");
    const FRAME_SIZE: usize = 960; // 20ms at 48kHz
    const FRAME_DURATION_MS: u64 = 20;
    let stream_start = Instant::now();

    for (frame_idx, chunk) in samples.chunks(FRAME_SIZE).enumerate() {
        // Calculate exact time this frame should be sent
        let frame_send_time =
            stream_start + Duration::from_millis(frame_idx as u64 * FRAME_DURATION_MS);

        // Sleep until the exact time this frame should be sent
        let now = Instant::now();
        if frame_send_time > now {
            sleep(frame_send_time - now).await;
        }

        // Convert i16 samples to f32 in range [-1.0, 1.0]
        let float_frame: Vec<f32> = chunk
            .iter()
            .map(|&sample| sample as f32 / 32768.0)
            .collect();

        if voice_input_tx.send(float_frame).await.is_err() {
            info!("Voice input channel closed, stopping stream");
            break;
        }
    }

    info!("Audio stream completed!");

    // Keep the client connected for a bit to ensure frames are sent
    sleep(Duration::from_secs(1)).await;

    Ok(())
}
