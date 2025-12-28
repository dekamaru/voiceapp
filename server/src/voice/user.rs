use std::net::SocketAddr;

#[derive(Clone, Copy, Debug)]
pub struct User {
    pub token: u64,
    pub in_voice: bool,
    pub udp_address: Option<SocketAddr>,
}