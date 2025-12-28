/// Events for voice relay server communication
pub enum Event {
    UserJoinedServer { id: u64, token: u64 },
    UserJoinedVoice { id: u64 },
    UserLeftVoice { id: u64 },
    UserLeftServer { id: u64 },
}