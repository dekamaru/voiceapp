use async_channel::{unbounded, Receiver, Sender};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{debug, error, info};
use voiceapp_protocol::{Packet, ParticipantInfo};

/// Events from the voice server
#[derive(Debug, Clone)]
pub enum ClientEvent {
    /// Initial participant list sent after successful connection
    ParticipantsList {
        user_id: u64,
        participants: Vec<ParticipantInfo>,
    },
    /// A user joined the server
    UserJoinedServer { user_id: u64, username: String },
    /// A user joined a voice channel
    UserJoinedVoice { user_id: u64 },
    /// A user left a voice channel
    UserLeftVoice { user_id: u64 },
    /// A user left the server
    UserLeftServer { user_id: u64 },
    /// A user sent a chat message
    UserSentMessage {
        user_id: u64,
        timestamp: u64,
        message: String,
    },
    /// A user's mute state changed
    UserMuteState {
        user_id: u64,
        is_muted: bool,
    },
}

/// Handles TCP event processing and emits client events
pub struct EventHandler {
    event_tx: Sender<ClientEvent>,
    event_rx: Receiver<ClientEvent>,
}

impl EventHandler {
    /// Create a new TCP event handler
    pub fn new() -> Self {
        let (event_tx, event_rx) = unbounded();
        
        Self {
            event_tx,
            event_rx,
        }
    }

    /// Get event stream for processed events
    pub fn event_stream(&self) -> Receiver<ClientEvent> {
        self.event_rx.clone()
    }

    /// Listen to incoming packets and process events
    pub fn listen_to_packets(&self, packet_rx: Receiver<Packet>) {
        let event_tx = self.event_tx.clone();

        tokio::spawn(async move {
            loop {
                match packet_rx.recv().await {
                    Ok(packet) => {
                        if let Err(e) = Self::handle_packet(packet, &event_tx).await {
                            error!("Event handling error: {}", e);
                        }
                    }
                    Err(_) => {
                        debug!("TCP event stream closed");
                        break;
                    }
                }
            }
        });
    }

    /// Handle individual packet based on type
    async fn handle_packet(packet: Packet, event_tx: &Sender<ClientEvent>) -> Result<(), String> {
        match packet {
            Packet::LoginResponse { request_id: _, id, voice_token: _, participants } => {
                Self::handle_login_response(id, participants, event_tx).await
            }
            Packet::UserJoinedServer { participant } => {
                Self::handle_user_joined_server(participant, event_tx).await
            }
            Packet::UserLeftServer { user_id } => {
                Self::handle_user_left_server(user_id, event_tx).await
            }
            Packet::UserJoinedVoice { user_id } => {
                Self::handle_user_joined_voice(user_id, event_tx).await
            }
            Packet::UserLeftVoice { user_id } => {
                Self::handle_user_left_voice(user_id, event_tx).await
            }
            Packet::UserSentMessage { user_id, timestamp, message } => {
                Self::handle_user_sent_message(user_id, timestamp, message, event_tx).await
            }
            Packet::UserMuteState { user_id, is_muted } => {
                Self::handle_user_mute_state(user_id, is_muted, event_tx).await
            }
            _ => { Ok(()) }
        }
    }

    async fn handle_login_response(
        id: u64,
        participants: Vec<ParticipantInfo>,
        event_tx: &Sender<ClientEvent>,
    ) -> Result<(), String> {
        if event_tx.send(ClientEvent::ParticipantsList { user_id: id, participants }).await.is_err() {
            tracing::warn!("channel closed");
        }

        debug!("Login successful: user_id={}", id);
        Ok(())
    }

    async fn handle_user_joined_server(
        participant: ParticipantInfo,
        event_tx: &Sender<ClientEvent>,
    ) -> Result<(), String> {
        let user_id = participant.user_id;
        let username = participant.username.clone();

        if event_tx.send(ClientEvent::UserJoinedServer { user_id, username }).await.is_err() {
            tracing::warn!("channel closed");
        }

        debug!("User joined server: id={}", user_id);
        Ok(())
    }

    async fn handle_user_left_server(
        user_id: u64,
        event_tx: &Sender<ClientEvent>,
    ) -> Result<(), String> {
        if event_tx.send(ClientEvent::UserLeftServer { user_id }).await.is_err() {
            tracing::warn!("channel closed");
        }

        debug!("User left server: id={}", user_id);
        Ok(())
    }

    async fn handle_user_joined_voice(
        user_id: u64,
        event_tx: &Sender<ClientEvent>,
    ) -> Result<(), String> {
        if event_tx.send(ClientEvent::UserJoinedVoice { user_id }).await.is_err() {
            tracing::warn!("channel closed");
        }

        debug!("User joined voice: id={}", user_id);
        Ok(())
    }

    async fn handle_user_left_voice(
        user_id: u64,
        event_tx: &Sender<ClientEvent>,
    ) -> Result<(), String> {
        if event_tx.send(ClientEvent::UserLeftVoice { user_id }).await.is_err() {
            tracing::warn!("channel closed");
        }

        debug!("User left voice: id={}", user_id);
        Ok(())
    }

    async fn handle_user_sent_message(
        user_id: u64,
        timestamp: u64,
        message: String,
        event_tx: &Sender<ClientEvent>,
    ) -> Result<(), String> {
        if event_tx.send(ClientEvent::UserSentMessage { user_id, timestamp, message }).await.is_err() {
            tracing::warn!("channel closed");
        }

        Ok(())
    }

    async fn handle_user_mute_state(
        user_id: u64,
        is_muted: bool,
        event_tx: &Sender<ClientEvent>,
    ) -> Result<(), String> {
        if event_tx.send(ClientEvent::UserMuteState { user_id, is_muted }).await.is_err() {
            tracing::warn!("channel closed");
        }

        debug!("User mute state changed: id={}, is_muted={}", user_id, is_muted);
        Ok(())
    }
}
