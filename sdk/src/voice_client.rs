use async_channel::{Receiver, Sender};
use std::sync::Arc;
use tracing::info;

use voiceapp_protocol::PacketId;
pub use crate::error::VoiceClientError;
use crate::tcp_client::TcpClient;
use crate::udp_client::UdpClient;
use crate::event_handler::{EventHandler, VoiceClientEvent};
use crate::voice_decoder_manager::VoiceDecoderManager;

/// The VoiceClient for managing voice connections
pub struct VoiceClient {
    tcp_client: TcpClient,
    udp_client: UdpClient,
    event_handler: EventHandler,
    decoder_manager: Arc<VoiceDecoderManager>,
}

impl VoiceClient {
    /// Create a new VoiceClient with all channels initialized
    /// sample_rate: target output sample rate for the decoder (should match audio device)
    pub fn new(output_sample_rate: u32) -> Result<Self, VoiceClientError> {
        // Create TCP client
        let tcp_client = TcpClient::new();
        let decoder_manager = Arc::new(VoiceDecoderManager::new(output_sample_rate));
        let udp_client = UdpClient::new(Arc::clone(&decoder_manager));
        let event_handler = EventHandler::new(Arc::clone(&decoder_manager));

        // Create client
        Ok(VoiceClient {
            tcp_client,
            udp_client,
            event_handler,
            decoder_manager,
        })
    }

    /// Subscribe to the event stream from VoiceClient
    /// Returns a cloneable receiver that will receive all events from this point forward
    pub fn event_stream(&self) -> Receiver<VoiceClientEvent> {
        self.event_handler.event_stream()
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
        let response = self
            .tcp_client
            .send_request_with_response(
                voiceapp_protocol::encode_login_request(username),
                PacketId::LoginResponse,
                voiceapp_protocol::decode_login_response,
            )
            .await?;

        let voice_token = response.voice_token;

        info!("[Management server] Authenticated, user_id={}", response.id);

        Ok(voice_token)
    }

    /// Authenticate with voice server via UDP
    async fn authenticate_voice(&self, voice_token: u64) -> Result<(), VoiceClientError> {
        let success = self
            .udp_client
            .send_request_with_response(
                voiceapp_protocol::encode_voice_auth_request(voice_token),
                PacketId::VoiceAuthResponse,
                voiceapp_protocol::decode_voice_auth_response,
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
        self.tcp_client.send_request(
            voiceapp_protocol::encode_join_voice_channel_request(),
            PacketId::JoinVoiceChannelResponse,
        ).await?;

        Ok(())
    }

    pub async fn leave_channel(&self) -> Result<(), VoiceClientError> {
        self.tcp_client.send_request(
            voiceapp_protocol::encode_leave_voice_channel_request(),
            PacketId::LeaveVoiceChannelResponse
        ).await?;
        
        Ok(())
    }

    pub async fn send_message(&self, message: &str) -> Result<(), VoiceClientError> {
        self.tcp_client.send_request(
            voiceapp_protocol::encode_chat_message_request(message),
            PacketId::ChatMessageResponse,
        ).await?;
        
        Ok(())
    }

    /// Get a receiver for decoded voice output (mono F32 PCM samples at 48kHz)
    /// Subscribe to incoming voice from other participants
    pub fn get_decoder_manager(&self) -> Arc<VoiceDecoderManager> {
        self.decoder_manager.clone()
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

    /// Get UDP send channel for AudioManager to forward encoded voice packets
    pub fn get_udp_send_tx(&self) -> Sender<Vec<u8>> {
        self.udp_client.packet_sender()
    }
}
