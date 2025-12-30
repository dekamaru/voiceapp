pub(crate) struct VoiceData {
    pub sequence: u32,
    pub timestamp: u32,
    pub user_id: u64,
    pub opus_frame: Vec<u8>,
}