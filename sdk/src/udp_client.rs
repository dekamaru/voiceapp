use async_channel::{unbounded, Receiver, Sender};
use std::sync::Arc;
use std::time::Duration;
use tokio::net::UdpSocket;
use tracing::{debug, error};
use voiceapp_protocol::PacketId;

use crate::error::VoiceClientError;
use crate::voice_decoder_manager::VoiceDecoderManager;

/// Default timeout for request/response operations (per attempt)
const REQUEST_TIMEOUT_SECS: u64 = 5;

/// Default number of retry attempts for UDP requests
const MAX_RETRY_ATTEMPTS: u32 = 3;

/// UDP client for managing voice data communication
pub struct UdpClient {
    send_tx: Sender<Vec<u8>>,
    send_rx: Receiver<Vec<u8>>,
    packet_tx: Sender<(PacketId, Vec<u8>)>,
    packet_rx: Receiver<(PacketId, Vec<u8>)>,
    decoder_manager: Arc<VoiceDecoderManager>,
}

impl UdpClient {
    /// Create a new UdpClient
    pub fn new(decoder_manager: Arc<VoiceDecoderManager>) -> Self {
        let (send_tx, send_rx) = unbounded();
        let (packet_tx, packet_rx) = unbounded();

        Self {
            send_tx,
            send_rx,
            packet_tx,
            packet_rx,
            decoder_manager,
        }
    }

    /// Get sender for outgoing packets
    pub fn packet_sender(&self) -> Sender<Vec<u8>> {
        self.send_tx.clone()
    }

    /// Connect UDP socket to server and spawn handler
    pub async fn connect(&self, addr: &str) -> Result<(), VoiceClientError> {
        // Create UDP socket (bind to any available port)
        let socket = UdpSocket::bind("0.0.0.0:0")
            .await
            .map_err(|e| VoiceClientError::ConnectionFailed(format!("UDP bind failed: {}", e)))?;

        // Connect socket to server address
        socket
            .connect(addr)
            .await
            .map_err(|e| VoiceClientError::ConnectionFailed(format!("UDP connect failed: {}", e)))?;

        debug!("UDP connected to {}", addr);

        self.spawn_handler(socket);
        Ok(())
    }

    /// Send request and wait for response with retry logic
    pub async fn send_request(
        &self,
        request: Vec<u8>,
        expected_response_id: PacketId,
    ) -> Result<(), VoiceClientError> {
        let packet_rx = self.packet_rx.clone();

        for attempt in 1..=MAX_RETRY_ATTEMPTS {
            debug!(
                "[UDP] Sending request (attempt {}/{})",
                attempt, MAX_RETRY_ATTEMPTS
            );

            // Send request
            self.send_tx
                .send(request.clone())
                .await
                .map_err(|_| VoiceClientError::Disconnected)?;

            // Wait for response with timeout
            let timeout_result =
                tokio::time::timeout(Duration::from_secs(REQUEST_TIMEOUT_SECS), async {
                    loop {
                        match packet_rx.recv().await {
                            Ok((packet_id, _payload)) => {
                                if packet_id == expected_response_id {
                                    return Ok(());
                                }
                                // Ignore other packets, keep waiting for expected response
                            }
                            Err(_) => return Err(VoiceClientError::Disconnected),
                        }
                    }
                })
                .await;

            match timeout_result {
                Ok(Ok(())) => {
                    debug!("[UDP] Request successful");
                    return Ok(());
                }
                Ok(Err(e)) => return Err(e),
                Err(_) => {
                    debug!("[UDP] Request timeout on attempt {}", attempt);
                    if attempt < MAX_RETRY_ATTEMPTS {
                        tokio::time::sleep(Duration::from_millis(100)).await;
                    }
                }
            }
        }

        Err(VoiceClientError::Timeout(format!(
            "UDP request failed after {} attempts",
            MAX_RETRY_ATTEMPTS
        )))
    }

