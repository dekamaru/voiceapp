use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{RwLock, mpsc};
use tracing::{debug, error, info};
use crate::output::AudioOutputHandle;
use crate::opus_decode::{OpusDecoder, mono_to_stereo};
use voiceapp_common::VoicePacket;

/// Channel for sending voice packets to a user's stream processor
pub type VoicePacketSender = mpsc::UnboundedSender<VoicePacket>;

/// Manages audio playback for a single remote user
pub struct UserVoiceStream {
    username: String,
    output_handle: AudioOutputHandle,
    decoder: OpusDecoder,
}

impl UserVoiceStream {
    /// Create a new user voice stream
    pub fn new(username: String, output_handle: AudioOutputHandle) -> Result<Self, String> {
        let decoder = OpusDecoder::new()
            .map_err(|e| format!("Failed to create decoder: {}", e))?;

        Ok(UserVoiceStream {
            username,
            output_handle,
            decoder,
        })
    }

    /// Process and play a voice packet from the user
    pub async fn process_packet(&mut self, packet: &VoicePacket) -> Result<(), String> {
        // Decode Opus frame to mono F32
        let mono_samples = self.decoder.decode_frame(&packet.opus_frame)?;

        // Convert mono to stereo
        let stereo_samples = mono_to_stereo(&mono_samples);

        // Queue for playback
        if let Err(e) = self.output_handle.sender().send(stereo_samples).await {
            error!("Failed to queue audio for {}: {}", self.username, e);
            return Err(format!("Failed to queue audio: {}", e));
        }

        debug!("Processed packet from {}", self.username);

        Ok(())
    }

    /// Get username
    pub fn username(&self) -> &str {
        &self.username
    }
}

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
    pub async fn process_packet(&self, username: &str, packet: &VoicePacket) -> Result<(), String> {
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

    /// Check if a user has an active sender
    pub async fn has_sender(&self, username: &str) -> bool {
        let senders = self.senders.read().await;
        senders.contains_key(username)
    }

    /// Get list of users with active senders
    pub async fn get_active_users(&self) -> Vec<String> {
        let senders = self.senders.read().await;
        senders.keys().cloned().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_manager_register_unregister() {
        let manager = UserVoiceStreamManager::new();

        // Initially no senders
        assert!(!manager.has_sender("alice").await);

        // Register a sender
        let (tx, _rx) = mpsc::unbounded_channel();
        assert!(manager.register_sender("alice".to_string(), tx).await.is_ok());
        assert!(manager.has_sender("alice").await);

        // Unregister
        assert!(manager.unregister_sender("alice").await.is_ok());
        assert!(!manager.has_sender("alice").await);
    }

    #[tokio::test]
    async fn test_get_active_users() {
        let manager = UserVoiceStreamManager::new();

        let active = manager.get_active_users().await;
        assert_eq!(active.len(), 0);

        // Add a sender
        let (tx, _rx) = mpsc::unbounded_channel();
        manager.register_sender("alice".to_string(), tx).await.unwrap();

        let active = manager.get_active_users().await;
        assert_eq!(active.len(), 1);
        assert!(active.contains(&"alice".to_string()));
    }
}
