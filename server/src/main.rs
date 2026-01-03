mod config;
mod error;
mod event;
mod management;
mod voice;

use tracing::error;
use crate::config::{management_port, voice_port};
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

    let mgmt_port = management_port();
    let vc_port = voice_port();

    let (management_server, events_rx) = ManagementServer::new();
    let management_thread = tokio::spawn(async move {
        if let Err(e) = management_server.run(mgmt_port).await {
            error!("ManagementServer error: {}", e);
        }
    });

    let mut voice_relay_server = VoiceRelayServer::new(events_rx);
    let voice_relay_thread = tokio::spawn(async move {
        if let Err(e) = voice_relay_server.run(vc_port).await {
            error!("VoiceRelayServer error: {}", e);
        }
    });

    // Wait for both servers
    let _ = tokio::join!(management_thread, voice_relay_thread);
}
