use async_channel::{unbounded, Receiver, Sender};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{debug, error, info};
use voiceapp_protocol::{PacketId, ParticipantInfo};

use crate::voice_decoder_manager::VoiceDecoderManager;

/// Events emitted by VoiceClient when state changes occur
#[derive(Debug, Clone)]
pub enum VoiceClientEvent {
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
}

/// Client-side state for voice connection
pub struct ClientState {
    pub user_id: Option<u64>,
    pub participants: HashMap<u64, ParticipantInfo>,
    pub in_voice_channel: bool,
}

/// Handles TCP event processing and emits client events
pub struct EventHandler {
    state: Arc<RwLock<ClientState>>,
    decoder_manager: Arc<VoiceDecoderManager>,
    event_tx: Sender<VoiceClientEvent>,
    event_rx: Receiver<VoiceClientEvent>,
}

impl EventHandler {
    /// Create a new TCP event handler
    pub fn new(decoder_manager: Arc<VoiceDecoderManager>) -> Self {
        let (event_tx, event_rx) = unbounded();
        
        // Create initial empty state
        let state = Arc::new(RwLock::new(ClientState {
            user_id: None,
            participants: HashMap::new(),
            in_voice_channel: false,
        }));
        
        Self {
            state,
            decoder_manager,
            event_tx,
            event_rx,
        }
    }

    /// Get event stream for processed events
    pub fn event_stream(&self) -> Receiver<VoiceClientEvent> {
        self.event_rx.clone()
    }

    /// Get state accessor for read operations
    pub fn state(&self) -> Arc<RwLock<ClientState>> {
        Arc::clone(&self.state)
    }

