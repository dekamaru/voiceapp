use async_channel::{unbounded, Receiver, Sender};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{debug, error, info};
use voiceapp_protocol::{Packet, ParticipantInfo};

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
    event_tx: Sender<VoiceClientEvent>,
    event_rx: Receiver<VoiceClientEvent>,
}

impl EventHandler {
    /// Create a new TCP event handler
    pub fn new() -> Self {
        let (event_tx, event_rx) = unbounded();
        
        // Create initial empty state
        let state = Arc::new(RwLock::new(ClientState {
            user_id: None,
            participants: HashMap::new(),
            in_voice_channel: false,
        }));
        
        Self {
            state,
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
    pub fn listen_to_packets(&self, packet_rx: Receiver<Packet>) {
        let state = Arc::clone(&self.state);
        let event_tx = self.event_tx.clone();

        tokio::spawn(async move {
            loop {
                match packet_rx.recv().await {
                    Ok(packet) => {
                        let result = Self::handle_packet(
                            packet,
                            &state,
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
        packet: Packet,
        state: &Arc<RwLock<ClientState>>,
        event_tx: &Sender<VoiceClientEvent>,
    ) -> Result<(), String> {
        match packet {
            Packet::LoginResponse { request_id: _, id, voice_token: _, participants } => {
                Self::handle_login_response(id, participants, state, event_tx).await
            }
            Packet::JoinVoiceChannelResponse { request_id: _, success: _ } => {
                Self::handle_join_voice_channel_response(state).await
            }
            Packet::LeaveVoiceChannelResponse { request_id: _, success: _ } => {
                Self::handle_leave_voice_channel_response(state).await
            }
            Packet::UserJoinedServer { participant } => {
                Self::handle_user_joined_server(participant, state, event_tx).await
            }
            Packet::UserLeftServer { user_id } => {
                Self::handle_user_left_server(user_id, state, event_tx).await
            }
            Packet::UserJoinedVoice { user_id } => {
                Self::handle_user_joined_voice(user_id, state, event_tx).await
            }
            Packet::UserLeftVoice { user_id } => {
                Self::handle_user_left_voice(user_id, state, event_tx).await
            }
            Packet::UserSentMessage { user_id, timestamp, username: _, message } => {
                Self::handle_user_sent_message(user_id, timestamp, message, event_tx).await
            }
            _ => { Ok(()) }
        }
    }

    async fn handle_login_response(
        id: u64,
        participants: Vec<ParticipantInfo>,
        state: &Arc<RwLock<ClientState>>,
        event_tx: &Sender<VoiceClientEvent>,
    ) -> Result<(), String> {
        // Update client state with user_id and participants
        let mut s = state.write().await;
        s.user_id = Some(id);
        s.participants = participants
            .iter()
            .map(|p| (p.user_id, p.clone()))
            .collect();
        drop(s);

        // Emit ParticipantsList event
        let _ = event_tx
            .send(VoiceClientEvent::ParticipantsList {
                user_id: id,
                participants,
            })
            .await;

        debug!("Login successful: user_id={}", id);
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
        participant: ParticipantInfo,
        state: &Arc<RwLock<ClientState>>,
        event_tx: &Sender<VoiceClientEvent>,
    ) -> Result<(), String> {
        let user_id = participant.user_id;
        let username = participant.username.clone();

        let mut s = state.write().await;
        s.participants.insert(user_id, participant);
        drop(s);

        let _ = event_tx
            .send(VoiceClientEvent::UserJoinedServer { user_id, username })
            .await;

        debug!("User joined server: id={}", user_id);
        Ok(())
    }

    async fn handle_user_left_server(
        user_id: u64,
        state: &Arc<RwLock<ClientState>>,
        event_tx: &Sender<VoiceClientEvent>,
    ) -> Result<(), String> {
        let mut s = state.write().await;
        s.participants.remove(&user_id);
        drop(s);

        let _ = event_tx.send(VoiceClientEvent::UserLeftServer { user_id }).await;

        debug!("User left server: id={}", user_id);
        Ok(())
    }

    async fn handle_user_joined_voice(
        user_id: u64,
        state: &Arc<RwLock<ClientState>>,
        event_tx: &Sender<VoiceClientEvent>,
    ) -> Result<(), String> {
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
        user_id: u64,
        state: &Arc<RwLock<ClientState>>,
        event_tx: &Sender<VoiceClientEvent>,
    ) -> Result<(), String> {
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
        user_id: u64,
        timestamp: u64,
        message: String,
        event_tx: &Sender<VoiceClientEvent>,
    ) -> Result<(), String> {
        let _ = event_tx
            .send(VoiceClientEvent::UserSentMessage {
                user_id,
                timestamp,
                message,
            })
            .await;

        Ok(())
    }
}
