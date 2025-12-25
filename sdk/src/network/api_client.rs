use std::sync::atomic::{AtomicU64, Ordering};
use tracing::info;
use voiceapp_protocol::Packet;

use crate::error::ClientError;
use super::tcp_client::TcpClient;
use super::udp_client::UdpClient;

/// ServerApi handles all request/response communication with the server
pub struct ApiClient {
    tcp_client: TcpClient,
    udp_client: UdpClient,
    request_id_counter: AtomicU64,
}

impl ApiClient {
    /// Create a new ServerApi with TCP and UDP clients
    pub fn new(tcp_client: TcpClient, udp_client: UdpClient) -> Self {
        Self {
            tcp_client,
            udp_client,
            request_id_counter: AtomicU64::new(1),
        }
    }

    /// Generate a unique request ID
    fn next_request_id(&self) -> u64 {
        self.request_id_counter.fetch_add(1, Ordering::Relaxed)
    }

    /// Authenticate with management server via TCP
    /// Returns the voice_token needed for UDP voice authentication
    pub async fn authenticate_management(&self, username: &str) -> Result<u64, ClientError> {
        let request_id = self.next_request_id();
        let request = Packet::LoginRequest {
            request_id,
            username: username.to_string(),
        };

        let response = self
            .tcp_client
            .send_request_with_response(
                request,
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
    pub async fn authenticate_voice(&self, voice_token: u64) -> Result<(), ClientError> {
        let request_id = self.next_request_id();
        let request = Packet::VoiceAuthRequest { request_id, voice_token };

        let success = self
            .udp_client
            .send_request_with_response(
                request,
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
            return Err(ClientError::ConnectionFailed(
                "Voice auth denied".to_string(),
            ));
        }

        info!("[Voice server] Authenticated successfully");

        Ok(())
    }

    /// Join voice channel
    pub async fn join_channel(&self) -> Result<(), ClientError> {
        let request_id = self.next_request_id();
        let request = Packet::JoinVoiceChannelRequest { request_id };

        self.tcp_client.send_request(request).await?;

        Ok(())
    }

    /// Leave voice channel
    pub async fn leave_channel(&self) -> Result<(), ClientError> {
        let request_id = self.next_request_id();
        let request = Packet::LeaveVoiceChannelRequest { request_id };

        self.tcp_client.send_request(request).await?;

        Ok(())
    }

    /// Send chat message
    pub async fn send_message(&self, message: &str) -> Result<(), ClientError> {
        let request_id = self.next_request_id();
        let request = Packet::ChatMessageRequest {
            request_id,
            message: message.to_string(),
        };

        self.tcp_client.send_request(request).await?;

        Ok(())
    }

    /// Send mute state (event, no response expected)
    pub async fn send_mute_state(&self, user_id: u64, is_muted: bool) -> Result<(), ClientError> {
        let packet = Packet::UserMuteState { user_id, is_muted };

        self.tcp_client.send_event(packet).await?;

        Ok(())
    }
}
