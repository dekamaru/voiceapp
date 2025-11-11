mod audio;
mod voice;
mod opus_decode;
mod output;
mod user_voice_stream;
mod udp_voice_receiver;
mod jitter_buffer;

use std::io::BufRead;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::{mpsc, RwLock};
use tokio::task::JoinHandle;
use tracing::{error, info, warn, debug};
use voiceapp_common::{TcpPacket, PacketTypeId, encode_username, decode_username, decode_participant_list_with_voice, username_to_ssrc, VoicePacket, UdpAuthPacket, decode_voice_token};
use audio::AudioInputHandle;
use voice::VoiceEncoder;
use user_voice_stream::UserVoiceStreamManager;
use udp_voice_receiver::UdpVoiceReceiver;

async fn pretty_print_packet(packet: &TcpPacket, voice_state: &VoiceState) {
    match packet.packet_type {
        PacketTypeId::Login => {
            if let Ok(username) = decode_username(&packet.payload) {
                info!("Server: User logged in: {}", username);
            }
        }
        PacketTypeId::UserJoinedServer => {
            if let Ok(username) = decode_username(&packet.payload) {
                info!("Server: User joined server: {}", username);
            }
        }
        PacketTypeId::JoinVoiceChannel => {
            if let Ok(username) = decode_username(&packet.payload) {
                info!("Server: User requested to join voice: {}", username);
            }
        }
        PacketTypeId::UserJoinedVoice => {
            if let Ok(username) = decode_username(&packet.payload) {
                // Register this user for voice reception
                let ssrc = username_to_ssrc(&username);
                voice_state.receiver.register_user(ssrc, username.clone()).await;

                // Create a channel for receiving voice packets
                let (tx, rx) = mpsc::unbounded_channel::<VoicePacket>();

                // Register the sender in the manager
                if let Err(e) = voice_state.manager.register_sender(username.clone(), tx).await {
                    error!("Failed to register sender for {}: {}", username, e);
                } else {
                    // Create output stream for audio playback
                    if let Ok(output_handle) = output::create_output_stream() {
                        let audio_sender = output_handle.sender();
                        let username_clone = username.clone();

                        // Store the handle in VoiceState so it stays alive for the entire session
                        let mut outputs = voice_state.audio_outputs.write().await;
                        outputs.insert(username.clone(), output_handle);
                        drop(outputs);

                        // Spawn a task to process voice packets: decode and play audio
                        tokio::spawn(async move {
                            let mut rx = rx;
                            while let Some(packet) = rx.recv().await {
                                // Decode Opus frame to mono F32
                                match opus_decode::OpusDecoder::new() {
                                    Ok(mut decoder) => {
                                        match decoder.decode_frame(&packet.opus_frame) {
                                            Ok(mono_samples) => {
                                                // Convert mono to stereo for playback
                                                let stereo_samples = opus_decode::mono_to_stereo(&mono_samples);

                                                // Send to audio output
                                                if let Err(e) = audio_sender.send(stereo_samples) {
                                                    error!("Failed to queue audio for {}: {}", username_clone, e);
                                                }
                                            }
                                            Err(e) => {
                                                error!("Failed to decode audio for {}: {}", username_clone, e);
                                            }
                                        }
                                    }
                                    Err(e) => {
                                        error!("Failed to create decoder for {}: {}", username_clone, e);
                                        break;
                                    }
                                }
                            }
                            debug!("Voice playback for {} closed", username_clone);
                        });
                        debug!("Created voice playback for {}", username);
                    } else {
                        error!("Failed to create output stream for {}", username);
                    }
                }

                info!("User joined voice channel: {} (SSRC: {})", username, ssrc);
            }
        }
        PacketTypeId::UserLeftVoice => {
            if let Ok(username) = decode_username(&packet.payload) {
                // Unregister this user from voice reception
                let ssrc = username_to_ssrc(&username);
                voice_state.receiver.unregister_user(ssrc).await;

                // Unregister from packet sender (will close the channel)
                if let Err(e) = voice_state.manager.unregister_sender(&username).await {
                    warn!("Failed to unregister sender for {}: {}", username, e);
                }

                // Remove the audio output handle (will stop the audio stream)
                let mut outputs = voice_state.audio_outputs.write().await;
                outputs.remove(&username);
                drop(outputs);

                info!("User left voice channel: {} (SSRC: {})", username, ssrc);
            }
        }
        PacketTypeId::UserLeftServer => {
            if let Ok(username) = decode_username(&packet.payload) {
                info!("User left server: {}", username);
            }
        }
        PacketTypeId::LoginResponse => {
            match decode_voice_token(&packet.payload) {
                Ok(token) => {
                    // Store the token for UDP voice authentication
                    let mut token_lock = voice_state.voice_token.write().await;
                    *token_lock = Some(token);
                    debug!("UDP voice token received in LoginResponse: {}", token);
                    info!("Successfully logged in with UDP voice token");
                }
                Err(e) => {
                    error!("Failed to decode UDP voice token from LoginResponse: {}", e);
                }
            }
        }
        PacketTypeId::ServerParticipantList => {
            match decode_participant_list_with_voice(&packet.payload) {
                Ok(participants) => {
                    info!("Server participant list: {} users", participants.len());
                    for participant in participants.iter() {
                        if participant.in_voice {
                            // Register voice users
                            let ssrc = username_to_ssrc(&participant.username);
                            voice_state.receiver.register_user(ssrc, participant.username.clone()).await;

                            // Register voice packet senders for users already in voice
                            let (tx, rx) = mpsc::unbounded_channel::<VoicePacket>();
                            if let Err(e) = voice_state.manager.register_sender(participant.username.clone(), tx).await {
                                error!("Failed to register sender for {}: {}", participant.username, e);
                            } else {
                                // Create output stream for audio playback
                                if let Ok(output_handle) = output::create_output_stream() {
                                    let audio_sender = output_handle.sender();
                                    let username = participant.username.clone();

                                    // Store the handle in VoiceState so it stays alive for the entire session
                                    let mut outputs = voice_state.audio_outputs.write().await;
                                    outputs.insert(participant.username.clone(), output_handle);
                                    drop(outputs);

                                    // Spawn a task to process voice packets: decode and play audio
                                    tokio::spawn(async move {
                                        let mut rx = rx;
                                        while let Some(packet) = rx.recv().await {
                                            // Decode Opus frame to mono F32
                                            match opus_decode::OpusDecoder::new() {
                                                Ok(mut decoder) => {
                                                    match decoder.decode_frame(&packet.opus_frame) {
                                                        Ok(mono_samples) => {
                                                            // Convert mono to stereo for playback
                                                            let stereo_samples = opus_decode::mono_to_stereo(&mono_samples);

                                                            // Send to audio output
                                                            if let Err(e) = audio_sender.send(stereo_samples) {
                                                                error!("Failed to queue audio for {}: {}", username, e);
                                                            }
                                                        }
                                                        Err(e) => {
                                                            error!("Failed to decode audio for {}: {}", username, e);
                                                        }
                                                    }
                                                }
                                                Err(e) => {
                                                    error!("Failed to create decoder for {}: {}", username, e);
                                                    break;
                                                }
                                            }
                                        }
                                        debug!("Voice playback for {} closed", username);
                                    });
                                    debug!("Registered voice playback for {} (from participant list)", participant.username);
                                } else {
                                    error!("Failed to create output stream for {}", participant.username);
                                }
                            }

                            debug!("  {} [IN_VOICE] - SSRC: {}", participant.username, ssrc);
                        } else {
                            debug!("  {}", participant.username);
                        }
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

/// Shared state for voice reception
struct VoiceState {
    manager: Arc<UserVoiceStreamManager>,
    receiver: Arc<UdpVoiceReceiver>,
    // Keep audio output handles alive for the lifetime of the session
    audio_outputs: Arc<RwLock<std::collections::HashMap<String, output::AudioOutputHandle>>>,
    // Token for UDP voice authentication
    voice_token: Arc<RwLock<Option<u64>>>,
}

impl VoiceState {
    async fn new(voice_bind_addr: &str) -> Result<Self, Box<dyn std::error::Error>> {
        let manager = Arc::new(UserVoiceStreamManager::new());
        let receiver = Arc::new(UdpVoiceReceiver::new(voice_bind_addr, manager.clone()).await?);

        // Spawn the receiver task
        receiver.start_receiving();

        Ok(VoiceState {
            manager,
            receiver,
            audio_outputs: Arc::new(RwLock::new(std::collections::HashMap::new())),
            voice_token: Arc::new(RwLock::new(None)),
        })
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

    // Initialize voice reception (listen on random port)
    let voice_state = VoiceState::new("127.0.0.1:0").await?;
    info!("Voice receiver listening on {}", voice_state.receiver.local_addr()?);

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
    let mut buf = vec![0u8; 4096];
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
                                // First, request to join voice channel
                                let pkt = TcpPacket::new(PacketTypeId::JoinVoiceChannel, vec![]);
                                match socket.write_all(&pkt.encode()?).await {
                                    Ok(_) => {
                                        socket.flush().await?;
                                        info!("Sent join voice channel request");

                                        // Start audio input stream
                                        match audio::create_input_stream() {
                                            Ok(mut audio_handle) => {
                                                // Create voice encoder
                                                match VoiceEncoder::new(username.to_string()) {
                                                    Ok(mut encoder) => {
                                                        // Get the token received during login
                                                        let token = {
                                                            let token_lock = voice_state.voice_token.read().await;
                                                            token_lock.clone()
                                                        };

                                                        if let Some(token) = token {
                                                            debug!("Using UDP voice token for authentication: {}", token);
                                                            let server_voice_addr = server_addr.replace("9001", "9002");
                                                            // Send auth packet with retries (3 attempts, 5 seconds timeout each)
                                                            // Use receiver's socket to ensure packets come from listening port
                                                            let max_attempts = 3;
                                                            let mut auth_success = false;

                                                            for attempt in 1..=max_attempts {
                                                                let auth_packet = UdpAuthPacket::new(token);
                                                                match auth_packet.encode() {
                                                                    Ok(auth_data) => {
                                                                        // Send from receiver socket so server knows to send packets back to receiver port
                                                                        if let Err(e) = voice_state.receiver.send_to(&auth_data, &server_voice_addr).await {
                                                                            error!("Attempt {}: Failed to send auth packet from receiver socket: {}", attempt, e);
                                                                            continue;
                                                                        }
                                                                        debug!("Attempt {}: Sent UDP auth packet from receiver socket ({})", attempt, voice_state.receiver.local_addr().unwrap_or_else(|_| "unknown".parse().unwrap()));

                                                                        // Wait for auth response with 5-second timeout on receiver socket
                                                                        match voice_state.receiver.wait_auth_response(5).await {
                                                                            Ok(true) => {
                                                                                info!("Attempt {}: Auth response received - SUCCESS", attempt);
                                                                                auth_success = true;
                                                                                break;
                                                                            }
                                                                            Ok(false) => {
                                                                                error!("Attempt {}: Auth response received - FAILURE", attempt);
                                                                                continue;
                                                                            }
                                                                            Err(e) => {
                                                                                warn!("Attempt {}: Auth response error: {}", attempt, e);
                                                                                continue;
                                                                            }
                                                                        }
                                                                    }
                                                                    Err(e) => {
                                                                        error!("Attempt {}: Failed to encode auth packet: {}", attempt, e);
                                                                        continue;
                                                                    }
                                                                }
                                                            }

                                                            if auth_success {
                                                                // Start receiving voice packets now that auth succeeded
                                                                voice_state.receiver.start_receiving();
                                                                debug!("Started receiving voice packets after successful auth");

                                                                // Extract receiver from audio handle
                                                                match audio_handle.take_receiver() {
                                                                    Ok(receiver) => {
                                                                        // Spawn encoding task
                                                                        // Use receiver socket for sending voice packets (not the separate sender socket)
                                                                        let receiver_socket = voice_state.receiver.clone();
                                                                        let encoding_task = tokio::spawn(async move {
                                                                            let mut receiver = receiver;
                                                                            while let Some(audio_frame) = receiver.recv().await {
                                                                                match encoder.encode_frame(&audio_frame) {
                                                                                    Ok(packets) => {
                                                                                        for packet in packets {
                                                                                            if let Err(e) = receiver_socket.send_voice_packet(&packet, &server_voice_addr).await {
                                                                                                error!("Failed to send voice packet from receiver socket: {}", e);
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
                                                                                let _ = receiver_socket.send_voice_packet(&packet, &server_voice_addr).await;
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
                                                                        audio_state = AudioState::Idle;
                                                                    }
                                                                }
                                                            } else {
                                                                error!("Failed to authenticate with voice server after {} attempts", max_attempts);
                                                                audio_state = AudioState::Idle;
                                                            }
                                                        } else {
                                                            error!("UDP voice token not available - not logged in or token not received");
                                                            audio_state = AudioState::Idle;
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
                                        error!("Failed to send join packet: {}", e);
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

                                // Send leave packet
                                let pkt = TcpPacket::new(PacketTypeId::UserLeftVoice, vec![]);
                                match socket.write_all(&pkt.encode()?).await {
                                    Ok(_) => {
                                        socket.flush().await?;
                                        info!("Left voice channel and stopped recording audio");
                                    }
                                    Err(e) => {
                                        error!("Failed to send leave packet: {}", e);
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
                                pretty_print_packet(&packet, &voice_state).await;
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
