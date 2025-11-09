use std::time::Duration;
use tokio::net::TcpStream;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use voiceapp_common::{TcpPacket, PacketTypeId, encode_username, decode_username, decode_participant_list_with_voice, encode_username_with_udp_port};
use voiceapp_server::Server;

/// Test client wrapper that handles packet I/O
struct TestClient {
    socket: TcpStream,
}

impl TestClient {
    async fn connect(addr: &str) -> Result<Self, Box<dyn std::error::Error>> {
        let socket = TcpStream::connect(addr).await?;
        Ok(TestClient { socket })
    }

    async fn send_packet(&mut self, packet: &TcpPacket) -> Result<(), Box<dyn std::error::Error>> {
        let encoded = packet.encode()?;
        self.socket.write_all(&encoded).await?;
        self.socket.flush().await?;
        Ok(())
    }

    async fn recv_packet(&mut self) -> Result<TcpPacket, Box<dyn std::error::Error>> {
        let mut buf = [0u8; 4096];
        let n = self.socket.read(&mut buf).await?;
        if n == 0 {
            return Err("Connection closed".into());
        }
        let (packet, _) = TcpPacket::decode(&buf[..n])?;
        Ok(packet)
    }

    async fn login(&mut self, username: &str) -> Result<(), Box<dyn std::error::Error>> {
        let login_pkt = TcpPacket::new(PacketTypeId::Login, encode_username(username));
        self.send_packet(&login_pkt).await?;
        Ok(())
    }
}

/// Start a test server on a random port and return its address
async fn start_test_server_with_voice_port(voice_port: u16) -> Result<String, Box<dyn std::error::Error>> {
    let server = Server::new().with_voice_port(voice_port);
    let addr = server.bind("127.0.0.1:0").await?;
    Ok(addr.to_string())
}

#[tokio::test]
async fn test_single_client_login() {
    let server_addr = start_test_server_with_voice_port(19001).await.expect("Failed to start server");

    let mut client = TestClient::connect(&server_addr)
        .await
        .expect("Failed to connect");

    // Send login
    client.login("alice").await.expect("Failed to login");

    // Receive ServerParticipantList
    let pkt = client.recv_packet().await.expect("Failed to receive packet");
    assert_eq!(pkt.packet_type, PacketTypeId::ServerParticipantList);

    let participants = decode_participant_list_with_voice(&pkt.payload).expect("Failed to decode participants");
    assert_eq!(participants.len(), 1);
    assert_eq!(participants[0].username, "alice");
    assert!(!participants[0].in_voice);
}

#[tokio::test]
async fn test_two_clients_login_broadcast() {
    let server_addr = start_test_server_with_voice_port(19002).await.expect("Failed to start server");

    // First client joins
    let mut client1 = TestClient::connect(&server_addr)
        .await
        .expect("Failed to connect client1");
    client1.login("alice").await.expect("Failed to login alice");

    // Receive ServerParticipantList
    let pkt = client1.recv_packet().await.expect("Failed to receive packet");
    assert_eq!(pkt.packet_type, PacketTypeId::ServerParticipantList);
    let participants = decode_participant_list_with_voice(&pkt.payload).expect("Failed to decode participants");
    assert_eq!(participants.len(), 1);
    assert_eq!(participants[0].username, "alice");

    // Second client joins
    let mut client2 = TestClient::connect(&server_addr)
        .await
        .expect("Failed to connect client2");
    client2.login("bob").await.expect("Failed to login bob");

    // Client2 receives ServerParticipantList with both users
    let pkt = client2.recv_packet().await.expect("Failed to receive participant list");
    assert_eq!(pkt.packet_type, PacketTypeId::ServerParticipantList);
    let participants = decode_participant_list_with_voice(&pkt.payload).expect("Failed to decode participants");
    assert_eq!(participants.len(), 2);
    assert!(participants.iter().any(|p| p.username == "alice"));
    assert!(participants.iter().any(|p| p.username == "bob"));

    // Client1 receives UserJoinedServer broadcast for bob
    let pkt = client1.recv_packet().await.expect("Failed to receive broadcast");
    assert_eq!(pkt.packet_type, PacketTypeId::UserJoinedServer);
    let username = decode_username(&pkt.payload).expect("Failed to decode username");
    assert_eq!(username, "bob");
}

