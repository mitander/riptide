//! Simulated peer manager for deterministic testing and development

use std::collections::HashMap;
use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4};
use std::sync::Arc;
use std::time::Instant;

use async_trait::async_trait;
use riptide_core::torrent::{
    ConnectionStatus, InfoHash, PeerId, PeerInfo, PeerManager, PeerMessage, PeerMessageEvent,
    PieceIndex, TorrentError,
};
use tokio::sync::{Mutex, RwLock, mpsc};

/// Configuration for in-memory peer behavior
#[derive(Debug, Clone)]
pub struct InMemoryPeerConfig {
    /// Base delay for message responses in milliseconds
    pub message_delay_ms: u64,
    /// Probability of connection failure (0.0 to 1.0)
    pub connection_failure_rate: f64,
    /// Probability of message loss (0.0 to 1.0)
    pub message_loss_rate: f64,
    /// Maximum number of simultaneous connections
    pub max_connections: usize,
    /// Whether to generate automatic keep-alive messages
    pub auto_keepalive: bool,
}

impl Default for InMemoryPeerConfig {
    fn default() -> Self {
        Self {
            message_delay_ms: 50,
            connection_failure_rate: 0.0,
            message_loss_rate: 0.0,
            max_connections: 50,
            auto_keepalive: true,
        }
    }
}

/// In-memory peer for deterministic testing
#[derive(Debug, Clone)]
struct InMemoryPeer {
    address: SocketAddr,
    _info_hash: InfoHash,
    _peer_id: PeerId,
    status: ConnectionStatus,
    connected_at: Option<Instant>,
    last_activity: Instant,
    has_pieces: Vec<bool>, // Which pieces this peer has
}

impl InMemoryPeer {
    fn new(address: SocketAddr, info_hash: InfoHash, peer_id: PeerId, total_pieces: u32) -> Self {
        // Simulate peer having random pieces
        let has_pieces = (0..total_pieces)
            .map(|_| rand::random::<f64>() < 0.8) // 80% chance of having each piece
            .collect();

        Self {
            address,
            _info_hash: info_hash,
            _peer_id: peer_id,
            status: ConnectionStatus::Connecting,
            connected_at: None,
            last_activity: Instant::now(),
            has_pieces,
        }
    }

    fn to_peer_info(&self) -> PeerInfo {
        PeerInfo {
            address: self.address,
            status: self.status.clone(),
            connected_at: self.connected_at,
            last_activity: self.last_activity,
            bytes_downloaded: 0,
            bytes_uploaded: 0,
        }
    }
}

/// In-memory peer manager for deterministic testing and development.
///
/// Provides controllable peer behavior without network communication.
/// Enables deterministic testing and fuzzing of BitTorrent engine logic.
pub struct InMemoryPeerManager {
    config: InMemoryPeerConfig,
    peers: Arc<RwLock<HashMap<SocketAddr, InMemoryPeer>>>,
    message_receiver: Arc<Mutex<mpsc::Receiver<PeerMessageEvent>>>,
    message_sender: mpsc::Sender<PeerMessageEvent>,
    _next_peer_address: Arc<Mutex<u32>>,
    total_pieces: u32,
}

impl InMemoryPeerManager {
    /// Creates new in-memory peer manager with default configuration.
    pub fn new(total_pieces: u32) -> Self {
        Self::with_config(InMemoryPeerConfig::default(), total_pieces)
    }

    /// Creates in-memory peer manager with custom configuration.
    pub fn with_config(config: InMemoryPeerConfig, total_pieces: u32) -> Self {
        let (message_sender, message_receiver) = mpsc::channel(1000);

        Self {
            config,
            peers: Arc::new(RwLock::new(HashMap::new())),
            message_receiver: Arc::new(Mutex::new(message_receiver)),
            message_sender,
            _next_peer_address: Arc::new(Mutex::new(1)),
            total_pieces,
        }
    }

    /// Injects a pre-configured peer for testing specific scenarios.
    pub async fn inject_peer(
        &mut self,
        address: SocketAddr,
        info_hash: InfoHash,
        has_pieces: Vec<bool>,
    ) {
        let peer_id = PeerId::generate();
        let mut peer = InMemoryPeer::new(address, info_hash, peer_id, has_pieces.len() as u32);
        peer.has_pieces = has_pieces;
        peer.status = ConnectionStatus::Connected;
        peer.connected_at = Some(Instant::now());

        let mut peers = self.peers.write().await;
        peers.insert(address, peer);
    }

