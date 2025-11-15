use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{RwLock, mpsc};
use tracing::info;
use voiceapp_common::VoiceData;

/// Channel for sending voice packets to a user's stream processor
pub type VoicePacketSender = mpsc::UnboundedSender<VoiceData>;

/// Manages voice packet routing for all connected remote users
pub struct UserVoiceStreamManager {
    senders: Arc<RwLock<HashMap<String, VoicePacketSender>>>,
}

impl UserVoiceStreamManager {
    /// Create a new stream manager
    pub fn new() -> Self {
        UserVoiceStreamManager {
            senders: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Register a user's voice packet sender
    pub async fn register_sender(&self, username: String, sender: VoicePacketSender) -> Result<(), String> {
        let mut senders = self.senders.write().await;

        if senders.contains_key(&username) {
            return Err(format!("Sender for {} already exists", username));
        }

        senders.insert(username.clone(), sender);
        info!("Registered voice sender for {}", username);

        Ok(())
    }

    /// Unregister a user's voice packet sender
    pub async fn unregister_sender(&self, username: &str) -> Result<(), String> {
        let mut senders = self.senders.write().await;

        if senders.remove(username).is_none() {
            return Err(format!("No sender found for {}", username));
        }

        info!("Unregistered voice sender for {}", username);

        Ok(())
    }

    /// Send a voice packet to a specific user (non-blocking)
    pub async fn process_packet(&self, username: &str, packet: &VoiceData) -> Result<(), String> {
        let senders = self.senders.read().await;

        let sender = senders
            .get(username)
            .ok_or_else(|| format!("No sender found for {}", username))?;

        // Clone packet and send it - this is synchronous
        let packet_clone = packet.clone();
        sender.send(packet_clone)
            .map_err(|_| format!("Failed to send packet to {}", username))?;

        Ok(())
    }

}
