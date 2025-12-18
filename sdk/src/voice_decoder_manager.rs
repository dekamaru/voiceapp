use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;
use voiceapp_protocol::VoiceData;
use crate::voice_decoder::{VoiceDecoder, VoiceDecoderError};

/// Manages multiple VoiceDecoder instances, one per user (SSRC)
/// Each user gets their own NetEQ jitter buffer for independent audio processing
pub struct VoiceDecoderManager {
    sample_rate: u32,
    decoders: Arc<Mutex<HashMap<u64, Arc<VoiceDecoder>>>>,
}

impl VoiceDecoderManager {
    pub fn new(sample_rate: u32) -> Self {
        Self {
            sample_rate,
            decoders: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Insert a voice packet, routing to the correct per-user decoder
    /// Creates a new decoder if this is the first packet from this user
    pub async fn insert_packet(&self, packet: VoiceData) -> Result<(), VoiceDecoderError> {
        let user_id = packet.ssrc;
        let mut decoders = self.decoders.lock().await;

        // Get or create decoder for this user
        let decoder = decoders
            .entry(user_id)
            .or_insert_with(|| {
                tracing::info!("Creating decoder for user {}", user_id);
                Arc::new(VoiceDecoder::new(self.sample_rate).expect("Failed to create decoder"))
            });

        // Clone the Arc before releasing the lock
        let decoder = decoder.clone();
        drop(decoders);

        // Insert packet without holding the HashMap lock
        decoder.insert_packet(packet).await
    }

    /// Get decoder for a specific user (blocking version for sync contexts)
    pub fn get_decoder(&self, user_id: u64) -> Arc<VoiceDecoder> {
        let mut decoders = self.decoders.blocking_lock();
        
        decoders
            .entry(user_id)
            .or_insert_with(|| {
                tracing::info!("Creating decoder for user {}", user_id);
                Arc::new(VoiceDecoder::new(self.sample_rate).expect("Failed to create decoder"))
            })
            .clone()
    }

    /// Remove decoder for a user who left
    pub async fn remove_user(&self, user_id: u64) {
        let mut decoders = self.decoders.lock().await;
        if decoders.remove(&user_id).is_some() {
            tracing::info!("Removed decoder for user {}", user_id);
        }
    }

    /// Flush all decoders (e.g., when leaving voice channel)
    pub fn flush_all(&self) {
        let mut decoders = self.decoders.blocking_lock();
        for decoder in decoders.values() {
            decoder.flush();
        }
        decoders.clear();
    }
}