    /// Simulates sending a message from a peer to the manager.
    pub async fn simulate_peer_message(
        &self,
        peer_address: SocketAddr,
        message: PeerMessage,
    ) -> Result<(), TorrentError> {
        // Check if peer exists and is connected
        {
            let peers = self.peers.read().await;
            let peer =
                peers
                    .get(&peer_address)
                    .ok_or_else(|| TorrentError::PeerConnectionError {
                        reason: format!("Peer not connected: {peer_address}"),
                    })?;

            if peer.status != ConnectionStatus::Connected {
                return Err(TorrentError::PeerConnectionError {
                    reason: format!("Peer not in connected state: {peer_address}"),
                });
            }
        }

        // Simulate message loss
        if rand::random::<f64>() < self.config.message_loss_rate {
            return Ok(()); // Message lost
        }

        // Add simulated delay
        if self.config.message_delay_ms > 0 {
            tokio::time::sleep(tokio::time::Duration::from_millis(
                self.config.message_delay_ms,
            ))
            .await;
        }

        // Send message to manager
        let event = PeerMessageEvent {
            peer_address,
            message,
            received_at: Instant::now(),
        };

        self.message_sender
            .send(event)
            .await
            .map_err(|_| TorrentError::PeerConnectionError {
                reason: "Manager shut down".to_string(),
            })?;

        // Update peer activity
        {
            let mut peers = self.peers.write().await;
            if let Some(peer) = peers.get_mut(&peer_address) {
                peer.last_activity = Instant::now();
            }
        }

        Ok(())
    }

    /// Simulates a peer having a specific piece.
    pub async fn set_peer_has_piece(&self, peer_address: SocketAddr, piece_index: PieceIndex) {
        let mut peers = self.peers.write().await;
        if let Some(peer) = peers.get_mut(&peer_address)
            && (piece_index.as_u32() as usize) < peer.has_pieces.len()
        {
            peer.has_pieces[piece_index.as_u32() as usize] = true;
        }
    }

    /// Triggers connection failure for specific peer.
    pub async fn trigger_connection_failure(&self, peer_address: SocketAddr) {
        let mut peers = self.peers.write().await;
        if let Some(peer) = peers.get_mut(&peer_address) {
            peer.status = ConnectionStatus::Failed;
        }
    }

    /// Returns whether a peer has a specific piece.
    pub async fn peer_has_piece(&self, peer_address: SocketAddr, piece_index: PieceIndex) -> bool {
        let peers = self.peers.read().await;
        if let Some(peer) = peers.get(&peer_address)
            && let Some(&has_piece) = peer.has_pieces.get(piece_index.as_u32() as usize)
        {
            return has_piece;
        }
        false
    }

    /// Generates deterministic peer address for testing.
    async fn _generate_peer_address(&self) -> SocketAddr {
        let mut counter = self._next_peer_address.lock().await;
        let address_int = *counter;
        *counter += 1;

        // Generate address in 192.168.x.x range for simulation
        let a = 192;
        let b = 168;
        let c = ((address_int / 256) % 256) as u8;
        let d = (address_int % 256) as u8;

        let ip = Ipv4Addr::new(a, b, c, d);
        let port = 6881 + ((address_int % 1000) as u16);

        SocketAddr::V4(SocketAddrV4::new(ip, port))
    }
}

#[async_trait]
impl PeerManager for InMemoryPeerManager {
    async fn connect_peer(
        &mut self,
        address: SocketAddr,
        info_hash: InfoHash,
        peer_id: PeerId,
    ) -> Result<(), TorrentError> {
        // Check connection limit
        {
            let peers = self.peers.read().await;
            if peers.len() >= self.config.max_connections {
                return Err(TorrentError::PeerConnectionError {
                    reason: format!("Connection limit reached: {}", self.config.max_connections),
                });
            }
        }

        // Simulate connection failure
        if rand::random::<f64>() < self.config.connection_failure_rate {
            return Err(TorrentError::PeerConnectionError {
                reason: "Simulated connection failure".to_string(),
            });
        }

        // Create simulated peer
        let mut peer = InMemoryPeer::new(address, info_hash, peer_id, self.total_pieces);
        peer.status = ConnectionStatus::Connected;
        peer.connected_at = Some(Instant::now());

        // Store peer
        {
            let mut peers = self.peers.write().await;
            peers.insert(address, peer);
        }

        // Simulate initial handshake delay
        tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;

        Ok(())
    }

