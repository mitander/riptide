//! Simulation peer manager for deterministic BitTorrent protocol testing.
//!
//! Provides deterministic, reproducible simulation with configurable network conditions.
//! Used for bug reproduction, edge case testing, and regression validation.

use std::collections::HashMap;
use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4};
use std::sync::Arc;
use std::time::Instant;

use async_trait::async_trait;
use riptide_core::torrent::{
    ConnectionStatus, InfoHash, PeerId, PeerInfo, PeerManager, PeerMessage, PeerMessageEvent,
    PieceIndex, PieceStore, TorrentError,
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
}

impl Default for InMemoryPeerConfig {
    fn default() -> Self {
        Self {
            message_delay_ms: 1, // Minimal delay for development speed
            connection_failure_rate: 0.0,
            message_loss_rate: 0.0,
            max_connections: 50,
        }
    }
}

/// Simulated peer for deterministic testing.
#[derive(Debug, Clone)]
struct SimPeer {
    address: SocketAddr,
    info_hash: InfoHash,
    #[allow(dead_code)]
    peer_id: PeerId,
    status: ConnectionStatus,
    connected_at: Option<Instant>,
    last_activity: Instant,
    available_pieces: Vec<bool>, // Which pieces this peer has available
    upload_rate_bytes_per_second: u64, // Simulated upload speed
    bytes_downloaded: u64,
    bytes_uploaded: u64,
}

impl SimPeer {
    fn new(
        address: SocketAddr,
        info_hash: InfoHash,
        peer_id: PeerId,
        piece_count: u32,
        upload_rate: u64,
    ) -> Self {
        // In development mode, peers have all pieces (they're seeders)
        let available_pieces = (0..piece_count)
            .map(|_| true) // All pieces available
            .collect();

        Self {
            address,
            info_hash,
            peer_id,
            status: ConnectionStatus::Connecting,
            connected_at: None,
            last_activity: Instant::now(),
            available_pieces,
            upload_rate_bytes_per_second: upload_rate,
            bytes_downloaded: 0,
            bytes_uploaded: 0,
        }
    }

    fn to_peer_info(&self) -> PeerInfo {
        PeerInfo {
            address: self.address,
            status: self.status.clone(),
            connected_at: self.connected_at,
            last_activity: self.last_activity,
            bytes_downloaded: self.bytes_downloaded,
            bytes_uploaded: self.bytes_uploaded,
        }
    }

    fn has_piece(&self, piece_index: PieceIndex) -> bool {
        let index = piece_index.as_u32() as usize;
        index < self.available_pieces.len() && self.available_pieces[index]
    }

    fn update_piece_availability(&mut self, piece_index: PieceIndex, has_piece: bool) {
        let index = piece_index.as_u32() as usize;
        if index < self.available_pieces.len() {
            self.available_pieces[index] = has_piece;
        }
    }
}

/// Content-aware peer manager with real piece serving
///
/// Uses a PieceStore to serve actual file data in response to piece requests.
/// Enables true end-to-end content distribution simulation where downloaded
/// pieces can be reassembled into the original media files.
pub struct SimPeerManager<P: PieceStore> {
    config: InMemoryPeerConfig,
    piece_store: Arc<P>,
    peers: Arc<RwLock<HashMap<SocketAddr, SimPeer>>>,
    message_receiver: Arc<Mutex<mpsc::Receiver<PeerMessageEvent>>>,
    message_sender: mpsc::Sender<PeerMessageEvent>,
    next_peer_address: Arc<Mutex<u32>>,
}

impl<P: PieceStore> SimPeerManager<P> {
    /// Creates content-aware peer manager with piece store
    pub fn new(config: InMemoryPeerConfig, piece_store: Arc<P>) -> Self {
        let (message_sender, message_receiver) = mpsc::channel(1000);

        Self {
            config,
            piece_store,
            peers: Arc::new(RwLock::new(HashMap::new())),
            message_receiver: Arc::new(Mutex::new(message_receiver)),
            message_sender,
            next_peer_address: Arc::new(Mutex::new(1)),
        }
    }

    /// Injects a peer with specific piece availability for testing
    pub async fn inject_peer_with_pieces(
        &mut self,
        address: SocketAddr,
        info_hash: InfoHash,
        available_pieces: Vec<bool>,
        upload_rate: u64,
    ) {
        let peer_id = PeerId::generate();
        let mut peer = SimPeer::new(
            address,
            info_hash,
            peer_id,
            available_pieces.len() as u32,
            upload_rate,
        );
        peer.available_pieces = available_pieces;
        peer.status = ConnectionStatus::Connected;
        peer.connected_at = Some(Instant::now());

        let mut peers = self.peers.write().await;
        peers.insert(address, peer);
    }

