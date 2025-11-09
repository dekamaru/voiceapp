use voiceapp_client::voice::VoiceEncoder;
use voiceapp_client::udp_voice::UdpVoiceSender;
use voiceapp_common::VoicePacket;
use tokio::net::UdpSocket;

/// Test that voice encoder produces valid packets with correct structure
#[test]
fn test_voice_encoder_produces_valid_packets() {
    let mut encoder = VoiceEncoder::new().expect("Failed to create encoder");

    // Create a frame of silence
    let samples = vec![0.0; 960];

    let packets = encoder.encode_frame(&samples).expect("Encoding failed");

    assert_eq!(packets.len(), 1);

    let packet = &packets[0];
    assert_eq!(packet.sequence, 0);
    assert_eq!(packet.timestamp, 0);
    assert!(!packet.opus_frame.is_empty());

    // Verify packet can be encoded/decoded
    let encoded = packet.encode().expect("Encoding failed");
    let (decoded, bytes_read) = VoicePacket::decode(&encoded).expect("Decoding failed");

    assert_eq!(bytes_read, encoded.len());
    assert_eq!(decoded.sequence, packet.sequence);
    assert_eq!(decoded.timestamp, packet.timestamp);
    assert_eq!(decoded.ssrc, packet.ssrc);
    assert_eq!(decoded.opus_frame, packet.opus_frame);
}

/// Test that encoder correctly handles multiple consecutive frames
#[test]
fn test_voice_encoder_multiple_frame_sequence() {
    let mut encoder = VoiceEncoder::new().expect("Failed to create encoder");

    // Send 3 frames worth of samples
    let samples = vec![0.0; 2880]; // 3 * 960

    let packets = encoder.encode_frame(&samples).expect("Encoding failed");

    assert_eq!(packets.len(), 3);

    // Verify sequence numbers increase
    assert_eq!(packets[0].sequence, 0);
    assert_eq!(packets[1].sequence, 1);
    assert_eq!(packets[2].sequence, 2);

    // Verify timestamps are correct
    assert_eq!(packets[0].timestamp, 0);
    assert_eq!(packets[1].timestamp, 960);
    assert_eq!(packets[2].timestamp, 1920);

    // Verify SSRC is the same
    assert_eq!(packets[0].ssrc, packets[1].ssrc);
    assert_eq!(packets[1].ssrc, packets[2].ssrc);
}

/// Test that UDP sender can transmit packets
#[tokio::test]
async fn test_udp_sender_transmits_packets() {
    // Create a receiver to listen for packets
    let receiver = UdpSocket::bind("127.0.0.1:0")
        .await
        .expect("Failed to bind receiver socket");
    let receiver_addr = receiver.local_addr().expect("Failed to get receiver address");

    // Create sender pointing to receiver
    let sender = UdpVoiceSender::new("127.0.0.1:0", &receiver_addr.to_string())
        .await
        .expect("Failed to create sender");

    // Create a test packet
    let packet = VoicePacket::new(42, 1000, 54321, vec![0x11, 0x22, 0x33, 0x44]);

    // Send it
    sender.send_packet(&packet).await.expect("Failed to send");

    // Receive and verify
    let mut buf = vec![0u8; 1024];
    let (n, _) = receiver
        .recv_from(&mut buf)
        .await
        .expect("Failed to receive");

    buf.truncate(n);

    // Decode the received packet
    let (received, _) = VoicePacket::decode(&buf).expect("Failed to decode received packet");

    assert_eq!(received.sequence, packet.sequence);
    assert_eq!(received.timestamp, packet.timestamp);
    assert_eq!(received.ssrc, packet.ssrc);
    assert_eq!(received.opus_frame, packet.opus_frame);
}

/// Test the full voice encoding pipeline: samples → encoder → packets → UDP transmission
#[tokio::test]
async fn test_voice_pipeline_end_to_end() {
    // Setup receiver
    let receiver = UdpSocket::bind("127.0.0.1:0")
        .await
        .expect("Failed to bind receiver");
    let receiver_addr = receiver.local_addr().expect("Failed to get address");

    // Create encoder and sender
    let mut encoder = VoiceEncoder::new().expect("Failed to create encoder");
    let sender = UdpVoiceSender::new("127.0.0.1:0", &receiver_addr.to_string())
        .await
        .expect("Failed to create sender");

    // Simulate audio data: 2 frames worth
    let audio_data = vec![0.0; 1920];

    // Encode
    let packets = encoder.encode_frame(&audio_data).expect("Encoding failed");
    assert_eq!(packets.len(), 2);

    // Send both packets
    for packet in &packets {
        sender.send_packet(packet).await.expect("Failed to send");
    }

    // Receive and verify first packet
    let mut buf = vec![0u8; 1024];
    let (n, _) = receiver.recv_from(&mut buf).await.expect("Failed to receive packet 1");
    buf.truncate(n);

    let (packet1, _) = VoicePacket::decode(&buf).expect("Failed to decode packet 1");
    assert_eq!(packet1.sequence, packets[0].sequence);
    assert_eq!(packet1.timestamp, packets[0].timestamp);

    // Receive and verify second packet
    let mut buf = vec![0u8; 1024];
    let (n, _) = receiver.recv_from(&mut buf).await.expect("Failed to receive packet 2");
    buf.truncate(n);

    let (packet2, _) = VoicePacket::decode(&buf).expect("Failed to decode packet 2");
    assert_eq!(packet2.sequence, packets[1].sequence);
    assert_eq!(packet2.timestamp, packets[1].timestamp);
}