    async fn disconnect_peer(&mut self, address: SocketAddr) -> Result<(), TorrentError> {
        let mut peers = self.peers.write().await;
        if let Some(peer) = peers.get_mut(&address) {
            peer.status = ConnectionStatus::Disconnected;
        }
        Ok(())
    }

    async fn send_message(
        &mut self,
        peer_address: SocketAddr,
        message: PeerMessage,
    ) -> Result<(), TorrentError> {
        // Check if peer exists and is connected
        {
            let peers = self.peers.read().await;
            let peer =
                peers
                    .get(&peer_address)
                    .ok_or_else(|| TorrentError::PeerConnectionError {
                        reason: format!("Peer not connected: {peer_address}"),
                    })?;

            if peer.status != ConnectionStatus::Connected {
                return Err(TorrentError::PeerConnectionError {
                    reason: format!("Peer not in connected state: {peer_address}"),
                });
            }
        }

        // Simulate message loss
        if rand::random::<f64>() < self.config.message_loss_rate {
            return Ok(()); // Message lost, but not an error from sender perspective
        }

        // For simulated peers, we just log the outgoing message
        // In a real implementation, this would send over TCP

        // Update peer activity
        {
            let mut peers = self.peers.write().await;
            if let Some(peer) = peers.get_mut(&peer_address) {
                peer.last_activity = Instant::now();
            }
        }

        // Simulate automatic responses for certain message types
        match message {
            PeerMessage::Interested => {
                // Peer might respond with unchoke
                if rand::random::<f64>() < 0.8 {
                    self.simulate_peer_message(peer_address, PeerMessage::Unchoke)
                        .await?;
                }
            }
            PeerMessage::Request { piece_index, .. } => {
                // Check if peer has this piece and respond
                if self.peer_has_piece(peer_address, piece_index).await {
                    // Simulate piece data response
                    let piece_data = vec![0u8; 16384]; // Typical piece size
                    let response = PeerMessage::Piece {
                        piece_index,
                        offset: 0,
                        data: piece_data.into(),
                    };
                    self.simulate_peer_message(peer_address, response).await?;
                }
            }
            _ => {
                // Other messages don't typically generate automatic responses
            }
        }

        Ok(())
    }

    async fn receive_message(&mut self) -> Result<PeerMessageEvent, TorrentError> {
        let mut receiver = self.message_receiver.lock().await;
        receiver
            .recv()
            .await
            .ok_or_else(|| TorrentError::PeerConnectionError {
                reason: "All peer connections closed".to_string(),
            })
    }

    async fn connected_peers(&self) -> Vec<PeerInfo> {
        let peers = self.peers.read().await;
        peers
            .values()
            .filter(|peer| peer.status == ConnectionStatus::Connected)
            .map(|peer| peer.to_peer_info())
            .collect()
    }

    async fn connection_count(&self) -> usize {
        let peers = self.peers.read().await;
        peers
            .values()
            .filter(|peer| peer.status == ConnectionStatus::Connected)
            .count()
    }

    async fn shutdown(&mut self) -> Result<(), TorrentError> {
        let mut peers = self.peers.write().await;
        for peer in peers.values_mut() {
            peer.status = ConnectionStatus::Disconnected;
        }
        Ok(())
    }
}

#[cfg(test)]
mod simulated_peer_manager_tests {
    use std::net::{IpAddr, Ipv4Addr};

    use super::*;

    #[tokio::test]
    async fn test_simulated_peer_manager_creation() {
        let manager = InMemoryPeerManager::new(100);
        assert_eq!(manager.connection_count().await, 0);
    }

    #[tokio::test]
    async fn test_simulated_peer_connection_success() {
        let mut manager = InMemoryPeerManager::new(100);
        let address = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 6881);
        let info_hash = InfoHash::new([1u8; 20]);
        let peer_id = PeerId::generate();

        let result = manager.connect_peer(address, info_hash, peer_id).await;
        assert!(result.is_ok());
        assert_eq!(manager.connection_count().await, 1);

