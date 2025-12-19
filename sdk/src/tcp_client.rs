use async_channel::{unbounded, Receiver, Sender};
use std::collections::HashMap;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::oneshot;
use tracing::{debug, error};
use voiceapp_protocol::PacketId;

use crate::voice_client::VoiceClientError;

/// Default timeout for request/response operations
const REQUEST_TIMEOUT_SECS: u64 = 5;

type RequestCallback = (PacketId, oneshot::Sender<Vec<u8>>);

/// TCP client for managing request/response communication
pub struct TcpClient {
    send_tx: Sender<(Vec<u8>, Option<RequestCallback>)>,
    send_rx: Receiver<(Vec<u8>, Option<RequestCallback>)>,
    event_tx: Sender<(PacketId, Vec<u8>)>,
    event_rx: Receiver<(PacketId, Vec<u8>)>,
}

impl TcpClient {
    /// Create a new TcpClient
    pub fn new() -> Self {
        let (send_tx, send_rx) = unbounded();
        let (event_tx, event_rx) = unbounded();
        Self {
            send_tx,
            send_rx,
            event_tx,
            event_rx,
        }
    }

    /// Get a receiver for incoming events
    pub fn event_stream(&self) -> Receiver<(PacketId, Vec<u8>)> {
        self.event_rx.clone()
    }

    /// Connect to TCP server and spawn handler
    pub async fn connect(&self, addr: &str) -> Result<(), VoiceClientError> {
        let socket = tokio::time::timeout(Duration::from_secs(5), TcpStream::connect(addr))
            .await
            .map_err(|_| VoiceClientError::ConnectionFailed("Operation timed out".to_string()))?
            .map_err(|e| VoiceClientError::ConnectionFailed(e.to_string()))?;

        debug!("TCP connected to {}", addr);

        self.spawn_handler(socket);
        Ok(())
    }

    /// Send request and wait for acknowledgment
    pub async fn send_request(
        &self,
        request: Vec<u8>,
        expected_response_id: PacketId,
    ) -> Result<(), VoiceClientError> {
        let (response_tx, response_rx) = oneshot::channel();

        // Send request with callback registered for expected response
        self.send_tx
            .send((request, Some((expected_response_id, response_tx))))
            .await
            .map_err(|_| VoiceClientError::Disconnected)?;

        // Wait for response with timeout
        self.wait_for_response(response_rx, expected_response_id)
            .await?;

        Ok(())
    }

    /// Send request and wait for decoded response
    pub async fn send_request_with_response<T, F>(
        &self,
        request: Vec<u8>,
        expected_response_id: PacketId,
        decoder: F,
    ) -> Result<T, VoiceClientError>
    where
        F: Fn(&[u8]) -> std::io::Result<T>,
    {
        let (response_tx, response_rx) = oneshot::channel();

        // Send request with callback registered for expected response
        self.send_tx
            .send((request, Some((expected_response_id, response_tx))))
            .await
            .map_err(|_| VoiceClientError::Disconnected)?;

        // Wait for response with timeout
        let payload = self
            .wait_for_response(response_rx, expected_response_id)
            .await?;

        // Decode payload
        decoder(&payload).map_err(|e| VoiceClientError::ConnectionFailed(e.to_string()))
    }

    /// Spawn TCP handler task
    fn spawn_handler(&self, mut socket: TcpStream) {
        let send_rx = self.send_rx.clone();
        let event_tx = self.event_tx.clone();

        tokio::spawn(async move {
            let mut read_buf = [0u8; 4096];
            let mut pending_responses: HashMap<PacketId, oneshot::Sender<Vec<u8>>> =
                HashMap::new();

            loop {
                tokio::select! {
                    // Handle outgoing packets
                    result = send_rx.recv() => {
                        if let Err(e) = Self::handle_outgoing(
                            &mut socket,
                            result,
                            &mut pending_responses
                        ).await {
                            error!("TCP handler error: {}", e);
                            break;
                        }
                    }

                    // Handle incoming packets
                    result = socket.read(&mut read_buf) => {
                        match Self::handle_incoming(
                            result,
                            &read_buf,
                            &mut pending_responses,
                            &event_tx
                        ).await {
                            Ok(should_continue) => {
                                if !should_continue {
                                    break;
                                }
                            }
                            Err(e) => {
                                error!("TCP handler error: {}", e);
                                break;
                            }
                        }
                    }
                }
            }

            debug!("TCP handler stopped");
        });
    }

    /// Handle outgoing packet
    async fn handle_outgoing(
        socket: &mut TcpStream,
        recv_result: Result<
            (Vec<u8>, Option<(PacketId, oneshot::Sender<Vec<u8>>)>),
            async_channel::RecvError,
        >,
        pending_responses: &mut HashMap<PacketId, oneshot::Sender<Vec<u8>>>,
    ) -> Result<(), String> {
        match recv_result {
            Ok((packet, response_callback)) => {
                // Register callback if provided
                if let Some((expected_packet_id, tx)) = response_callback {
                    pending_responses.insert(expected_packet_id, tx);
                }

                // Send packet to socket
                socket
                    .write_all(&packet)
                    .await
                    .map_err(|e| format!("Write error: {}", e))?;

                Ok(())
            }
            Err(_) => Err("Send channel closed".to_string()),
        }
    }

    /// Handle incoming packet, returns Ok(should_continue)
    async fn handle_incoming(
        read_result: std::io::Result<usize>,
        read_buf: &[u8],
        pending_responses: &mut HashMap<PacketId, oneshot::Sender<Vec<u8>>>,
        event_tx: &Sender<(PacketId, Vec<u8>)>,
    ) -> Result<bool, String> {
        match read_result {
            Ok(0) => {
                // Connection closed
                Ok(false)
            }
            Ok(n) => {
                // Parse packet
                let (packet_id, payload) = voiceapp_protocol::parse_packet(&read_buf[..n])
                    .map_err(|e| format!("Parse error: {}", e))?;

                debug!("Received TCP packet: {:?}", packet_id);

                Self::route_packet(packet_id, payload, pending_responses, event_tx).await;

                Ok(true)
            }
            Err(e) => Err(format!("Read error: {}", e)),
        }
    }

    /// Route packet to appropriate handler
    async fn route_packet(
        packet_id: PacketId,
        payload: &[u8],
        pending_responses: &mut HashMap<PacketId, oneshot::Sender<Vec<u8>>>,
        event_tx: &Sender<(PacketId, Vec<u8>)>,
    ) {
        // Check if this is a pending response
        if let Some(tx) = pending_responses.remove(&packet_id) {
            let _ = tx.send(payload.to_vec());
            return;
        }

        // Send to event stream (VoiceClient will filter for actual events)
        let _ = event_tx.send((packet_id, payload.to_vec())).await;
    }

    /// Wait for response with timeout
    async fn wait_for_response(
        &self,
        response_rx: oneshot::Receiver<Vec<u8>>,
        expected_id: PacketId,
    ) -> Result<Vec<u8>, VoiceClientError> {
        let timeout = tokio::time::timeout(Duration::from_secs(REQUEST_TIMEOUT_SECS), response_rx);

        match timeout.await {
            Ok(Ok(payload)) => Ok(payload),
            Ok(Err(_)) => Err(VoiceClientError::Disconnected),
            Err(_) => Err(VoiceClientError::Timeout(format!(
                "packet {}",
                expected_id.as_u8()
            ))),
        }
    }
}