#[tokio::test]
async fn test_client_receives_only_other_joins() {
    let server_addr = start_test_server_with_voice_port(19003).await.expect("Failed to start server");

    let mut client = TestClient::connect(&server_addr)
        .await
        .expect("Failed to connect");
    client.login("alice").await.expect("Failed to login");

    // Receive ServerParticipantList
    let pkt = client.recv_packet().await.expect("Failed to receive packet");
    assert_eq!(pkt.packet_type, PacketTypeId::ServerParticipantList);

    // Give server a moment to process
    tokio::time::sleep(Duration::from_millis(100)).await;

    // Should NOT receive UserJoinedServer for self
    // Try to receive packet with timeout - should timeout since no other client joined
    let result = tokio::time::timeout(Duration::from_millis(200), client.recv_packet()).await;
    assert!(result.is_err(), "Should not receive self-confirmation");
}

#[tokio::test]
async fn test_three_clients_broadcast() {
    let server_addr = start_test_server_with_voice_port(19004).await.expect("Failed to start server");

    // Client 1 joins
    let mut client1 = TestClient::connect(&server_addr)
        .await
        .expect("Failed to connect client1");
    client1.login("alice").await.expect("Failed to login alice");
    let pkt = client1.recv_packet().await.expect("Failed to receive participant list");
    assert_eq!(pkt.packet_type, PacketTypeId::ServerParticipantList);

    // Client 2 joins
    let mut client2 = TestClient::connect(&server_addr)
        .await
        .expect("Failed to connect client2");
    client2.login("bob").await.expect("Failed to login bob");
    let pkt = client2.recv_packet().await.expect("Failed to receive participant list");
    assert_eq!(pkt.packet_type, PacketTypeId::ServerParticipantList);

    // Client 1 receives broadcast for bob
    let pkt = client1.recv_packet().await.expect("Failed to receive bob broadcast");
    assert_eq!(pkt.packet_type, PacketTypeId::UserJoinedServer);
    assert_eq!(decode_username(&pkt.payload).unwrap(), "bob");

    // Client 3 joins
    let mut client3 = TestClient::connect(&server_addr)
        .await
        .expect("Failed to connect client3");
    client3.login("charlie").await.expect("Failed to login charlie");
    let pkt = client3.recv_packet().await.expect("Failed to receive participant list");
    assert_eq!(pkt.packet_type, PacketTypeId::ServerParticipantList);
    let participants = decode_participant_list_with_voice(&pkt.payload).expect("Failed to decode");
    assert_eq!(participants.len(), 3);

    // Both client1 and client2 receive broadcast for charlie
    let pkt = client1.recv_packet().await.expect("Client1 should receive charlie broadcast");
    assert_eq!(pkt.packet_type, PacketTypeId::UserJoinedServer);
    assert_eq!(decode_username(&pkt.payload).unwrap(), "charlie");

    let pkt = client2.recv_packet().await.expect("Client2 should receive charlie broadcast");
    assert_eq!(pkt.packet_type, PacketTypeId::UserJoinedServer);
    assert_eq!(decode_username(&pkt.payload).unwrap(), "charlie");
}

#[tokio::test]
async fn test_voice_channel_join() {
    let server_addr = start_test_server_with_voice_port(19005).await.expect("Failed to start server");

    // First client joins server and voice channel
    let mut client1 = TestClient::connect(&server_addr)
        .await
        .expect("Failed to connect client1");
    client1.login("alice").await.expect("Failed to login alice");
    let pkt = client1.recv_packet().await.expect("Failed to receive participant list");
    assert_eq!(pkt.packet_type, PacketTypeId::ServerParticipantList);

    // Alice joins voice channel
    let payload = encode_username_with_udp_port("alice", 19999).expect("Failed to encode");
    let join_voice_pkt = TcpPacket::new(PacketTypeId::JoinVoiceChannel, payload);
    client1.send_packet(&join_voice_pkt).await.expect("Failed to send join voice");

    // Second client joins server
    let mut client2 = TestClient::connect(&server_addr)
        .await
        .expect("Failed to connect client2");
    client2.login("bob").await.expect("Failed to login bob");
    let pkt = client2.recv_packet().await.expect("Failed to receive participant list");
    assert_eq!(pkt.packet_type, PacketTypeId::ServerParticipantList);

    // Alice receives bob joining server
    let pkt = client1.recv_packet().await.expect("Failed to receive bob joined server");
    assert_eq!(pkt.packet_type, PacketTypeId::UserJoinedServer);
    assert_eq!(decode_username(&pkt.payload).unwrap(), "bob");

    // Bob joins voice channel
    let payload = encode_username_with_udp_port("bob", 19998).expect("Failed to encode");
    let join_voice_pkt = TcpPacket::new(PacketTypeId::JoinVoiceChannel, payload);
    client2.send_packet(&join_voice_pkt).await.expect("Failed to send join voice");

    // Alice receives bob joining voice channel (broadcast to all except self)
    let pkt = client1.recv_packet().await.expect("Failed to receive bob joined voice");
    assert_eq!(pkt.packet_type, PacketTypeId::UserJoinedVoice);
    assert_eq!(decode_username(&pkt.payload).unwrap(), "bob");

    // Bob should NOT receive self-confirmation
    let result = tokio::time::timeout(Duration::from_millis(200), client2.recv_packet()).await;
    assert!(result.is_err(), "Bob should not receive self-confirmation for joining voice");
}

