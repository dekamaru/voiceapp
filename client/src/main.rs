mod audio;
mod voice;
mod udp_voice;

use std::io::BufRead;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tracing::{error, info, warn};
use voiceapp_common::{TcpPacket, PacketTypeId, encode_username, decode_username, decode_participant_list_with_voice};
use audio::AudioInputHandle;
use voice::VoiceEncoder;
use udp_voice::UdpVoiceSender;

async fn pretty_print_packet(packet: &TcpPacket) {
    match packet.packet_type {
        PacketTypeId::Login => {
            if let Ok(username) = decode_username(&packet.payload) {
                println!("[LOGIN] username={}", username);
            }
        }
        PacketTypeId::UserJoinedServer => {
            if let Ok(username) = decode_username(&packet.payload) {
                println!("[USER_JOINED_SERVER] username={}", username);
            }
        }
        PacketTypeId::JoinVoiceChannel => {
            if let Ok(username) = decode_username(&packet.payload) {
                println!("[JOIN_VOICE_CHANNEL] username={}", username);
            }
        }
        PacketTypeId::UserJoinedVoice => {
            if let Ok(username) = decode_username(&packet.payload) {
                println!("[USER_JOINED_VOICE] username={}", username);
            }
        }
        PacketTypeId::UserLeftVoice => {
            if let Ok(username) = decode_username(&packet.payload) {
                println!("[USER_LEFT_VOICE] username={}", username);
            }
        }
        PacketTypeId::UserLeftServer => {
            if let Ok(username) = decode_username(&packet.payload) {
                println!("[USER_LEFT_SERVER] username={}", username);
            }
        }
        PacketTypeId::ServerParticipantList => {
            match decode_participant_list_with_voice(&packet.payload) {
                Ok(participants) => {
                    println!("[SERVER_PARTICIPANT_LIST] count={}", participants.len());
                    for (i, participant) in participants.iter().enumerate() {
                        let voice_status = if participant.in_voice { " [IN_VOICE]" } else { "" };
                        println!("  [{}] {}{}", i + 1, participant.username, voice_status);
                    }
                }
                Err(e) => {
                    error!("Failed to decode participant list: {}", e);
                }
            }
        }
    }
}

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
    let mut socket = TcpStream::connect(server_addr).await?;
    info!("Connected to server at {}", server_addr);

    // Send Login packet
    let login_packet = TcpPacket::new(PacketTypeId::Login, encode_username(username));
    socket.write_all(&login_packet.encode()?).await?;
    socket.flush().await?;
    info!("Sent login packet for user '{}'", username);

    // Initialize audio state
    let mut audio_state = AudioState::Idle;
    println!("\nType 'join' to join voice channel and start recording, 'leave' to stop, or 'help' for commands");

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
    let mut buf = vec![0u8; 4096];
    loop {
        tokio::select! {
            // Handle commands from stdin
            Some(cmd) = rx.recv() => {
                if let Some(user_cmd) = UserCommand::parse(&cmd) {
                    match user_cmd {
                        UserCommand::Join => {
                            if audio_state.is_recording() {
                                warn!("Already in voice channel");
                                println!(">>> Already in voice channel");
                            } else {
                                // Start audio input stream
                                match audio::create_input_stream() {
                                    Ok(mut audio_handle) => {
                                        // Create voice encoder
                                        match VoiceEncoder::new() {
                                            Ok(mut encoder) => {
                                                // Create UDP voice sender
                                                let server_voice_addr = server_addr.replace("9001", "9002");
                                                match UdpVoiceSender::new("127.0.0.1:0", &server_voice_addr).await {
                                                    Ok(udp_sender) => {
                                                        // Extract receiver from audio handle
                                                        let receiver = audio_handle.take_receiver();

                                                        // Spawn encoding task
                                                        let encoding_task = tokio::spawn(async move {
                                                            let mut receiver = receiver;
                                                            while let Some(audio_frame) = receiver.recv().await {
                                                                match encoder.encode_frame(&audio_frame) {
                                                                    Ok(packets) => {
                                                                        for packet in packets {
                                                                            if let Err(e) = udp_sender.send_packet(&packet).await {
                                                                                error!("Failed to send voice packet: {}", e);
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
                                                                let _ = udp_sender.send_packet(&packet).await;
                                                            }
                                                        });

                                                        audio_state = AudioState::Recording {
                                                            _audio_handle: audio_handle,
                                                            _encoding_task: encoding_task,
                                                        };

                                                        // Send join packet
                                                        let pkt = TcpPacket::new(PacketTypeId::JoinVoiceChannel, encode_username(username));
                                                        match socket.write_all(&pkt.encode()?).await {
                                                            Ok(_) => {
                                                                socket.flush().await?;
                                                                println!(">>> Joined voice channel and started recording");
                                                                info!("Started recording audio");
                                                            }
                                                            Err(e) => {
                                                                error!("Failed to send join packet: {}", e);
                                                                // Revert audio state on error
                                                                audio_state = AudioState::Idle;
                                                            }
                                                        }
                                                    }
                                                    Err(e) => {
                                                        error!("Failed to create UDP voice sender: {}", e);
                                                        println!(">>> Failed to initialize voice transmission: {}", e);
                                                    }
                                                }
                                            }
                                            Err(e) => {
                                                error!("Failed to create voice encoder: {}", e);
                                                println!(">>> Failed to initialize voice encoding: {}", e);
                                            }
                                        }
                                    }
                                    Err(e) => {
                                        error!("Failed to initialize audio: {}", e);
                                        println!(">>> Failed to start recording: {}", e);
                                    }
                                }
                            }
                        }
                        UserCommand::Leave => {
                            if !audio_state.is_recording() {
                                warn!("Not in voice channel");
                                println!(">>> Not in voice channel");
                            } else {
                                // Stop audio stream
                                audio_state = AudioState::Idle;

                                // Send leave packet
                                let pkt = TcpPacket::new(PacketTypeId::UserLeftVoice, encode_username(username));
                                match socket.write_all(&pkt.encode()?).await {
                                    Ok(_) => {
                                        socket.flush().await?;
                                        println!(">>> Left voice channel and stopped recording");
                                        info!("Stopped recording audio");
                                    }
                                    Err(e) => {
                                        error!("Failed to send leave packet: {}", e);
                                    }
                                }
                            }
                        }
                        UserCommand::Help => {
                            println!("\nAvailable commands:");
                            println!("  join  - Join voice channel");
                            println!("  leave - Leave voice channel");
                            println!("  help  - Show this help message\n");
                        }
                    }
                } else if !cmd.trim().is_empty() {
                    println!("Unknown command: '{}'. Type 'help' for available commands", cmd);
                }
            }

            // Handle packets from server
            result = socket.read(&mut buf) => {
                match result {
                    Ok(0) => {
                        info!("Server closed connection");
                        break;
                    }
                    Ok(n) => {
                        match TcpPacket::decode(&buf[..n]) {
                            Ok((packet, _bytes_read)) => {
                                pretty_print_packet(&packet).await;
                            }
                            Err(e) => {
                                error!("Failed to decode packet: {}", e);
                            }
                        }
                    }
                    Err(e) => {
                        error!("Socket read error: {}", e);
                        break;
                    }
                }
            }
        }
    }

    Ok(())
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();

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
