use async_channel::{unbounded, Receiver, Sender};
use dashmap::DashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;
use tokio::net::UdpSocket;
use tokio::sync::oneshot;
use tracing::{debug, error};
use voiceapp_protocol::Packet;

use crate::error::SdkError;

/// Default timeout for request/response operations (per attempt)
const REQUEST_TIMEOUT_SECS: u64 = 5;

/// Default number of retry attempts for UDP requests
const MAX_RETRY_ATTEMPTS: u32 = 3;

/// UDP client for managing voice data communication
#[derive(Clone)]
pub struct UdpClient {
    send_tx: Sender<Vec<u8>>,
    send_rx: Receiver<Vec<u8>>,
    packet_tx: Sender<Packet>,
    packet_rx: Receiver<Packet>,
    pending_requests: Arc<DashMap<u64, oneshot::Sender<Packet>>>,
    bytes_sent: Arc<AtomicU64>,
    bytes_received: Arc<AtomicU64>,
}

impl UdpClient {
    /// Create a new UdpClient
    pub fn new() -> Self {
        let (send_tx, send_rx) = unbounded();
        let (packet_tx, packet_rx) = unbounded();

        Self {
            send_tx,
            send_rx,
            packet_tx,
            packet_rx,
            pending_requests: Arc::new(DashMap::new()),
            bytes_sent: Arc::new(AtomicU64::new(0)),
            bytes_received: Arc::new(AtomicU64::new(0)),
        }
    }

    /// Get current stats (bytes_sent, bytes_received)
    pub fn get_stats(&self) -> (u64, u64) {
        (
            self.bytes_sent.load(Ordering::Relaxed),
            self.bytes_received.load(Ordering::Relaxed),
        )
    }

    /// Get sender for outgoing packets
    pub fn packet_sender(&self) -> Sender<Vec<u8>> {
        self.send_tx.clone()
    }

    /// Get receiver for incoming packets
    pub fn packet_receiver(&self) -> Receiver<Packet> {
        self.packet_rx.clone()
    }

    /// Connect UDP socket to server and spawn handler
    pub async fn connect(&self, addr: &str) -> Result<(), SdkError> {
        // Create UDP socket (bind to any available port)
        let socket = UdpSocket::bind("0.0.0.0:0")
            .await
            .map_err(|e| SdkError::ConnectionFailed(format!("UDP bind failed: {}", e)))?;

        // Connect socket to server address
        socket
            .connect(addr)
            .await
            .map_err(|e| SdkError::ConnectionFailed(format!("UDP connect failed: {}", e)))?;

        debug!("UDP connected to {}", addr);

        self.spawn_handler(socket);
        Ok(())
    }

    /// Send request packet and wait for decoded response with retry logic
    pub async fn send_request_with_response<T, F>(
        &self,
        request: Packet,
        decoder: F,
    ) -> Result<T, SdkError>
    where
        F: Fn(Packet) -> Result<T, String>,
    {
        // Extract request_id from the packet
        let request_id = request.request_id()
            .ok_or_else(|| SdkError::ConnectionFailed("Packet does not have request_id".to_string()))?;

        for attempt in 1..=MAX_RETRY_ATTEMPTS {
            debug!(
                "[UDP] Sending request with response (attempt {}/{})",
                attempt, MAX_RETRY_ATTEMPTS
            );

            // Create oneshot channel for this specific request
            let (response_tx, response_rx) = oneshot::channel();

            // Register the oneshot sender in pending requests map
            self.pending_requests.insert(request_id, response_tx);

            // Send request
            self.send_tx
                .send(request.encode())
                .await
                .map_err(|_| SdkError::Disconnected)?;

            // Wait for response with timeout
            let timeout_result = tokio::time::timeout(
                Duration::from_secs(REQUEST_TIMEOUT_SECS),
                response_rx,
            )
            .await;

            match timeout_result {
                Ok(Ok(packet)) => {
                    debug!("[UDP] Request with response successful");
                    // Clean up pending request
                    self.pending_requests.remove(&request_id);
                    return decoder(packet)
                        .map_err(SdkError::ConnectionFailed);
                }
                Ok(Err(_)) => {
                    // Oneshot channel closed (handler stopped)
                    self.pending_requests.remove(&request_id);
                    return Err(SdkError::Disconnected);
                }
                Err(_) => {
                    debug!("[UDP] Request timeout on attempt {}", attempt);
                    // Clean up pending request on timeout
                    self.pending_requests.remove(&request_id);
                    if attempt < MAX_RETRY_ATTEMPTS {
                        tokio::time::sleep(Duration::from_millis(100)).await;
                    }
                }
            }
        }

        Err(SdkError::Timeout(format!(
            "UDP request failed after {} attempts",
            MAX_RETRY_ATTEMPTS
        )))
    }

    /// Spawn UDP handler task
    fn spawn_handler(&self, socket: UdpSocket) {
        let send_rx = self.send_rx.clone();
        let packet_tx = self.packet_tx.clone();
        let pending_requests = self.pending_requests.clone();
        let bytes_sent = self.bytes_sent.clone();
        let bytes_received = self.bytes_received.clone();

        tokio::spawn(async move {
            let mut read_buf = [0u8; 4096];

            loop {
                tokio::select! {
                    // Handle outgoing packets
                    result = send_rx.recv() => {
                        if let Err(e) = Self::handle_outgoing(&socket, result, &bytes_sent).await {
                            error!("UDP handler error: {}", e);
                            break;
                        }
                    }

                    // Handle incoming packets
                    result = socket.recv(&mut read_buf) => {
                        match Self::handle_incoming(
                            result,
                            &read_buf,
                            &packet_tx,
                            &pending_requests,
                            &bytes_received
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
        bytes_sent: &Arc<AtomicU64>,
    ) -> Result<(), String> {
        match recv_result {
            Ok(packet) => {
                let len = packet.len();
                socket
                    .send(&packet)
                    .await
                    .map_err(|e| format!("Send error: {}", e))?;
                bytes_sent.fetch_add(len as u64, Ordering::Relaxed);
                Ok(())
            }
            Err(_) => Err("Send channel closed".to_string()),
        }
    }

    /// Handle incoming packet, returns Ok(should_continue)
    async fn handle_incoming(
        read_result: std::io::Result<usize>,
        read_buf: &[u8],
        packet_tx: &Sender<Packet>,
        pending_requests: &Arc<DashMap<u64, oneshot::Sender<Packet>>>,
        bytes_received: &Arc<AtomicU64>,
    ) -> Result<bool, String> {
        match read_result {
            Ok(0) => {
                debug!("UDP socket closed");
                Ok(false)
            }
            Ok(n) => {
                bytes_received.fetch_add(n as u64, Ordering::Relaxed);
                // Parse incoming packet
                let (packet, _size) = Packet::decode(&read_buf[..n])
                    .map_err(|e| format!("Parse error: {}", e))?;

                // Extract request_id from response packets
                let request_id = match &packet {
                    Packet::VoiceAuthResponse { request_id, .. } => Some(*request_id),
                    _ => None,
                };

                // Check if this packet is a response to a pending request
                if let Some(req_id) = request_id {
                    if let Some((_, response_tx)) = pending_requests.remove(&req_id) {
                        // Send to the oneshot channel for the specific request
                        let _ = response_tx.send(packet);
                        return Ok(true);
                    }
                }

                // Not a pending request response, broadcast to general packet channel
                let _ = packet_tx.send(packet).await;

                Ok(true)
            }
            Err(e) => Err(format!("Receive error: {}", e)),
        }
    }
}
