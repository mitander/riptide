//! BitTorrent wire protocol implementation

use super::TorrentError;
use std::net::SocketAddr;

/// Connection to a BitTorrent peer
pub struct PeerConnection {
    addr: SocketAddr,
}

impl PeerConnection {
    pub async fn connect(addr: SocketAddr) -> Result<Self, TorrentError> {
        // TODO: Implement actual peer connection
        Ok(Self { addr })
    }
    
    pub fn peer_addr(&self) -> SocketAddr {
        self.addr
    }
}