    /// Simulates peer message with actual piece data serving
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

    /// Updates whether a peer has a specific piece
    pub async fn update_peer_piece_availability(
        &self,
        peer_address: SocketAddr,
        piece_index: PieceIndex,
        has_piece: bool,
    ) {
        let mut peers = self.peers.write().await;
        if let Some(peer) = peers.get_mut(&peer_address) {
            peer.update_piece_availability(piece_index, has_piece);
        }
    }

    /// Returns whether a peer has a specific piece
    pub async fn peer_has_piece(&self, peer_address: SocketAddr, piece_index: PieceIndex) -> bool {
        let peers = self.peers.read().await;
        if let Some(peer) = peers.get(&peer_address) {
            return peer.has_piece(piece_index);
        }
        false
    }

    /// Triggers connection failure for specific peer
    pub async fn trigger_connection_failure(&self, peer_address: SocketAddr) {
        let mut peers = self.peers.write().await;
        if let Some(peer) = peers.get_mut(&peer_address) {
            peer.status = ConnectionStatus::Failed;
        }
    }

    /// Generates deterministic peer address for testing
    async fn generate_peer_address(&self) -> SocketAddr {
        let mut counter = self.next_peer_address.lock().await;
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

    /// Seeds content-aware peers for a specific torrent.
    ///
    /// This creates simulated peers that have pieces available for the given torrent.
    /// The tracker can then return these peer addresses to downloaders.
    pub async fn seed_peers_for_torrent(
        &mut self,
        info_hash: InfoHash,
        peer_count: usize,
    ) -> Vec<SocketAddr> {
        let piece_count = self.piece_store.piece_count(info_hash).unwrap_or(1000);
        let mut seeded_addresses = Vec::new();

        for _ in 0..peer_count {
            let address = self.generate_peer_address().await;
            let upload_rate = 5_000_000 + (rand::random::<u64>() % 5_000_000); // 5MB/s to 10MB/s for development

            // Create peer with all pieces available (seeder)
            let available_pieces = vec![true; piece_count as usize];
            self.inject_peer_with_pieces(address, info_hash, available_pieces, upload_rate)
                .await;

            seeded_addresses.push(address);
        }

        seeded_addresses
    }

    /// Returns all connected peer addresses for a specific torrent.
    ///
    /// Used by TrackerManager to provide realistic peer lists in announce responses.
    pub async fn peers_for_torrent(&self, info_hash: InfoHash) -> Vec<SocketAddr> {
        let peers = self.peers.read().await;
        peers
            .values()
            .filter(|peer| {
                peer.info_hash == info_hash && peer.status == ConnectionStatus::Connected
            })
            .map(|peer| peer.address)
            .collect()
    }
}

#[async_trait]
impl<P: PieceStore + 'static> PeerManager for SimPeerManager<P> {
    async fn connect_peer(
        &mut self,
        address: SocketAddr,
        info_hash: InfoHash,
        peer_id: PeerId,
    ) -> Result<(), TorrentError> {
        tracing::debug!(
            "connect_peer called for {} with info_hash {}",
            address,
            info_hash
        );
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

        // Get piece count from store
        let piece_count = self.piece_store.piece_count(info_hash).unwrap_or(1000);
        tracing::debug!("piece_count for {} is {}", info_hash, piece_count);

        // Create simulated peer with fast upload rate for development
        let upload_rate = 5_000_000 + (rand::random::<u64>() % 5_000_000); // 5MB/s to 10MB/s for development
        let mut peer = SimPeer::new(address, info_hash, peer_id, piece_count, upload_rate);
        peer.status = ConnectionStatus::Connected;
        peer.connected_at = Some(Instant::now());

        // Store peer and get available pieces for bitfield
        let available_pieces = {
            let mut peers = self.peers.write().await;
            let peer_clone = peer.clone();
            peers.insert(address, peer);
            peer_clone.available_pieces
        };

        // Simulate initial handshake delay
        tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
        tracing::debug!("Sending bitfield for peer {}", address);

        // Send bitfield message to inform engine about available pieces
        // Convert bool vector to bytes for bitfield
        let bitfield_bytes = {
            let mut bytes = Vec::new();
            for chunk in available_pieces.chunks(8) {
                let mut byte = 0u8;
                for (i, &has_piece) in chunk.iter().enumerate() {
                    if has_piece {
                        byte |= 1 << (7 - i);
                    }
                }
                bytes.push(byte);
            }
            bytes.into()
        };

        self.simulate_peer_message(
            address,
            PeerMessage::Bitfield {
                bitfield: bitfield_bytes,
            },
        )
        .await?;
        tracing::debug!("Successfully connected peer {} and sent bitfield", address);

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
        let (peer_info_hash, has_requested_piece) = {
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

            let has_piece = match &message {
                PeerMessage::Request { piece_index, .. } => peer.has_piece(*piece_index),
                _ => false,
            };

            (peer.info_hash, has_piece)
        };

        // Simulate message loss
        if rand::random::<f64>() < self.config.message_loss_rate {
            return Ok(()); // Message lost, but not an error from sender perspective
        }

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
            PeerMessage::Request {
                piece_index,
                offset,
                length,
            } => {
                // Check if peer has this piece and serve actual data
                if has_requested_piece {
                    // Simulate realistic upload delay based on piece size and upload rate
                    let peers = self.peers.read().await;
                    if let Some(peer) = peers.get(&peer_address) {
                        let transfer_time_ms =
                            (length as u64 * 1000) / peer.upload_rate_bytes_per_second;
                        drop(peers); // Release lock before sleeping

                        if transfer_time_ms > 0 {
                            tokio::time::sleep(tokio::time::Duration::from_millis(
                                transfer_time_ms,
                            ))
                            .await;
                        }
                    }

                    // Get actual piece data from store
                    match self
                        .piece_store
                        .piece_data(peer_info_hash, piece_index)
                        .await
                    {
                        Ok(piece_data) => {
                            // Extract requested block from piece
                            let start = offset as usize;
                            let end = (start + length as usize).min(piece_data.len());

                            if start < piece_data.len() {
                                let block_data = piece_data[start..end].to_vec();
                                let response = PeerMessage::Piece {
                                    piece_index,
                                    offset,
                                    data: block_data.into(),
                                };
                                self.simulate_peer_message(peer_address, response).await?;
                            }
                        }
                        Err(_) => {
                            // Piece not available in store - peer doesn't actually have it
                            self.update_peer_piece_availability(peer_address, piece_index, false)
                                .await;
                        }
                    }
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

    async fn upload_stats(&self) -> (u64, u64) {
        let peers = self.peers.read().await;
        let total_uploaded: u64 = peers.values().map(|peer| peer.bytes_uploaded).sum();
        // For simulation, upload speed is not tracked separately
        (total_uploaded, 0)
    }

    async fn shutdown(&mut self) -> Result<(), TorrentError> {
        let mut peers = self.peers.write().await;
        for peer in peers.values_mut() {
            peer.status = ConnectionStatus::Disconnected;
        }
        Ok(())
    }

    async fn configure_upload_manager(
        &mut self,
        info_hash: InfoHash,
        _piece_size: u64,
        _total_bandwidth: u64,
    ) -> Result<(), TorrentError> {
        tracing::debug!(
            "SimPeerManager: configure_upload_manager called for {}",
            info_hash
        );
        // No-op for simulation - no real bandwidth management needed
        Ok(())
    }

    async fn update_streaming_position(
        &mut self,
        info_hash: InfoHash,
        _byte_position: u64,
    ) -> Result<(), TorrentError> {
        tracing::debug!(
            "SimPeerManager: update_streaming_position called for {}",
            info_hash
        );
        // No-op for simulation - no real bandwidth management needed
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use riptide_core::torrent::TorrentPiece;

    use super::*;
    use crate::piece_store::InMemoryPieceStore;

    #[tokio::test]
    async fn test_sim_peer_manager_creation() {
        let piece_store = Arc::new(InMemoryPieceStore::new());
        let config = InMemoryPeerConfig::default();
        let manager = SimPeerManager::new(config, piece_store);
        assert_eq!(manager.connection_count().await, 0);
    }

    #[tokio::test]
    async fn test_sim_peer_serves_real_data() {
        let piece_store = Arc::new(InMemoryPieceStore::new());
        let info_hash = InfoHash::new([1u8; 20]);

        // Add test piece to store
        let test_piece = TorrentPiece {
            index: 0,
            hash: [0u8; 20],
            data: b"Hello, BitTorrent world! This is real piece data.".to_vec(),
        };
        piece_store
            .add_torrent_pieces(info_hash, vec![test_piece])
            .await
            .unwrap();

        let config = InMemoryPeerConfig::default();
        let mut manager = SimPeerManager::new(config, piece_store);

        let peer_address = SocketAddr::from((Ipv4Addr::LOCALHOST, 6881));
        let peer_id = PeerId::generate();

        // Connect peer
        manager
            .connect_peer(peer_address, info_hash, peer_id)
            .await
            .unwrap();

        // First, receive and ignore the bitfield message sent during connection
        let bitfield_response = tokio::time::timeout(
            tokio::time::Duration::from_millis(100),
            manager.receive_message(),
        )
        .await
        .unwrap()
        .unwrap();

        // Verify it's a bitfield message
        assert!(matches!(
            bitfield_response.message,
            PeerMessage::Bitfield { .. }
        ));

        // Set peer to have the piece
        manager
            .update_peer_piece_availability(peer_address, PieceIndex::new(0), true)
            .await;

        // Request piece block
        let request = PeerMessage::Request {
            piece_index: PieceIndex::new(0),
            offset: 0,
            length: 50,
        };
        manager.send_message(peer_address, request).await.unwrap();

        // Should receive piece data response
        let response = manager.receive_message().await.unwrap();
        assert_eq!(response.peer_address, peer_address);

        if let PeerMessage::Piece {
            piece_index,
            offset,
            data,
        } = response.message
        {
            assert_eq!(piece_index, PieceIndex::new(0));
            assert_eq!(offset, 0);
            assert_eq!(
                data.as_ref(),
                b"Hello, BitTorrent world! This is real piece data."
            );
        } else {
            panic!("Expected Piece message, got {:?}", response.message);
        }
    }

    #[tokio::test]
    async fn test_sim_peer_without_piece() {
        let piece_store = Arc::new(InMemoryPieceStore::new());
        let config = InMemoryPeerConfig::default();
        let mut manager = SimPeerManager::new(config, piece_store);

        let peer_address = SocketAddr::from((Ipv4Addr::LOCALHOST, 6881));
        let info_hash = InfoHash::new([2u8; 20]);
        let peer_id = PeerId::generate();

        // Connect peer
        manager
            .connect_peer(peer_address, info_hash, peer_id)
            .await
            .unwrap();

        // First, receive and ignore the bitfield message sent during connection
        let bitfield_response = tokio::time::timeout(
            tokio::time::Duration::from_millis(100),
            manager.receive_message(),
        )
        .await
        .unwrap()
        .unwrap();

        // Verify it's a bitfield message
        assert!(matches!(
            bitfield_response.message,
            PeerMessage::Bitfield { .. }
        ));

        // Clear all pieces from peer (simulate peer that doesn't have any pieces)
        for i in 0..1000 {
            manager
                .update_peer_piece_availability(peer_address, PieceIndex::new(i), false)
                .await;
        }

        // Request piece that peer doesn't have
        let request = PeerMessage::Request {
            piece_index: PieceIndex::new(0),
            offset: 0,
            length: 1024,
        };
        manager.send_message(peer_address, request).await.unwrap();

        // Should not receive any response (no piece data available)
        let receive_result = tokio::time::timeout(
            tokio::time::Duration::from_millis(100),
            manager.receive_message(),
        )
        .await;

        assert!(receive_result.is_err()); // Timeout - no response
    }

    #[tokio::test]
    async fn test_peer_seeding_and_coordination() {
        let piece_store = Arc::new(InMemoryPieceStore::new());
        let info_hash = InfoHash::new([3u8; 20]);

        // Add test piece to store
        let test_piece = TorrentPiece {
            index: 0,
            hash: [0u8; 20],
            data: b"Test content for peer coordination".to_vec(),
        };
        piece_store
            .add_torrent_pieces(info_hash, vec![test_piece])
            .await
            .unwrap();

        let config = InMemoryPeerConfig::default();
        let mut manager = SimPeerManager::new(config, piece_store);

        // Seed peers for the torrent
        let seeded_addresses = manager.seed_peers_for_torrent(info_hash, 3).await;
        assert_eq!(seeded_addresses.len(), 3);

        // Verify we can get peers for the torrent
        let peers_for_torrent = manager.peers_for_torrent(info_hash).await;
        assert_eq!(peers_for_torrent.len(), 3);

        // Verify addresses match
        for address in &seeded_addresses {
            assert!(peers_for_torrent.contains(address));
        }

        // Test that each peer can serve the piece
        for &peer_address in &seeded_addresses {
            let request = PeerMessage::Request {
                piece_index: PieceIndex::new(0),
                offset: 0,
                length: 34, // Length of test content
            };
            manager.send_message(peer_address, request).await.unwrap();

            let response = manager.receive_message().await.unwrap();
            assert_eq!(response.peer_address, peer_address);

            if let PeerMessage::Piece { data, .. } = response.message {
                assert_eq!(data.as_ref(), b"Test content for peer coordination");
            } else {
                panic!("Expected Piece message");
            }
        }
    }
}
