use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;
use tracing::info;
use voiceapp_protocol::Packet;

use crate::error::SdkError;
use super::tcp_client::TcpClient;
use super::udp_client::UdpClient;

/// ServerApi handles all request/response communication with the server
pub(crate) struct ApiClient {
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
    /// Returns the user_id and voice_token needed for UDP voice authentication
    pub async fn authenticate_management(&self, username: &str) -> Result<(u64, u64), SdkError> {
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

        info!("[Management server] Authenticated, user_id={}", response.0);

        Ok(response)
    }

    /// Authenticate with voice server via UDP
    pub async fn authenticate_voice(&self, voice_token: u64) -> Result<(), SdkError> {
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
            return Err(SdkError::ConnectionFailed(
                "Voice auth denied".to_string(),
            ));
        }

        info!("[Voice server] Authenticated successfully");

        Ok(())
    }

    /// Join voice channel
    pub async fn join_channel(&self) -> Result<(), SdkError> {
        let request_id = self.next_request_id();
        let request = Packet::JoinVoiceChannelRequest { request_id };

        self.tcp_client.send_request(request).await?;

        Ok(())
    }

    /// Leave voice channel
    pub async fn leave_channel(&self) -> Result<(), SdkError> {
        let request_id = self.next_request_id();
        let request = Packet::LeaveVoiceChannelRequest { request_id };

        self.tcp_client.send_request(request).await?;

        Ok(())
    }

    /// Send chat message
    pub async fn send_message(&self, message: &str) -> Result<(), SdkError> {
        let request_id = self.next_request_id();
        let request = Packet::ChatMessageRequest {
            request_id,
            message: message.to_string(),
        };

        self.tcp_client.send_request(request).await?;

        Ok(())
    }

    /// Send mute state (event, no response expected)
    pub async fn send_mute_state(&self, user_id: u64, is_muted: bool) -> Result<(), SdkError> {
        // TODO: this is bad from security perspective and we need to get rid of event as request
        let packet = Packet::UserMuteState { user_id, is_muted };

        self.tcp_client.send_event(packet).await?;

        Ok(())
    }

    /// Ping the management server and return round-trip time in milliseconds
    pub async fn ping(&self) -> Result<u64, SdkError> {
        let request_id = self.next_request_id();
        let request = Packet::PingRequest { request_id };

        let start = Instant::now();

        self.tcp_client
            .send_request_with_response(request, |packet| {
                if let Packet::PingResponse { .. } = packet {
                    Ok(())
                } else {
                    Err("Expected PingResponse packet".to_string())
                }
            })
            .await?;

        Ok(start.elapsed().as_millis() as u64)
    }
}
