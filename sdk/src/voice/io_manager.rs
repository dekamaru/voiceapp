use std::sync::Arc;
use async_channel::{unbounded, Receiver, Sender};
use dashmap::DashMap;
use tracing::{error, info};
use voiceapp_protocol::Packet;
use crate::voice::input_pipeline::{InputPipeline};
use crate::voice::decoder::VoiceData;
use crate::Decoder;

/// Manages voice input and output with dynamic sample rate configuration
pub struct InputOutputManager {
    send_tx: Sender<Vec<u8>>,
    input_pipeline: Option<InputPipeline>,
    output_decoders: Arc<DashMap<u64, (u32, Arc<Decoder>)>>,
}

impl InputOutputManager {
    pub fn new(send_tx: Sender<Vec<u8>>, receive_tx: Receiver<Packet>) -> Self {
        let output_decoders = Arc::new(DashMap::new());

        // Spawn async task to process incoming voice packets
        tokio::spawn(Self::process_incoming_packets(receive_tx, Arc::clone(&output_decoders)));

        InputOutputManager {
            send_tx,
            input_pipeline: None,
            output_decoders
        }
    }

    /// Get the voice input sender for external audio sources
    /// External sources can change, but they all write to the same stream
    pub fn get_voice_input_sender(&mut self, input_sample_rate: u32) -> Result<Sender<Vec<f32>>, String> {
        // Drop the old pipeline
        self.input_pipeline = None;

        // Create a new channel for the new pipeline
        let (new_tx, new_rx) = unbounded();

        let pipeline = InputPipeline::new(
            input_sample_rate,
            new_rx,
            self.send_tx.clone(),
        )?;

        self.input_pipeline = Some(pipeline);

        info!("Voice input pipeline initialized with sample rate {}", input_sample_rate);

        Ok(new_tx)
    }

    /// Get or create a voice output decoder for a specific user
    /// If decoder exists and sample rate matches, returns existing decoder
    /// If sample rate changed, creates new decoder with new sample rate
    pub fn get_voice_output_for(&mut self, user_id: u64, output_sample_rate: u32) -> Arc<Decoder> {
        // Check if decoder exists for this user
        if let Some(entry) = self.output_decoders.get(&user_id) {
            let (current_sample_rate, decoder) = entry.value();
            // If sample rate matches, return existing decoder
            if *current_sample_rate == output_sample_rate {
                return Arc::clone(decoder);
            }

            // Sample rate changed, will create new decoder below
            info!("Sample rate changed for user {}: {} -> {}", user_id, current_sample_rate, output_sample_rate);
        }

        // Create new decoder with the specified sample rate
        let decoder = Arc::new(
            Decoder::new(output_sample_rate)
                .expect("Failed to create voice decoder")
        );

        // Store decoder with its sample rate
        self.output_decoders.insert(user_id, (output_sample_rate, Arc::clone(&decoder)));

        info!("Created voice decoder for user {} with sample rate {}", user_id, output_sample_rate);

        decoder
    }

    pub fn remove_voice_output_for(&mut self, user_id: u64) {
        self.output_decoders.remove(&user_id);
    }

    pub fn remove_all_voice_outputs(&mut self) {
        self.output_decoders.clear();
    }

    /// Background task that processes incoming voice packets
    /// Runs until receive_tx is closed
    async fn process_incoming_packets(
        receive_rx: Receiver<Packet>,
        output_decoders: Arc<DashMap<u64, (u32, Arc<Decoder>)>>,
    ) {
        info!("Voice packet processor started");

        loop {
            match receive_rx.recv().await {
                Ok(packet) => {
                    // Only process VoiceData packets
                    if let Packet::VoiceData { user_id, sequence, timestamp, data } = packet {
                        // Create VoiceData struct for decoder
                        let voice_data = VoiceData {
                            sequence,
                            timestamp,
                            user_id,
                            opus_frame: data,
                        };

                        // Look up decoder for this user
                        if let Some(entry) = output_decoders.get(&user_id) {
                            let (_, decoder) = entry.value();
                            // Insert packet into the decoder's NetEQ buffer
                            if let Err(e) = decoder.consume_voice_data(voice_data) {
                                error!("Failed to insert packet for user {}: {}", user_id, e);
                            }
                        }
                    }
                }
                Err(_) => {
                    info!("Voice packet processor stopped (channel closed)");
                    break;
                }
            }
        }
    }
}