#[tokio::test]
async fn test_voice_channel_leave() {
    let server_addr = start_test_server_with_voice_port(19006).await.expect("Failed to start server");

    // Both clients join and enter voice channel
    let mut client1 = TestClient::connect(&server_addr)
        .await
        .expect("Failed to connect client1");
    client1.login("alice").await.expect("Failed to login alice");
    let _ = client1.recv_packet().await.expect("Failed to receive participant list");
    let payload = encode_username_with_udp_port("alice", 19997).expect("Failed to encode");
    let join_voice_pkt = TcpPacket::new(PacketTypeId::JoinVoiceChannel, payload);
    client1.send_packet(&join_voice_pkt).await.expect("Failed to send join voice");

    let mut client2 = TestClient::connect(&server_addr)
        .await
        .expect("Failed to connect client2");
    client2.login("bob").await.expect("Failed to login bob");
    let _ = client2.recv_packet().await.expect("Failed to receive participant list");

    // Bob joins voice channel
    let payload = encode_username_with_udp_port("bob", 19996).expect("Failed to encode");
    let join_voice_pkt = TcpPacket::new(PacketTypeId::JoinVoiceChannel, payload);
    client2.send_packet(&join_voice_pkt).await.expect("Failed to send join voice");

    // Alice receives bob's join to voice
    let _ = client1.recv_packet().await.expect("Failed to receive bob joined server");
    let pkt = client1.recv_packet().await.expect("Failed to receive bob joined voice");
    assert_eq!(pkt.packet_type, PacketTypeId::UserJoinedVoice);
    assert_eq!(decode_username(&pkt.payload).unwrap(), "bob");

    // Alice leaves voice channel
    let leave_voice_pkt = TcpPacket::new(PacketTypeId::UserLeftVoice, encode_username("alice"));
    client1.send_packet(&leave_voice_pkt).await.expect("Failed to send left voice");

    // Bob receives alice leaving voice
    let pkt = client2.recv_packet().await.expect("Failed to receive alice left voice");
    assert_eq!(pkt.packet_type, PacketTypeId::UserLeftVoice);
    assert_eq!(decode_username(&pkt.payload).unwrap(), "alice");
}

#[tokio::test]
async fn test_voice_isolation() {
    let server_addr = start_test_server_with_voice_port(19007).await.expect("Failed to start server");

    // Client 1 joins and enters voice
    let mut client1 = TestClient::connect(&server_addr)
        .await
        .expect("Failed to connect client1");
    client1.login("alice").await.expect("Failed to login alice");
    let pkt = client1.recv_packet().await.expect("Failed to receive participant list");
    assert_eq!(pkt.packet_type, PacketTypeId::ServerParticipantList);
    let participants = decode_participant_list_with_voice(&pkt.payload).unwrap();
    assert_eq!(participants.len(), 1);
    assert!(!participants[0].in_voice); // alice just joined, not in voice yet

    let payload = encode_username_with_udp_port("alice", 19995).expect("Failed to encode");
    let join_voice_pkt = TcpPacket::new(PacketTypeId::JoinVoiceChannel, payload);
    client1.send_packet(&join_voice_pkt).await.expect("Failed to send join voice");

    // Client 2 joins server but does NOT enter voice
    let mut client2 = TestClient::connect(&server_addr)
        .await
        .expect("Failed to connect client2");
    client2.login("bob").await.expect("Failed to login bob");
    let pkt = client2.recv_packet().await.expect("Failed to receive participant list");
    assert_eq!(pkt.packet_type, PacketTypeId::ServerParticipantList);
    let participants = decode_participant_list_with_voice(&pkt.payload).unwrap();
    assert_eq!(participants.len(), 2);
    // Alice should be marked as in_voice=true in the list
    let alice_info = participants.iter().find(|p| p.username == "alice").unwrap();
    assert!(alice_info.in_voice, "Alice should be marked in voice channel in participant list");
    let bob_info = participants.iter().find(|p| p.username == "bob").unwrap();
    assert!(!bob_info.in_voice, "Bob should NOT be in voice channel");

    // Drain UserJoinedServer broadcast for bob that alice receives
    let _ = client1.recv_packet().await.expect("Failed to drain bob joined broadcast");
}