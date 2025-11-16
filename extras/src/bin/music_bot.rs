use std::time::{Duration, Instant};
use tokio::time::sleep;
use tracing::{error, info};
use voiceapp_sdk::VoiceClient;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize logging
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .init();

    let args: Vec<String> = std::env::args().collect();

    if args.len() < 2 {
        eprintln!("Usage: music_bot <wav_file>");
        eprintln!("Example: music_bot music.wav");
        std::process::exit(1);
    }

    let server_addr = "127.0.0.1:9001";
    let voice_server_addr = "127.0.0.1:9002";
    let wav_file = &args[1];

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
    let samples: Vec<i16> = reader.into_samples::<i16>()
        .collect::<Result<Vec<_>, _>>()?;

    info!(
        "WAV file loaded: {} Hz, {} channels, {} bits/sample, {} samples",
        spec.sample_rate, spec.channels, spec.bits_per_sample, samples.len()
    );

    // Connect to voice server
    info!("Connecting to voice servers...");
    let mut client = VoiceClient::connect(&server_addr, &voice_server_addr).await?;
    info!("Connected!");

    // Authenticate
    info!("Authenticating as 'music_bot'...");
    client.authenticate("music_bot").await?;
    info!("Authenticated!");

    // Get the voice input sender
    let voice_input_tx = client.voice_input_sender();

    // Stream the WAV file in 20ms frames (960 samples at 48kHz)
    info!("Starting audio stream...");
    const FRAME_SIZE: usize = 960; // 20ms at 48kHz
    const FRAME_DURATION_MS: u64 = 20;
    let stream_start = Instant::now();
    let mut frame_count = 0u64;

    for chunk in samples.chunks(FRAME_SIZE) {
        // Convert i16 samples to f32 in range [-1.0, 1.0]
        let float_frame: Vec<f32> = chunk
            .iter()
            .map(|&sample| sample as f32 / 32768.0)
            .collect();

        if voice_input_tx.send(float_frame).is_err() {
            info!("Voice input channel closed, stopping stream");
            break;
        }

        frame_count += 1;

        // Calculate when this frame should have been sent based on elapsed time
        // This adapts to any processing delays automatically
        let expected_elapsed = Duration::from_millis(frame_count * FRAME_DURATION_MS);
        let actual_elapsed = stream_start.elapsed();

        // Sleep to maintain real-time timing: if we're ahead, sleep; if behind, skip sleep
        if actual_elapsed < expected_elapsed {
            let sleep_duration = expected_elapsed - actual_elapsed;
            sleep(sleep_duration).await;
        }
    }

    info!("Audio stream completed!");

    // Keep the client connected for a bit to ensure frames are sent
    sleep(Duration::from_secs(1)).await;

    Ok(())
}
