use async_channel::{Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::sync::atomic::{AtomicU64, Ordering};
use tracing::info;

use crate::error::SdkError;
use crate::network::{TcpClient, UdpClient, EventHandler, ClientEvent, ApiClient};
use crate::voice;
use crate::voice::decoder::Decoder;

/// Voice communication client
pub struct Client {
    tcp_client: TcpClient,
    udp_client: UdpClient,
    api_client: ApiClient,
    event_handler: EventHandler,
    voice_io_manager: Mutex<voice::io_manager::InputOutputManager>,
    user_id: AtomicU64,
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
            user_id: AtomicU64::new(0),
        }
    }

    /// Subscribe to the event stream from VoiceClient
    /// Returns a cloneable receiver that will receive all events from this point forward
    pub fn event_stream(&self) -> Receiver<ClientEvent> {
        self.event_handler.event_stream()
    }

    /// Returns stream which should be used for raw input samples sending
    pub fn get_voice_input_sender(&self, input_sample_rate: u32) -> Result<Sender<Vec<f32>>, SdkError> {
        let mut manager = self.voice_io_manager.lock().map_err(|_| SdkError::LockError)?;
        let sender = manager.get_voice_input_sender(input_sample_rate)?;
        Ok(sender)
    }

    /// Get or create a voice output decoder for a specific user
    ///
    /// # Note
    /// This method blocks. Avoid calling from async contexts.
    pub fn get_or_create_voice_output(&self, user_id: u64, sample_rate: u32) -> Result<Arc<Decoder>, SdkError> {
        let mut manager = self.voice_io_manager.lock().map_err(|_| SdkError::LockError)?;
        let decoder = manager.get_or_create_voice_output(user_id, sample_rate)?;
        Ok(decoder)
    }

    /// To cleanup voice decoder
    pub fn remove_voice_output_for(&self, user_id: u64) -> Result<(), SdkError> {
        let mut manager = self.voice_io_manager.lock().map_err(|_| SdkError::LockError)?;
        manager.remove_voice_output_for(user_id);
        Ok(())
    }

    pub fn remove_all_voice_outputs(&self) -> Result<(), SdkError> {
        let mut manager = self.voice_io_manager.lock().map_err(|_| SdkError::LockError)?;
        manager.remove_all_voice_outputs();
        Ok(())
    }

    /// Connects to the management and voice servers, returns user_id.
    pub async fn connect(
        &self,
        management_server_addr: &str,
        voice_server_addr: &str,
        username: &str,
    ) -> Result<u64, SdkError> {
        // Connect TCP socket
        self.tcp_client.connect(management_server_addr).await?;
        self.event_handler.listen_to_packets(self.tcp_client.packet_stream());
        info!("[Management server] Connected to {}", management_server_addr);

        // Connect UDP socket
        self.udp_client.connect(voice_server_addr).await?;
        info!("[Voice server] Connected to {}", voice_server_addr);

        let (user_id, voice_token) = self.api_client.authenticate_management(username).await?;
        self.user_id.store(user_id, Ordering::Relaxed);
        self.api_client.authenticate_voice(voice_token).await?;

        Ok(user_id)
    }

    pub async fn join_channel(&self) -> Result<(), SdkError> {
        self.api_client.join_channel().await
    }

    pub async fn leave_channel(&self) -> Result<(), SdkError> {
        self.api_client.leave_channel().await
    }

    pub async fn send_message(&self, message: &str) -> Result<(), SdkError> {
        self.api_client.send_message(message).await
    }

    pub async fn send_mute_state(&self, is_muted: bool) -> Result<(), SdkError> {
        self.api_client.send_mute_state(self.user_id.load(Ordering::Relaxed), is_muted).await
    }

    /// Ping the management server and return round-trip time in milliseconds
    pub async fn ping(&self) -> Result<u64, SdkError> {
        self.api_client.ping().await
    }
}