        let connected_peers = manager.connected_peers().await;
        assert_eq!(connected_peers.len(), 1);
        assert_eq!(connected_peers[0].address, address);
        assert_eq!(connected_peers[0].status, ConnectionStatus::Connected);
    }

    #[tokio::test]
    async fn test_simulated_peer_connection_failure() {
        let config = InMemoryPeerConfig {
            connection_failure_rate: 1.0, // Always fail
            ..Default::default()
        };
        let mut manager = InMemoryPeerManager::with_config(config, 100);
        let address = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 6881);
        let info_hash = InfoHash::new([1u8; 20]);
        let peer_id = PeerId::generate();

        let result = manager.connect_peer(address, info_hash, peer_id).await;
        assert!(result.is_err());
        assert_eq!(manager.connection_count().await, 0);
    }

    #[tokio::test]
    async fn test_peer_message_simulation() {
        let mut manager = InMemoryPeerManager::new(100);
        let address = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 6881);
        let info_hash = InfoHash::new([1u8; 20]);
        let peer_id = PeerId::generate();

        // Connect peer
        manager
            .connect_peer(address, info_hash, peer_id)
            .await
            .unwrap();

        // Simulate message from peer
        let message = PeerMessage::Choke;
        manager
            .simulate_peer_message(address, message.clone())
            .await
            .unwrap();

        // Receive message
        let received = manager.receive_message().await.unwrap();
        assert_eq!(received.peer_address, address);
        assert_eq!(received.message, message);
    }

    #[tokio::test]
    async fn test_peer_has_piece_functionality() {
        let mut manager = InMemoryPeerManager::new(100);
        let address = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 6881);
        let info_hash = InfoHash::new([1u8; 20]);

        // Inject peer with specific pieces
        let has_pieces = vec![true, false, true, false, true]; // Has pieces 0, 2, 4
        manager.inject_peer(address, info_hash, has_pieces).await;

        // Test piece availability
        assert!(manager.peer_has_piece(address, PieceIndex::new(0)).await);
        assert!(!manager.peer_has_piece(address, PieceIndex::new(1)).await);
        assert!(manager.peer_has_piece(address, PieceIndex::new(2)).await);
        assert!(!manager.peer_has_piece(address, PieceIndex::new(3)).await);
        assert!(manager.peer_has_piece(address, PieceIndex::new(4)).await);
    }

    #[tokio::test]
    async fn test_connection_limit() {
        let config = InMemoryPeerConfig {
            max_connections: 2,
            ..Default::default()
        };
        let mut manager = InMemoryPeerManager::with_config(config, 100);
        let info_hash = InfoHash::new([1u8; 20]);
        let peer_id = PeerId::generate();

        // Connect two peers successfully
        let addr1 = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 6881);
        let addr2 = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 6882);
        let addr3 = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 6883);

        assert!(
            manager
                .connect_peer(addr1, info_hash, peer_id)
                .await
                .is_ok()
        );
        assert!(
            manager
                .connect_peer(addr2, info_hash, peer_id)
                .await
                .is_ok()
        );

        // Third connection should fail due to limit
        let result = manager.connect_peer(addr3, info_hash, peer_id).await;
        assert!(result.is_err());
        assert_eq!(manager.connection_count().await, 2);
    }

    #[tokio::test]
    async fn test_automatic_piece_response() {
        let mut manager = InMemoryPeerManager::new(100);
        let address = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 6881);
        let info_hash = InfoHash::new([1u8; 20]);
        let peer_id = PeerId::generate();

        // Connect peer
        manager
            .connect_peer(address, info_hash, peer_id)
            .await
            .unwrap();

        // Set peer to have piece 0
        manager
            .set_peer_has_piece(address, PieceIndex::new(0))
            .await;

        // Send piece request
        let request = PeerMessage::Request {
            piece_index: PieceIndex::new(0),
            offset: 0,
            length: 16384,
        };
        manager.send_message(address, request).await.unwrap();

        // Should receive automatic piece response
        let response = manager.receive_message().await.unwrap();
        assert_eq!(response.peer_address, address);
        assert!(matches!(response.message, PeerMessage::Piece { .. }));
    }

    #[tokio::test]
    async fn test_disconnect_peer() {
        let mut manager = InMemoryPeerManager::new(100);
        let address = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 6881);
        let info_hash = InfoHash::new([1u8; 20]);
        let peer_id = PeerId::generate();

        // Connect then disconnect
        manager
            .connect_peer(address, info_hash, peer_id)
            .await
            .unwrap();
        assert_eq!(manager.connection_count().await, 1);

        manager.disconnect_peer(address).await.unwrap();
        assert_eq!(manager.connection_count().await, 0);
    }

    #[tokio::test]
    async fn test_shutdown() {
        let mut manager = InMemoryPeerManager::new(100);
        let result = manager.shutdown().await;
        assert!(result.is_ok());
    }
}
