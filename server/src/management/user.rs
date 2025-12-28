/// Represents a connected user with their voice channel status and authentication token
#[derive(Clone, Debug)]
pub struct User {
    pub id: u64,
    pub username: Option<String>,
    pub in_voice: bool,
    pub is_muted: bool,
    pub token: u64, // Authentication token for UDP connections
}