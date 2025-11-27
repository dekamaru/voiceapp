mod management_server;
mod voice_relay_server;

use tracing::error;

use crate::management_server::ManagementServer;
use crate::voice_relay_server::VoiceRelayServer;

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

    let management_server = ManagementServer::new();
    let management_server_for_relay = management_server.clone();
    let management_thread = tokio::spawn(async move {
        if let Err(e) = management_server.run("0.0.0.0:9001").await {
            error!("ManagementServer error: {}", e);
        }
    });

    let voice_relay_server = VoiceRelayServer::new(management_server_for_relay);
    let voice_relay_thread = tokio::spawn(async move {
        if let Err(e) = voice_relay_server.run("0.0.0.0:9002").await {
            error!("VoiceRelayServer error: {}", e);
        }
    });

    // Wait for both servers
    let _ = tokio::join!(management_thread, voice_relay_thread);
}
