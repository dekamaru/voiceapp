use async_channel::{Receiver, Sender};
use std::sync::{Arc, Mutex};
use tracing::info;

pub use crate::error::ClientError;
use crate::network::{TcpClient, UdpClient, EventHandler, ClientEvent, ApiClient};
use crate::{voice, Decoder};

/// The VoiceClient for managing voice connections
pub struct Client {
    tcp_client: TcpClient,
    udp_client: UdpClient,
    api_client: ApiClient,
    event_handler: EventHandler,
    voice_io_manager: Mutex<voice::io_manager::InputOutputManager>,
}

impl Client {
    pub fn new() -> Self {
        let tcp_client = TcpClient::new();
        let udp_client = UdpClient::new();
        let api_client = ApiClient::new(tcp_client.clone(), udp_client.clone());
        let event_handler = EventHandler::new();
        let voice_io_manager = Mutex::new(
            voice::io_manager::InputOutputManager::new(
                udp_client.packet_sender(),
                udp_client.packet_receiver()
            )
        );

        Client {
            tcp_client,
            udp_client,
            api_client,
            event_handler,
            voice_io_manager,
        }
    }

    /// Subscribe to the event stream from VoiceClient
    /// Returns a cloneable receiver that will receive all events from this point forward
    pub fn event_stream(&self) -> Receiver<ClientEvent> {
        self.event_handler.event_stream()
    }

    /// Returns stream which should be used for raw input samples sending
    pub fn get_voice_input_sender(&self, input_sample_rate: u32) -> Result<Sender<Vec<f32>>, ClientError> {
        let mut manager = self.voice_io_manager.lock()
            .map_err(|e| ClientError::VoiceInputOutputManagerError(format!("failed to get lock: {}", e)))?;

        let sender = manager.get_voice_input_sender(input_sample_rate)
            .map_err(|e| ClientError::VoiceInputOutputManagerError(format!("failed to get voice input sender: {}", e)))?;

        Ok(sender)
    }

    pub fn get_voice_output_for(&self, user_id: u64, output_sample_rate: usize) -> Result<Arc<Decoder>, ClientError> {
        let mut manager = self.voice_io_manager.lock()
            .map_err(|e| ClientError::VoiceInputOutputManagerError(format!("failed to get lock: {}", e)))?;

        let decoder = manager.get_voice_output_for(user_id, output_sample_rate as u32);

        Ok(decoder)
    }

    /// To cleanup voice decoder
    pub fn remove_voice_output_for(&self, user_id: u64) -> Result<(), ClientError> {
        let mut manager = self.voice_io_manager.lock()
            .map_err(|e| ClientError::VoiceInputOutputManagerError(format!("failed to get lock: {}", e)))?;

        manager.remove_voice_output_for(user_id);
        Ok(())
    }

    pub fn remove_all_voice_outputs(&self) -> Result<(), ClientError> {
        let mut manager = self.voice_io_manager.lock()
            .map_err(|e| ClientError::VoiceInputOutputManagerError(format!("failed to get lock: {}", e)))?;

        manager.remove_all_voice_outputs();
        Ok(())
    }

    pub async fn connect(
        &self,
        management_server_addr: &str,
        voice_server_addr: &str,
        username: &str,
    ) -> Result<(), ClientError> {
        // Connect TCP socket
        self.tcp_client.connect(management_server_addr).await?;
        self.event_handler.listen_to_packets(self.tcp_client.packet_stream());
        info!("[Management server] Connected to {}", management_server_addr);

        // Connect UDP socket
        self.udp_client.connect(voice_server_addr).await?;
        info!("[Voice server] Connected to {}", voice_server_addr);

        let voice_token = self.api_client.authenticate_management(username).await?;
        self.api_client.authenticate_voice(voice_token).await?;

        Ok(())
    }

    pub async fn join_channel(&self) -> Result<(), ClientError> {
        self.api_client.join_channel().await
    }

    pub async fn leave_channel(&self) -> Result<(), ClientError> {
        self.api_client.leave_channel().await
    }

    pub async fn send_message(&self, message: &str) -> Result<(), ClientError> {
        self.api_client.send_message(message).await
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
