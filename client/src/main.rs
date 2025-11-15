mod audio;
mod output;

use std::io::BufRead;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tracing::{error, info, warn};
use voiceapp_sdk::{VoiceClient, VoiceEncoder};
use audio::AudioInputHandle;

enum UserCommand {
    Join,
    Leave,
    Help,
}

impl UserCommand {
    fn parse(input: &str) -> Option<Self> {
        match input.trim().to_lowercase().as_str() {
            "join" => Some(UserCommand::Join),
            "leave" => Some(UserCommand::Leave),
            "help" | "?" => Some(UserCommand::Help),
            _ => None,
        }
    }
}

enum AudioState {
    Idle,
    Recording {
        _audio_handle: AudioInputHandle,
        _encoding_task: JoinHandle<()>,
    },
}

impl AudioState {
    fn is_recording(&self) -> bool {
        matches!(self, AudioState::Recording { .. })
    }
}

async fn run_client(username: &str, server_addr: &str) -> Result<(), Box<dyn std::error::Error>> {
    // Connect to voice server - all TCP/UDP setup is internal
    let mut client = VoiceClient::connect(server_addr).await?;
    info!("Connected to voice server");

    // Authenticate with the server
    client.authenticate(username).await?;
    info!("Authenticated as '{}'", username);

    // Subscribe to incoming audio frames
    let mut audio_output_rx = client.audio_output().await
        .ok_or("Failed to subscribe to audio output")?;

    // Initialize audio state
    let mut audio_state = AudioState::Idle;
    info!("Type 'join' to join voice channel, 'leave' to stop, or 'help' for commands");

    // Create channel for stdin commands
    let (tx, mut rx) = mpsc::channel::<String>(32);

    // Spawn stdin reader thread
    std::thread::spawn(move || {
        let stdin = std::io::stdin();
        let reader = stdin.lock();
        for line in reader.lines() {
            if let Ok(cmd) = line {
                let _ = tx.blocking_send(cmd);
            }
        }
    });

    // Main event loop
    loop {
        tokio::select! {
            // Handle commands from stdin
            Some(cmd) = rx.recv() => {
                if let Some(user_cmd) = UserCommand::parse(&cmd) {
                    match user_cmd {
                        UserCommand::Join => {
                            if audio_state.is_recording() {
                                warn!("User already in voice channel");
                            } else {
                                // Request to join voice channel
                                match client.join_channel().await {
                                    Ok(()) => {
                                        // Start audio input stream
                                        match audio::create_input_stream() {
                                            Ok(mut audio_handle) => {
                                                // Get user ID for voice encoder
                                                let user_id_val = client.current_user_id().await
                                                    .ok_or("User ID not available")?;

                                                // Create voice encoder
                                                match VoiceEncoder::new(user_id_val) {
                                                    Ok(encoder) => {
                                                        // Extract receiver from audio handle
                                                        match audio_handle.take_receiver() {
                                                            Ok(receiver) => {
                                                                // Clone client for encoding task
                                                                let client_clone = client.clone();
                                                                let encoding_task = tokio::spawn(async move {
                                                                    let mut receiver = receiver;
                                                                    let mut encoder = encoder;
                                                                    while let Some(audio_frame) = receiver.recv().await {
                                                                        match encoder.encode_frame(&audio_frame) {
                                                                            Ok(packets) => {
                                                                                for packet in packets {
                                                                                    if let Err(e) = client_clone.send_audio(packet) {
                                                                                        error!("Failed to send audio: {}", e);
                                                                                    }
                                                                                }
                                                                            }
                                                                            Err(e) => {
                                                                                error!("Failed to encode audio: {}", e);
                                                                            }
                                                                        }
                                                                    }
                                                                    // Flush any remaining samples
                                                                    if let Ok(Some(packet)) = encoder.flush() {
                                                                        let _ = client_clone.send_audio(packet);
                                                                    }
                                                                });

                                                                audio_state = AudioState::Recording {
                                                                    _audio_handle: audio_handle,
                                                                    _encoding_task: encoding_task,
                                                                };

                                                                info!("Joined voice channel and started recording audio");
                                                            }
                                                            Err(e) => {
                                                                error!("Failed to extract audio receiver: {}", e);
                                                            }
                                                        }
                                                    }
                                                    Err(e) => {
                                                        error!("Failed to create voice encoder: {}", e);
                                                    }
                                                }
                                            }
                                            Err(e) => {
                                                error!("Failed to initialize audio: {}", e);
                                            }
                                        }
                                    }
                                    Err(e) => {
                                        error!("Failed to join voice channel: {}", e);
                                    }
                                }
                            }
                        }
                        UserCommand::Leave => {
                            if !audio_state.is_recording() {
                                warn!("Not in voice channel");
                            } else {
                                // Stop audio stream
                                audio_state = AudioState::Idle;

                                // Leave voice channel
                                match client.leave_channel().await {
                                    Ok(()) => {
                                        info!("Left voice channel and stopped recording audio");
                                    }
                                    Err(e) => {
                                        error!("Failed to leave voice channel: {}", e);
                                    }
                                }
                            }
                        }
                        UserCommand::Help => {
                            info!("Available commands: join (join voice channel), leave (exit voice channel), help (show this message)");
                        }
                    }
                } else if !cmd.trim().is_empty() {
                    warn!("Unknown command: '{}'. Type 'help' for available commands", cmd);
                }
            }

            // Handle incoming audio frames
            Some(_frame) = audio_output_rx.recv() => {
                // Audio frames are decoded and played back internally
                // by the VoiceClient (jitter buffer, opus decoding, CPAL playback)
                // This branch just consumes the notification
            }
        }
    }
}

#[tokio::main]
async fn main() {
    #[cfg(debug_assertions)]
    {
        use tracing::Level;
        tracing_subscriber::fmt()
            .with_max_level(Level::DEBUG)
            .init();
    }

    #[cfg(not(debug_assertions))]
    {
        tracing_subscriber::fmt::init();
    }

    // Hardcoded username and server address for now
    let username = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "client".to_string());
    let server_addr = "127.0.0.1:9001";

    info!("Starting VoiceApp client with username '{}'", username);

    if let Err(e) = run_client(&username, server_addr).await {
        error!("Client error: {}", e);
    }
}
