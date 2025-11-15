use std::collections::BTreeMap;
use tracing::{debug, warn};
use voiceapp_protocol::VoiceData;

/// Jitter buffer for reordering and buffering voice packets
/// Handles out-of-order packet arrival and triggers packet loss concealment
pub struct JitterBuffer {
    buffer: BTreeMap<u32, VoiceData>,
    next_seq: u32,
    max_depth: usize,
}

impl JitterBuffer {
    /// Create a new jitter buffer with configurable maximum depth
    /// max_depth: Maximum number of packets to buffer before dropping oldest
    pub fn new(max_depth: usize) -> Self {
        JitterBuffer {
            buffer: BTreeMap::new(),
            next_seq: 0,
            max_depth,
        }
    }

    /// Insert a packet into the buffer
    /// Returns Some(packet) if this completes a sequence and packet is ready to decode
    /// Returns None if packet is buffered waiting for earlier packets
    pub fn insert(&mut self, packet: VoiceData) -> Option<VoiceData> {
        let seq = packet.sequence;

        // Initialize next_seq on first packet
        if self.buffer.is_empty() && self.next_seq == 0 {
            self.next_seq = seq;
        }

        // Check if packet is too old (already decoded or very far behind)
        // Use wrapping subtraction to handle sequence number wrap-around
        let diff = seq.wrapping_sub(self.next_seq);
        if diff > 100000 {
            // This is a very old packet (wrapping_sub resulted in large value = packet is behind)
            debug!("Dropping old packet: seq={}, next_seq={}", seq, self.next_seq);
            return None;
        }

        // Insert packet into buffer
        self.buffer.insert(seq, packet);

        // Check if we can decode the next sequence
        self.try_decode_next()
    }

    /// Try to decode the next packet in sequence
    /// Returns the first packet in sequence (caller may want to call again to get more)
    fn try_decode_next(&mut self) -> Option<VoiceData> {
        if let Some(packet) = self.buffer.remove(&self.next_seq) {
            self.next_seq = self.next_seq.wrapping_add(1);
            debug!("Decoded packet seq={}, buffer_size={}", packet.sequence, self.buffer.len());
            return Some(packet);
        }

        // Buffer is too full, trigger PLC by returning None (caller will handle PLC)
        if self.buffer.len() > self.max_depth {
            warn!(
                "Jitter buffer full (depth={}), skipping to next available packet",
                self.buffer.len()
            );
            // Find and skip to next available packet
            if let Some((&next_available, _)) = self.buffer.iter().next() {
                let gap = next_available.wrapping_sub(self.next_seq);
                if gap <= 1000 {
                    // Gap is reasonable, skip to next available
                    self.next_seq = next_available;
                    return self.try_decode_next();
                } else {
                    // Gap is too large, indicates packet loss
                    debug!(
                        "Large packet loss detected: gap={} packets, clearing buffer",
                        gap
                    );
                    self.buffer.clear();
                }
            }
        }

        None
    }

    /// Check if there are more packets available to decode
    /// Call this after insert() returns Some to get any buffered packets that are now in sequence
    pub fn next_available(&mut self) -> Option<VoiceData> {
        self.try_decode_next()
    }
}
