use tracing::error;
use voiceapp_server::Server;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();

    let server = Server::new();
    if let Err(e) = server.run("127.0.0.1:9001").await {
        error!("Server error: {}", e);
    }
}