    /// Send request and wait for decoded response with retry logic
    pub async fn send_request_with_response<T, F>(
        &self,
        request: Vec<u8>,
        expected_response_id: PacketId,
        decoder: F,
    ) -> Result<T, VoiceClientError>
    where
        F: Fn(&[u8]) -> std::io::Result<T>,
    {
        let packet_rx = self.packet_rx.clone();

        for attempt in 1..=MAX_RETRY_ATTEMPTS {
            debug!(
                "[UDP] Sending request with response (attempt {}/{})",
                attempt, MAX_RETRY_ATTEMPTS
            );

            // Send request
            self.send_tx
                .send(request.clone())
                .await
                .map_err(|_| VoiceClientError::Disconnected)?;

            // Wait for response with timeout
            let timeout_result =
                tokio::time::timeout(Duration::from_secs(REQUEST_TIMEOUT_SECS), async {
                    loop {
                        match packet_rx.recv().await {
                            Ok((packet_id, payload)) => {
                                if packet_id == expected_response_id {
                                    return Ok(payload);
                                }
                                // Ignore other packets, keep waiting for expected response
                            }
                            Err(_) => return Err(VoiceClientError::Disconnected),
                        }
                    }
                })
                .await;

            match timeout_result {
                Ok(Ok(payload)) => {
                    debug!("[UDP] Request with response successful");
                    return decoder(&payload)
                        .map_err(|e| VoiceClientError::ConnectionFailed(e.to_string()));
                }
                Ok(Err(e)) => return Err(e),
                Err(_) => {
                    debug!("[UDP] Request timeout on attempt {}", attempt);
                    if attempt < MAX_RETRY_ATTEMPTS {
                        tokio::time::sleep(Duration::from_millis(100)).await;
                    }
                }
            }
        }

        Err(VoiceClientError::Timeout(format!(
            "UDP request failed after {} attempts",
            MAX_RETRY_ATTEMPTS
        )))
    }

    /// Spawn UDP handler task
    fn spawn_handler(&self, socket: UdpSocket) {
        let send_rx = self.send_rx.clone();
        let packet_tx = self.packet_tx.clone();
        let decoder_manager = Arc::clone(&self.decoder_manager);

        tokio::spawn(async move {
            let mut read_buf = [0u8; 4096];

            loop {
                tokio::select! {
                    // Handle outgoing packets
                    result = send_rx.recv() => {
                        if let Err(e) = Self::handle_outgoing(&socket, result).await {
                            error!("UDP handler error: {}", e);
                            break;
                        }
                    }

                    // Handle incoming packets
                    result = socket.recv(&mut read_buf) => {
                        match Self::handle_incoming(
                            result,
                            &read_buf,
                            &decoder_manager,
                            &packet_tx
                        ).await {
                            Ok(should_continue) => {
                                if !should_continue {
                                    break;
                                }
                            }
                            Err(e) => {
                                error!("UDP handler error: {}", e);
                                break;
                            }
                        }
                    }
                }
            }

            debug!("UDP handler stopped");
        });
    }

    /// Handle outgoing packet
    async fn handle_outgoing(
        socket: &UdpSocket,
        recv_result: Result<Vec<u8>, async_channel::RecvError>,
    ) -> Result<(), String> {
        match recv_result {
            Ok(packet) => {
                socket
                    .send(&packet)
                    .await
                    .map_err(|e| format!("Send error: {}", e))?;
                Ok(())
            }
            Err(_) => Err("Send channel closed".to_string()),
        }
    }

    /// Handle incoming packet, returns Ok(should_continue)
    async fn handle_incoming(
        read_result: std::io::Result<usize>,
        read_buf: &[u8],
        decoder_manager: &Arc<VoiceDecoderManager>,
        packet_tx: &Sender<(PacketId, Vec<u8>)>,
    ) -> Result<bool, String> {
        match read_result {
            Ok(0) => {
                debug!("UDP socket closed");
                Ok(false)
            }
            Ok(n) => {
                // Parse incoming packet
                let (packet_id, payload) = voiceapp_protocol::parse_packet(&read_buf[..n])
                    .map_err(|e| format!("Parse error: {}", e))?;

                // Handle voice data packets specially
                if packet_id == PacketId::VoiceData {
                    if let Ok(voice_packet) = voiceapp_protocol::decode_voice_data(&payload) {
                        if let Err(e) = decoder_manager.insert_packet(voice_packet).await {
                            debug!("Failed to insert voice packet: {}", e);
                        }
                    }
                } else {
                    // Route non-voice packets to packet stream
                    let _ = packet_tx.send((packet_id, payload.to_vec())).await;
                }

                Ok(true)
            }
            Err(e) => Err(format!("Receive error: {}", e)),
        }
    }
}
