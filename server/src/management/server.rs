use dashmap::DashMap;
use rand::random;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::sync::mpsc::{Sender};
use tokio::net::TcpListener;
use tokio::sync::{broadcast, mpsc};
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};
use tracing::{error, info};
use crate::management::event::Event;
use crate::management::user::User;
use crate::management::user_handler::UserHandler;

/// ManagementServer handles TCP connections, user login, presence management,
/// and broadcasts events to all connected clients
pub struct ManagementServer {
    users: Arc<DashMap<SocketAddr, User>>,
    next_user_id: Arc<AtomicU64>,
    events_tx: UnboundedSender<Event>,
}

impl ManagementServer {
    pub fn new() -> (Self, UnboundedReceiver<Event>) {
        let (events_tx, events_rx) = mpsc::unbounded_channel();

        let server = ManagementServer {
            users: Arc::new(DashMap::new()),
            next_user_id: Arc::new(AtomicU64::new(1)),
            events_tx,
        };

        (server, events_rx)
    }

    /// Start the TCP listener and accept client connections
    pub async fn run(&self, addr: &str) -> Result<(), Box<dyn std::error::Error>> {
        let listener = TcpListener::bind(addr).await?;
        let local_addr = listener.local_addr()?;
        let (broadcast_tx, _) = broadcast::channel(1000);
        info!("ManagementServer listening on {}", local_addr);

        loop {
            let (socket, peer_addr) = listener.accept().await?;
            let user = self.register_new_user(peer_addr);
            let users = self.users.clone();
            let broadcast_tx = broadcast_tx.clone();
            let events_tx = self.events_tx.clone();

            tokio::spawn(async move {
                let _ = events_tx.send(Event::UserJoinedServer { id: user.id, token: user.token });

                let mut user_handler = UserHandler::new(
                    users,
                    socket,
                    peer_addr.clone(),
                    broadcast_tx,
                    events_tx.clone(),
                );

                if let Err(e) = user_handler.handle().await {
                    error!("[{}] Error: {}", peer_addr, e);
                }

                let _ = events_tx.send(Event::UserLeftServer { id: user.id });
            });
        }
    }

    fn register_new_user(&self, address: SocketAddr) -> User {
        let user_id = self.next_user_id.fetch_add(1, Ordering::Relaxed);

        let user = User {
            id: user_id,
            username: None,
            in_voice: false,
            is_muted: false,
            token: random::<u64>()
        };

        self.users.insert(address, user.clone());

        user
    }
}
