//! BitTorrent wire protocol implementation

use super::TorrentError;
use std::net::SocketAddr;

/// Connection to a BitTorrent peer
pub struct PeerConnection {
    addr: SocketAddr,
}

impl PeerConnection {
    /// Establishes connection to BitTorrent peer
    ///
    /// # Errors
    /// - Currently returns Ok but will add connection errors in future implementation
    pub async fn connect(addr: SocketAddr) -> Result<Self, TorrentError> {
        // BitTorrent handshake implementation pending
        Ok(Self { addr })
    }

    pub fn peer_address(&self) -> SocketAddr {
        self.addr
    }
}