/// Test that voice encoder handles incomplete frames + flush
#[test]
fn test_voice_encoder_incomplete_frame_flush() {
    let mut encoder = VoiceEncoder::new().expect("Failed to create encoder");

    // Send 1.5 frames worth of data
    let samples = vec![0.0; 1440]; // 1.5 * 960

    let packets = encoder.encode_frame(&samples).expect("Encoding failed");

    // Should only have 1 complete frame
    assert_eq!(packets.len(), 1);

    // Flush the incomplete frame
    let flushed = encoder.flush().expect("Flush failed");
    assert!(flushed.is_some());

    let packet = flushed.unwrap();
    // Sequence should increment from previous frame
    assert_eq!(packet.sequence, 1);
    assert_eq!(packet.timestamp, 960);
}

/// Test multiple encoders have different SSRCs
#[test]
fn test_different_encoders_different_ssrc() {
    let encoder1 = VoiceEncoder::new().expect("Failed to create encoder 1");
    let encoder2 = VoiceEncoder::new().expect("Failed to create encoder 2");

    // SSRCs should be different (extremely unlikely to be the same)
    assert_ne!(encoder1.ssrc, encoder2.ssrc);
}

/// Test that opus encoding compresses audio significantly
#[test]
fn test_opus_compression_ratio() {
    let mut encoder = VoiceEncoder::new().expect("Failed to create encoder");

    // 960 samples * 4 bytes per f32 = 3840 bytes
    let samples = vec![0.0; 960];

    let packets = encoder.encode_frame(&samples).expect("Encoding failed");

    assert_eq!(packets.len(), 1);

    // Opus should compress this to much less than 3840 bytes
    // Silence/low entropy should compress to <100 bytes
    assert!(packets[0].opus_frame.len() < 500);

    // But should still be at least a few bytes
    assert!(packets[0].opus_frame.len() > 0);
}

/// Test sending burst of voice packets via UDP
#[tokio::test]
async fn test_udp_burst_transmission() {
    // Setup receiver
    let receiver = UdpSocket::bind("127.0.0.1:0")
        .await
        .expect("Failed to bind receiver");
    let receiver_addr = receiver.local_addr().expect("Failed to get address");

    // Create sender
    let sender = UdpVoiceSender::new("127.0.0.1:0", &receiver_addr.to_string())
        .await
        .expect("Failed to create sender");

    // Create and send 10 packets rapidly
    let packets: Vec<_> = (0..10)
        .map(|i| VoicePacket::new(i, i * 960, 12345, vec![0xFF; 64]))
        .collect();

    for packet in &packets {
        sender.send_packet(packet).await.expect("Send failed");
    }

    // Receive and verify all packets
    for i in 0..10 {
        let mut buf = vec![0u8; 1024];
        let (n, _) = receiver.recv_from(&mut buf).await.expect("Receive failed");
        buf.truncate(n);

        let (received, _) = VoicePacket::decode(&buf).expect("Decode failed");
        assert_eq!(received.sequence, packets[i].sequence);
        assert_eq!(received.timestamp, packets[i].timestamp);
    }
}

/// Test encoder with varying sample patterns
#[test]
fn test_voice_encoder_various_audio_patterns() {
    // Test silence
    {
        let mut encoder = VoiceEncoder::new().expect("Failed to create encoder");
        let silence = vec![0.0; 960];
        let packets = encoder.encode_frame(&silence).expect("Encoding failed");
        assert_eq!(packets.len(), 1);
        assert!(!packets[0].opus_frame.is_empty());
    }

    // Test constant tone
    {
        let mut encoder = VoiceEncoder::new().expect("Failed to create encoder");
        let tone: Vec<f32> = vec![0.5; 960];
        let packets = encoder.encode_frame(&tone).expect("Encoding failed");
        assert_eq!(packets.len(), 1);
        assert!(!packets[0].opus_frame.is_empty());
    }

    // Test sine wave
    {
        let mut encoder = VoiceEncoder::new().expect("Failed to create encoder");
        let sine: Vec<f32> = (0..960)
            .map(|i| (i as f32 * 2.0 * std::f32::consts::PI / 960.0).sin() * 0.5)
            .collect();
        let packets = encoder.encode_frame(&sine).expect("Encoding failed");
        assert_eq!(packets.len(), 1);
        assert!(!packets[0].opus_frame.is_empty());
    }
}