    /// Listen to incoming packets and process events
    pub fn listen_to_packets(&self, packet_rx: Receiver<(PacketId, Vec<u8>)>) {
        let state = Arc::clone(&self.state);
        let decoder_manager = Arc::clone(&self.decoder_manager);
        let event_tx = self.event_tx.clone();

        tokio::spawn(async move {
            loop {
                match packet_rx.recv().await {
                    Ok((packet_id, payload)) => {
                        let result = Self::handle_packet(
                            packet_id,
                            &payload,
                            &state,
                            &decoder_manager,
                            &event_tx,
                        )
                        .await;

                        if let Err(e) = result {
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
    async fn handle_packet(
        packet_id: PacketId,
        payload: &[u8],
        state: &Arc<RwLock<ClientState>>,
        decoder_manager: &Arc<VoiceDecoderManager>,
        event_tx: &Sender<VoiceClientEvent>,
    ) -> Result<(), String> {
        match packet_id {
            PacketId::LoginResponse => {
                Self::handle_login_response(payload, state, event_tx).await
            }
            PacketId::JoinVoiceChannelResponse => {
                Self::handle_join_voice_channel_response(state).await
            }
            PacketId::LeaveVoiceChannelResponse => {
                Self::handle_leave_voice_channel_response(state).await
            }
            PacketId::UserJoinedServer => {
                Self::handle_user_joined_server(payload, state, event_tx).await
            }
            PacketId::UserLeftServer => {
                Self::handle_user_left_server(payload, state, event_tx).await
            }
            PacketId::UserJoinedVoice => {
                Self::handle_user_joined_voice(payload, state, event_tx).await
            }
            PacketId::UserLeftVoice => {
                Self::handle_user_left_voice(payload, state, decoder_manager, event_tx).await
            }
            PacketId::UserSentMessage => Self::handle_user_sent_message(payload, event_tx).await,
            _ => {
                // Ignore non-event packets
                debug!(
                    "Received non-event packet in event stream: {:?}",
                    packet_id
                );
                Ok(())
            }
        }
    }

    async fn handle_login_response(
        payload: &[u8],
        state: &Arc<RwLock<ClientState>>,
        event_tx: &Sender<VoiceClientEvent>,
    ) -> Result<(), String> {
        let response = voiceapp_protocol::decode_login_response(payload)
            .map_err(|e| format!("Decode error: {}", e))?;

        // Update client state with user_id and participants
        let mut s = state.write().await;
        s.user_id = Some(response.id);
        s.participants = response
            .participants
            .iter()
            .map(|p| (p.user_id, p.clone()))
            .collect();
        drop(s);

        // Emit ParticipantsList event
        let _ = event_tx
            .send(VoiceClientEvent::ParticipantsList {
                user_id: response.id,
                participants: response.participants,
            })
            .await;

        debug!("Login successful: user_id={}", response.id);
        Ok(())
    }

    async fn handle_join_voice_channel_response(
        state: &Arc<RwLock<ClientState>>,
    ) -> Result<(), String> {
        let mut s = state.write().await;
        s.in_voice_channel = true;
        drop(s);

        info!("Joined voice channel");
        Ok(())
    }

    async fn handle_leave_voice_channel_response(
        state: &Arc<RwLock<ClientState>>,
    ) -> Result<(), String> {
        let mut s = state.write().await;
        s.in_voice_channel = false;
        drop(s);

        info!("Left voice channel");
        Ok(())
    }

    async fn handle_user_joined_server(
        payload: &[u8],
        state: &Arc<RwLock<ClientState>>,
        event_tx: &Sender<VoiceClientEvent>,
    ) -> Result<(), String> {
        let (user_id, username) = voiceapp_protocol::decode_user_joined_server(payload)
            .map_err(|e| format!("Decode error: {}", e))?;

        let mut s = state.write().await;
        s.participants.insert(
            user_id,
            ParticipantInfo {
                user_id,
                username: username.clone(),
                in_voice: false,
            },
        );
        drop(s);

        let _ = event_tx
            .send(VoiceClientEvent::UserJoinedServer { user_id, username })
            .await;

        debug!("User joined server: id={}", user_id);
        Ok(())
    }

    async fn handle_user_left_server(
        payload: &[u8],
        state: &Arc<RwLock<ClientState>>,
        event_tx: &Sender<VoiceClientEvent>,
    ) -> Result<(), String> {
        let user_id = voiceapp_protocol::decode_user_left_server(payload)
            .map_err(|e| format!("Decode error: {}", e))?;

        let mut s = state.write().await;
        s.participants.remove(&user_id);
        drop(s);

        let _ = event_tx.send(VoiceClientEvent::UserLeftServer { user_id }).await;

        debug!("User left server: id={}", user_id);
        Ok(())
    }

    async fn handle_user_joined_voice(
        payload: &[u8],
        state: &Arc<RwLock<ClientState>>,
        event_tx: &Sender<VoiceClientEvent>,
    ) -> Result<(), String> {
        let user_id = voiceapp_protocol::decode_user_joined_voice(payload)
            .map_err(|e| format!("Decode error: {}", e))?;

        let mut s = state.write().await;
        if let Some(participant) = s.participants.get_mut(&user_id) {
            participant.in_voice = true;
        }
        drop(s);

        let _ = event_tx
            .send(VoiceClientEvent::UserJoinedVoice { user_id })
            .await;

        debug!("User joined voice: id={}", user_id);
        Ok(())
    }

    async fn handle_user_left_voice(
        payload: &[u8],
        state: &Arc<RwLock<ClientState>>,
        decoder_manager: &Arc<VoiceDecoderManager>,
        event_tx: &Sender<VoiceClientEvent>,
    ) -> Result<(), String> {
        let user_id = voiceapp_protocol::decode_user_left_voice(payload)
            .map_err(|e| format!("Decode error: {}", e))?;

        // Remove decoder for user who left
        decoder_manager.remove_user(user_id).await;

        let mut s = state.write().await;
        if let Some(participant) = s.participants.get_mut(&user_id) {
            participant.in_voice = false;
        }
        drop(s);

        let _ = event_tx
            .send(VoiceClientEvent::UserLeftVoice { user_id })
            .await;

        debug!("User left voice: id={}", user_id);
        Ok(())
    }

    async fn handle_user_sent_message(
        payload: &[u8],
        event_tx: &Sender<VoiceClientEvent>,
    ) -> Result<(), String> {
        let (user_id, timestamp, message) = voiceapp_protocol::decode_user_sent_message(payload)
            .map_err(|e| format!("Decode error: {}", e))?;

        let _ = event_tx
            .send(VoiceClientEvent::UserSentMessage {
                user_id,
                timestamp,
                message: message.clone(),
            })
            .await;

        debug!("User sent message: id={}, timestamp={}", user_id, timestamp);
        Ok(())
    }
}
