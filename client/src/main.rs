mod audio;
mod audio_manager;
mod output;

use std::io::BufRead;
use tokio::sync::mpsc;
use tracing::{error, info, warn};
use voiceapp_sdk::VoiceClient;
use audio_manager::AudioManager;

enum UserCommand {
    Join,
    Leave,
}

impl UserCommand {
    fn parse(input: &str) -> Option<Self> {
        match input.trim().to_lowercase().as_str() {
            "join" => Some(UserCommand::Join),
            "leave" => Some(UserCommand::Leave),
            _ => None,
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

    // Hardcoded username and server addresses for now
    let username = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "client".to_string());
    
    let management_server_addr = "127.0.0.1:9001";
    let voice_server_addr = "127.0.0.1:9002";

    info!("Starting VoiceApp client with username '{}'", username);

    if let Err(e) = run_client(&username, management_server_addr, voice_server_addr).await {
        error!("Client error: {}", e);
    }
}

async fn run_client(username: &str, management_server_addr: &str, voice_server_addr: &str) -> Result<(), Box<dyn std::error::Error>> {
    // Connect to voice server - all TCP/UDP setup is internal
    let mut client = VoiceClient::connect(management_server_addr, voice_server_addr).await?;
    info!("Connected to voice server");

    // Authenticate with the server
    client.authenticate(username).await?;
    info!("Authenticated as '{}'", username);
    info!("Type 'join' to join voice channel, 'leave' to stop, or 'help' for commands");

    // Create AudioManager with SDK's voice input sender and decoder
    let voice_input_tx = client.voice_input_sender();
    let decoder = client.get_decoder();
    let audio_manager = AudioManager::new(voice_input_tx, decoder);

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
                            match client.join_channel().await {
                                Ok(_) => {
                                    // Start audio playback
                                    if let Err(e) = audio_manager.start_playback().await {
                                        error!("Failed to start audio playback: {}", e);
                                    }

                                    if let Err(e) = audio_manager.start_recording().await {
                                        error!("Failed to start recording: {}", e);
                                    }
                                }
                                Err(e) => {
                                    error!("Failed to join voice channel: {}", e);
                                }
                            }
                        }
                        UserCommand::Leave => {
                            audio_manager.stop_recording().await;
                            audio_manager.stop_playback().await;
                            match client.leave_channel().await {
                                Ok(_) => {
                                    info!("Left voice channel");
                                }
                                Err(e) => {
                                    error!("Failed to leave voice channel: {}", e);
                                }
                            }
                        }
                    }
                } else if !cmd.trim().is_empty() {
                    warn!("Unknown command: '{}'. Type 'help' for available commands", cmd);
                }
            }
        }
    }
}