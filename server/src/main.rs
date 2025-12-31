mod config;
mod error;
mod event;
mod management;
mod voice;

use tracing::error;
use crate::config::{DEFAULT_MANAGEMENT_PORT, DEFAULT_VOICE_PORT};
use crate::management::server::ManagementServer;
use crate::voice::server::VoiceRelayServer;

#[tokio::main]
async fn main() {
    #[cfg(debug_assertions)]
    {
        use tracing::Level;
        tracing_subscriber::fmt()
            .with_max_level(Level::DEBUG)
            .init();
    }

    #[cfg(not(debug_assertions))]
    {
        tracing_subscriber::fmt::init();
    }

    let (management_server, events_rx) = ManagementServer::new();
    let management_thread = tokio::spawn(async move {
        if let Err(e) = management_server.run(DEFAULT_MANAGEMENT_PORT).await {
            error!("ManagementServer error: {}", e);
        }
    });

    let mut voice_relay_server = VoiceRelayServer::new(events_rx);
    let voice_relay_thread = tokio::spawn(async move {
        if let Err(e) = voice_relay_server.run(DEFAULT_VOICE_PORT).await {
            error!("VoiceRelayServer error: {}", e);
        }
    });

    // Wait for both servers
    let _ = tokio::join!(management_thread, voice_relay_thread);
}
