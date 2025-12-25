use async_channel::{Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::sync::atomic::{AtomicU64, Ordering};
use tracing::info;

use voiceapp_protocol::Packet;
pub use crate::error::VoiceClientError;
use crate::tcp_client::TcpClient;
use crate::udp_client::UdpClient;
use crate::event_handler::{EventHandler, VoiceClientEvent};
use crate::{VoiceDecoder, VoiceInputOutputManager};

/// The VoiceClient for managing voice connections
pub struct VoiceClient {
    tcp_client: TcpClient,
    udp_client: UdpClient,
    event_handler: EventHandler,
    voice_io_manager: Mutex<VoiceInputOutputManager>,
    request_id_counter: AtomicU64,
}

impl VoiceClient {
    /// Create a new VoiceClient with all channels initialized
    /// sample_rate: target output sample rate for the decoder (should match audio device)
    pub fn new() -> Result<Self, VoiceClientError> {
        // Create TCP client
        let tcp_client = TcpClient::new();
        let udp_client = UdpClient::new();
        let event_handler = EventHandler::new();
        let voice_io_manager = Mutex::new(
            VoiceInputOutputManager::new(
                udp_client.packet_sender(),
                udp_client.packet_receiver()
            )
        );

        // Create client
        Ok(VoiceClient {
            tcp_client,
            udp_client,
            event_handler,
            voice_io_manager,
            request_id_counter: AtomicU64::new(1),
        })
    }

    /// Generate a unique request ID
    fn next_request_id(&self) -> u64 {
        self.request_id_counter.fetch_add(1, Ordering::Relaxed)
    }

    /// Subscribe to the event stream from VoiceClient
    /// Returns a cloneable receiver that will receive all events from this point forward
    pub fn event_stream(&self) -> Receiver<VoiceClientEvent> {
        self.event_handler.event_stream()
    }

    /// Returns stream which should be used for raw input samples sending
    pub fn get_voice_input_sender(&self, input_sample_rate: usize) -> Result<Sender<Vec<f32>>, VoiceClientError> {
        let mut manager = self.voice_io_manager.lock()
            .map_err(|e| VoiceClientError::VoiceInputOutputManagerError(format!("failed to get lock: {}", e)))?;

        let sender = manager.get_voice_input_sender(input_sample_rate)
            .map_err(|e| VoiceClientError::VoiceInputOutputManagerError(format!("failed to get voice input sender: {}", e)))?;

        Ok(sender)
    }

    pub fn get_voice_output_for(&self, user_id: u64, output_sample_rate: usize) -> Result<Arc<VoiceDecoder>, VoiceClientError> {
        let mut manager = self.voice_io_manager.lock()
            .map_err(|e| VoiceClientError::VoiceInputOutputManagerError(format!("failed to get lock: {}", e)))?;

        let decoder = manager.get_voice_output_for(user_id, output_sample_rate as u32);

        Ok(decoder)
    }

    /// To cleanup voice decoder
    pub fn remove_voice_output_for(&self, user_id: u64) -> Result<(), VoiceClientError> {
        let mut manager = self.voice_io_manager.lock()
            .map_err(|e| VoiceClientError::VoiceInputOutputManagerError(format!("failed to get lock: {}", e)))?;

        manager.remove_voice_output_for(user_id);
        Ok(())
    }

    pub fn remove_all_voice_outputs(&self) -> Result<(), VoiceClientError> {
        let mut manager = self.voice_io_manager.lock()
            .map_err(|e| VoiceClientError::VoiceInputOutputManagerError(format!("failed to get lock: {}", e)))?;

        manager.remove_all_voice_outputs();
        Ok(())
    }

    pub async fn connect(
        &self,
        management_server_addr: &str,
        voice_server_addr: &str,
        username: &str,
    ) -> Result<(), VoiceClientError> {
        // Connect TCP socket
        self.tcp_client.connect(management_server_addr).await?;
        self.event_handler.listen_to_packets(self.tcp_client.packet_stream());
        info!("[Management server] Connected to {}", management_server_addr);

        // Connect UDP socket
        self.udp_client.connect(voice_server_addr).await?;
        info!("[Voice server] Connected to {}", voice_server_addr);

        let voice_token = self.authenticate_management(username).await?;
        self.authenticate_voice(voice_token).await?;

        Ok(())
    }

    /// Authenticate with management server via TCP
    async fn authenticate_management(&self, username: &str) -> Result<u64, VoiceClientError> {
        let request_id = self.next_request_id();
        let request = Packet::LoginRequest {
            request_id,
            username: username.to_string(),
        };

        let response = self
            .tcp_client
            .send_request_with_response(
                request.encode(),
                request_id,
                |packet| {
                    if let Packet::LoginResponse { request_id: _, id, voice_token, participants: _ } = packet {
                        Ok((id, voice_token))
                    } else {
                        Err("Expected LoginResponse packet".to_string())
                    }
                },
            )
            .await?;

        let (user_id, voice_token) = response;

        info!("[Management server] Authenticated, user_id={}", user_id);

        Ok(voice_token)
    }

    /// Authenticate with voice server via UDP
    async fn authenticate_voice(&self, voice_token: u64) -> Result<(), VoiceClientError> {
        let request_id = self.next_request_id();
        let request = Packet::VoiceAuthRequest { request_id, voice_token };

        let success = self
            .udp_client
            .send_request_with_response(
                request.encode(),
                request_id,
                |packet| {
                    if let Packet::VoiceAuthResponse { request_id: _, success } = packet {
                        Ok(success)
                    } else {
                        Err("Expected VoiceAuthResponse packet".to_string())
                    }
                },
            )
            .await?;

        if !success {
            return Err(VoiceClientError::ConnectionFailed(
                "Voice auth denied".to_string(),
            ));
        }

        info!("[Voice server] Authenticated successfully");

        Ok(())
    }

    pub async fn join_channel(&self) -> Result<(), VoiceClientError> {
        let request_id = self.next_request_id();
        let request = Packet::JoinVoiceChannelRequest { request_id };

        self.tcp_client.send_request(
            request.encode(),
            request_id,
        ).await?;

        Ok(())
    }

    pub async fn leave_channel(&self) -> Result<(), VoiceClientError> {
        let request_id = self.next_request_id();
        let request = Packet::LeaveVoiceChannelRequest { request_id };

        self.tcp_client.send_request(
            request.encode(),
            request_id,
        ).await?;
        
        Ok(())
    }

    pub async fn send_message(&self, message: &str) -> Result<(), VoiceClientError> {
        let request_id = self.next_request_id();
        let request = Packet::ChatMessageRequest {
            request_id,
            message: message.to_string(),
        };

        self.tcp_client.send_request(
            request.encode(),
            request_id,
        ).await?;
        
        Ok(())
    }

    /// Get list of user IDs currently in voice channel (blocking version)
    pub fn get_users_in_voice(&self) -> Vec<u64> {
        let state_arc = self.event_handler.state();
        let state = state_arc.blocking_read();
        state
            .participants
            .iter()
            .filter(|(_, info)| info.in_voice)
            .map(|(id, _)| *id)
            .collect()
    }

    pub fn is_in_voice_channel(&self) -> bool {
        let state_arc = self.event_handler.state();
        let state = state_arc.blocking_read();
        state.in_voice_channel
    }
}
