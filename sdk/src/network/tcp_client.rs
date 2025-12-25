use async_channel::{unbounded, Receiver, Sender};
use std::collections::HashMap;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::oneshot;
use tracing::{debug, error};
use voiceapp_protocol::Packet;

use crate::error::ClientError;

/// Default timeout for request/response operations
const REQUEST_TIMEOUT_SECS: u64 = 5;

type RequestCallback = (u64, oneshot::Sender<Packet>);

/// TCP client for managing request/response communication
#[derive(Clone)]
pub struct TcpClient {
    send_tx: Sender<(Vec<u8>, Option<RequestCallback>)>,
    send_rx: Receiver<(Vec<u8>, Option<RequestCallback>)>,
    packet_tx: Sender<Packet>,
    packet_rx: Receiver<Packet>,
}

impl TcpClient {
    /// Create a new TcpClient
    pub fn new() -> Self {
        let (send_tx, send_rx) = unbounded();
        let (packet_tx, packet_rx) = unbounded();

        Self {
            send_tx,
            send_rx,
            packet_tx,
            packet_rx,
        }
    }

    /// Get a receiver for incoming events
    pub fn packet_stream(&self) -> Receiver<Packet> {
        self.packet_rx.clone()
    }

    /// Connect to TCP server and spawn handler
    pub async fn connect(&self, addr: &str) -> Result<(), ClientError> {
        debug!("TCP connect to {}", addr);
        let socket = tokio::time::timeout(Duration::from_secs(REQUEST_TIMEOUT_SECS), TcpStream::connect(addr))
            .await
            .map_err(|_| ClientError::ConnectionFailed("Operation timed out".to_string()))?
            .map_err(|e| ClientError::ConnectionFailed(e.to_string()))?;

        debug!("TCP connected to {}", addr);

        self.spawn_handler(socket);
        Ok(())
    }

    /// Send packet without waiting for response (for events)
    pub async fn send_event(
        &self,
        packet: Packet,
    ) -> Result<(), ClientError> {
        // Encode packet
        let encoded = packet.encode();

        // Send packet without callback (no response expected)
        self.send_tx
            .send((encoded, None))
            .await
            .map_err(|_| ClientError::Disconnected)?;

        Ok(())
    }

    /// Send request packet and wait for response
    pub async fn send_request(
        &self,
        request: Packet,
    ) -> Result<Packet, ClientError> {
        // Extract request_id from the packet
        let request_id = request.request_id()
            .ok_or_else(|| ClientError::ConnectionFailed("Packet does not have request_id".to_string()))?;

        let (response_tx, response_rx) = oneshot::channel();

        // Encode packet
        let encoded = request.encode();

        // Send request with callback registered for expected response
        self.send_tx
            .send((encoded, Some((request_id, response_tx))))
            .await
            .map_err(|_| ClientError::Disconnected)?;

        // Wait for response with timeout
        self.wait_for_response(response_rx, request_id)
            .await
    }

    /// Send request packet and wait for decoded response
    pub async fn send_request_with_response<T, F>(
        &self,
        request: Packet,
        decoder: F,
    ) -> Result<T, ClientError>
    where
        F: Fn(Packet) -> Result<T, String>,
    {
        let packet = self.send_request(request).await?;
        decoder(packet).map_err(|e| ClientError::ConnectionFailed(e))
    }

    /// Spawn TCP handler task
    fn spawn_handler(&self, mut socket: TcpStream) {
        let send_rx = self.send_rx.clone();
        let packet_tx = self.packet_tx.clone();

        tokio::spawn(async move {
            let mut read_buf = [0u8; 4096];
            let mut pending_responses: HashMap<u64, oneshot::Sender<Packet>> =
                HashMap::new();
            // Buffer to accumulate partial packets across reads
            let mut accumulator: Vec<u8> = Vec::new();

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
                            &packet_tx,
                            &mut accumulator
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
        recv_result: Result<(Vec<u8>, Option<RequestCallback>), async_channel::RecvError>,
        pending_responses: &mut HashMap<u64, oneshot::Sender<Packet>>,
    ) -> Result<(), String> {
        match recv_result {
            Ok((packet, response_callback)) => {
                // Register callback if provided
                if let Some((expected_request_id, tx)) = response_callback {
                    pending_responses.insert(expected_request_id, tx);
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
        pending_responses: &mut HashMap<u64, oneshot::Sender<Packet>>,
        packet_tx: &Sender<Packet>,
        accumulator: &mut Vec<u8>,
    ) -> Result<bool, String> {
        match read_result {
            Ok(0) => { Ok(false) }
            Ok(n) => {
                // Append new data to accumulator
                accumulator.extend_from_slice(&read_buf[..n]);

                // Parse all complete packets from the accumulator
                loop {
                    match Packet::decode(accumulator) {
                        Ok((packet, size)) => {
                            let packet_id = packet.id();

                            debug!("Received TCP packet: ID 0x{:02x}", packet_id);

                            if let Some(req_id) = packet.request_id() {
                                if let Some(tx) = pending_responses.remove(&req_id) {
                                    let _ = tx.send(packet.clone());
                                }
                            }

                            let _ = packet_tx.send(packet).await;

                            // Remove the processed packet from the accumulator
                            accumulator.drain(..size);
                        }
                        Err(voiceapp_protocol::ProtocolError::IncompletePayload { .. }) |
                        Err(voiceapp_protocol::ProtocolError::PacketTooShort { .. }) => {
                            // Wait for more data
                            break;
                        }
                        Err(e) => {
                            error!("Parse error: {}, clearing buffer", e);
                            accumulator.clear();
                            break;
                        }
                    }
                }

                Ok(true)
            }
            Err(e) => Err(format!("Read error: {}", e)),
        }
    }

    /// Wait for response with timeout
    async fn wait_for_response(
        &self,
        response_rx: oneshot::Receiver<Packet>,
        request_id: u64,
    ) -> Result<Packet, ClientError> {
        let timeout = tokio::time::timeout(Duration::from_secs(REQUEST_TIMEOUT_SECS), response_rx);

        match timeout.await {
            Ok(Ok(packet)) => Ok(packet),
            Ok(Err(_)) => Err(ClientError::Disconnected),
            Err(_) => Err(ClientError::Timeout(format!(
                "request_id {}",
                request_id
            ))),
        }
    }
}
