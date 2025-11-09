use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tracing::{error, info};
use voiceapp_common::{TcpPacket, PacketTypeId, encode_username, decode_username, decode_participant_list};

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
            match decode_participant_list(&packet.payload) {
                Ok(participants) => {
                    println!("[SERVER_PARTICIPANT_LIST] count={}", participants.len());
                    for (i, username) in participants.iter().enumerate() {
                        println!("  [{}] {}", i + 1, username);
                    }
                }
                Err(e) => {
                    error!("Failed to decode participant list: {}", e);
                }
            }
        }
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

    // Listen for packets from server
    let mut buf = vec![0u8; 4096];
    loop {
        let n = socket.read(&mut buf).await?;
        if n == 0 {
            info!("Server closed connection");
            break;
        }

        match TcpPacket::decode(&buf[..n]) {
            Ok((packet, _bytes_read)) => {
                pretty_print_packet(&packet).await;
            }
            Err(e) => {
                error!("Failed to decode packet: {}", e);
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
