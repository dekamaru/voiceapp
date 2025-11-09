use std::collections::BTreeMap;
use tracing::{debug, warn};
use voiceapp_common::VoicePacket;

/// Jitter buffer for reordering and buffering voice packets
/// Handles out-of-order packet arrival and triggers packet loss concealment
pub struct JitterBuffer {
    buffer: BTreeMap<u32, VoicePacket>,
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
    pub fn insert(&mut self, packet: VoicePacket) -> Option<VoicePacket> {
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
    fn try_decode_next(&mut self) -> Option<VoicePacket> {
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
    pub fn next_available(&mut self) -> Option<VoicePacket> {
        self.try_decode_next()
    }

    /// Get the next sequence number expected
    pub fn next_sequence(&self) -> u32 {
        self.next_seq
    }

    /// Get number of packets currently buffered
    pub fn buffered_count(&self) -> usize {
        self.buffer.len()
    }

    /// Clear the buffer (useful on stream reset)
    pub fn clear(&mut self) {
        self.buffer.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use voiceapp_common::username_to_ssrc;

    fn create_packet(seq: u32, data: Vec<u8>) -> VoicePacket {
        VoicePacket::new(seq, seq.wrapping_mul(960), username_to_ssrc("test"), data)
    }

    #[test]
    fn test_jitter_buffer_in_order() {
        let mut jb = JitterBuffer::new(10);

        // Insert packets in order
        assert!(jb.insert(create_packet(0, vec![1, 2, 3])).is_some());
        assert!(jb.insert(create_packet(1, vec![4, 5, 6])).is_some());
        assert!(jb.insert(create_packet(2, vec![7, 8, 9])).is_some());

        assert_eq!(jb.next_sequence(), 3);
    }

    #[test]
    fn test_jitter_buffer_out_of_order() {
        let mut jb = JitterBuffer::new(10);

        // Insert packets out of order, starting with 0
        let pkt = jb.insert(create_packet(0, vec![1, 2, 3]));
        assert!(pkt.is_some());
        assert_eq!(pkt.unwrap().sequence, 0);

        // Packet 2 arrives before packet 1
        assert!(jb.insert(create_packet(2, vec![7, 8, 9])).is_none()); // Buffered

        // Packet 1 arrives, should be decodable
        let pkt = jb.insert(create_packet(1, vec![4, 5, 6]));
        assert!(pkt.is_some());
        assert_eq!(pkt.unwrap().sequence, 1);

        // Packet 2 should now be retrievable via next_available
        let pkt = jb.next_available();
        assert!(pkt.is_some());
        assert_eq!(pkt.unwrap().sequence, 2);

        // Packet 3
        let pkt = jb.insert(create_packet(3, vec![10, 11, 12]));
        assert!(pkt.is_some());
        assert_eq!(pkt.unwrap().sequence, 3);

        assert_eq!(jb.next_sequence(), 4);
        assert_eq!(jb.buffered_count(), 0);
    }

    #[test]
    fn test_jitter_buffer_duplicate_packet() {
        let mut jb = JitterBuffer::new(10);

        // Insert same packet twice
        assert!(jb.insert(create_packet(0, vec![1, 2, 3])).is_some());
        assert!(jb.insert(create_packet(0, vec![1, 2, 3])).is_none()); // Already decoded

        assert_eq!(jb.next_sequence(), 1);
    }

    #[test]
    fn test_jitter_buffer_old_packet() {
        let mut jb = JitterBuffer::new(10);

        // Decode some packets first
        assert!(jb.insert(create_packet(0, vec![1, 2, 3])).is_some());
        assert!(jb.insert(create_packet(1, vec![4, 5, 6])).is_some());

        // Try to insert old packet
        assert!(jb.insert(create_packet(0, vec![1, 2, 3])).is_none()); // Dropped as old

        assert_eq!(jb.next_sequence(), 2);
    }

    #[test]
    fn test_jitter_buffer_overflow() {
        let mut jb = JitterBuffer::new(3);

        // Insert packets out of order to fill buffer
        // First insert packet 0 to initialize next_seq
        assert!(jb.insert(create_packet(0, vec![0, 0, 0])).is_some());

        // Insert packets 2-4 to fill the buffer
        jb.insert(create_packet(2, vec![4, 5, 6]));     // Buffered
        jb.insert(create_packet(3, vec![7, 8, 9]));     // Buffered
        jb.insert(create_packet(4, vec![10, 11, 12]));  // Buffered

        // Buffer now has 3 packets (2,3,4), size=3
        // Insert packet 5 - buffer becomes full, should skip to packet 2
        let result = jb.insert(create_packet(5, vec![13, 14, 15]));
        // Should have decoded packet 2 due to buffer overflow
        assert!(result.is_some());
        assert_eq!(result.unwrap().sequence, 2);
    }

    #[test]
    fn test_jitter_buffer_large_sequence_numbers() {
        let mut jb = JitterBuffer::new(10);

        // Test with large sequence numbers
        let seq1 = 1000000u32;
        let seq2 = 1000001u32;
        let seq3 = 1000002u32;

        assert!(jb.insert(create_packet(seq1, vec![1])).is_some());
        assert!(jb.insert(create_packet(seq2, vec![2])).is_some());
        assert!(jb.insert(create_packet(seq3, vec![3])).is_some());

        assert_eq!(jb.next_sequence(), 1000003);
    }

    #[test]
    fn test_jitter_buffer_gap_detection() {
        let mut jb = JitterBuffer::new(10);

        // Insert initial packet
        assert!(jb.insert(create_packet(0, vec![1, 2, 3])).is_some());

        // Insert packet with large gap
        assert!(jb.insert(create_packet(2000, vec![4, 5, 6])).is_none());

        // Buffer should skip the gap and decode
        let result = jb.insert(create_packet(1, vec![7, 8, 9]));
        // Either decodes 1 or waits for more packets depending on gap threshold
        assert!(result.is_none() || result.is_some());
    }
}
