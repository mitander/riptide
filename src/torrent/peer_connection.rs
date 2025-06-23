//! BitTorrent wire protocol implementation

use std::net::SocketAddr;

use super::{BitTorrentPeerProtocol, InfoHash, PeerHandshake, PeerId, PeerProtocol, TorrentError};

/// Connection to a BitTorrent peer
pub struct PeerConnection {
    address: SocketAddr,
    protocol: BitTorrentPeerProtocol,
}

impl PeerConnection {
    /// Establishes connection and performs BitTorrent handshake
    ///
    /// # Errors
    /// - `TorrentError::PeerConnectionError` - TCP connection failed
    /// - `TorrentError::ProtocolError` - Handshake validation failed
    pub async fn connect(
        address: SocketAddr,
        info_hash: InfoHash,
        peer_id: PeerId,
    ) -> Result<Self, TorrentError> {
        let mut protocol = BitTorrentPeerProtocol::new();
        let handshake = PeerHandshake::new(info_hash, peer_id);

        // Perform connection and handshake
        protocol.connect(address, handshake).await?;

        Ok(Self { address, protocol })
    }

    /// Returns the socket address of the connected peer.
    pub fn peer_address(&self) -> SocketAddr {
        self.address
    }

    /// Check if peer is connected
    pub fn is_connected(&self) -> bool {
        self.protocol.peer_address().is_some()
    }

    /// Send message to peer
    ///
    /// # Errors
    /// - `TorrentError::PeerConnectionError` - Connection lost or send failed
    pub async fn send_message(&mut self, message: super::PeerMessage) -> Result<(), TorrentError> {
        self.protocol.send_message(message).await
    }

    /// Receive message from peer
    ///
    /// # Errors
    /// - `TorrentError::PeerConnectionError` - Connection lost or receive failed
    pub async fn receive_message(&mut self) -> Result<super::PeerMessage, TorrentError> {
        self.protocol.receive_message().await
    }

    /// Disconnect from peer
    ///
    /// # Errors
    /// - `TorrentError::PeerConnectionError` - Error during disconnect
    pub async fn disconnect(&mut self) -> Result<(), TorrentError> {
        self.protocol.disconnect().await
    }
}

#[cfg(test)]
mod tests {
    use std::net::{IpAddr, Ipv4Addr};

    use super::*;

    fn create_test_info_hash() -> InfoHash {
        InfoHash::new([1u8; 20])
    }

    fn create_test_peer_id() -> PeerId {
        PeerId::generate()
    }

    #[tokio::test]
    async fn test_peer_connection_interface() {
        let address = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 6881);
        let info_hash = create_test_info_hash();
        let peer_id = create_test_peer_id();

        // Connection will fail since no actual peer is running, but test the interface
        let result = PeerConnection::connect(address, info_hash, peer_id).await;

        // Expected to fail due to no actual peer, but verify the method signature works
        assert!(result.is_err());
        if let Err(e) = result {
            // Should be a connection error, not a protocol error
            assert!(
                e.to_string().contains("connection") || e.to_string().contains("Failed to connect")
            );
        }
    }

    #[tokio::test]
    async fn test_peer_connection_parameter_validation() {
        let addresses = [
            SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 6881),
            SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 8080),
            SocketAddr::new(IpAddr::V4(Ipv4Addr::new(192, 168, 1, 1)), 6881),
        ];

        let info_hash = create_test_info_hash();
        let peer_id = create_test_peer_id();

        for address in addresses {
            // All should fail to connect but with proper error handling
            let result = PeerConnection::connect(address, info_hash, peer_id).await;
            assert!(result.is_err());
        }
    }

    #[test]
    fn test_peer_id_generation() {
        let peer_id1 = create_test_peer_id();
        let peer_id2 = create_test_peer_id();

        // Should generate different IDs
        assert_ne!(peer_id1.as_bytes(), peer_id2.as_bytes());

        // Should start with Riptide prefix
        assert_eq!(&peer_id1.as_bytes()[..8], b"-RT0001-");
    }
}
