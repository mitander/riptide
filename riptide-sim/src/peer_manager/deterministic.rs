//! Deterministic peer implementation for simulation testing and bug reproduction.
//!
//! Provides fully deterministic, reproducible peer behavior with configurable
//! network conditions, failure injection, and precise timing control.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use riptide_core::torrent::{
    ConnectionStatus, InfoHash, PeerId, PeerInfo, PeerManager, PeerMessage, PeerMessageEvent,
    PieceIndex, PieceStore, TorrentError,
};
use tokio::sync::{Mutex, RwLock, mpsc};
use tokio::time::sleep;

/// Configuration for deterministic peer simulation.
#[derive(Debug, Clone)]
pub struct DeterministicConfig {
    /// Base delay for message responses in milliseconds.
    pub message_delay_ms: u64,
    /// Probability of connection failure (0.0 to 1.0).
    pub connection_failure_rate: f64,
    /// Probability of message loss (0.0 to 1.0).
    pub message_loss_rate: f64,
    /// Maximum number of simultaneous connections.
    pub max_connections: usize,
    /// Upload rate per peer in bytes per second.
    pub upload_rate_bps: u64,
    /// Random seed for deterministic behavior.
    pub seed: u64,
}

impl Default for DeterministicConfig {
    fn default() -> Self {
        Self {
            message_delay_ms: 50,          // Realistic internet delay
            connection_failure_rate: 0.05, // 5% connection failures
            message_loss_rate: 0.01,       // 1% message loss
            max_connections: 50,
            upload_rate_bps: 1024 * 1024, // 1 MB/s
            seed: 12345,                  // Fixed seed for reproducibility
        }
    }
}

impl DeterministicConfig {
    /// Configuration for ideal network conditions (no failures, minimal delay).
    pub fn ideal() -> Self {
        Self {
            message_delay_ms: 1,
            connection_failure_rate: 0.0,
            message_loss_rate: 0.0,
            max_connections: 100,
            upload_rate_bps: 10 * 1024 * 1024, // 10 MB/s
            seed: 12345,
        }
    }

    /// Configuration for poor network conditions (high latency, failures).
    pub fn poor() -> Self {
        Self {
            message_delay_ms: 200,
            connection_failure_rate: 0.15, // 15% connection failures
            message_loss_rate: 0.05,       // 5% message loss
            max_connections: 20,
            upload_rate_bps: 256 * 1024, // 256 KB/s
            seed: 12345,
        }
    }

    /// Configuration with specified seed for reproducible testing.
    pub fn with_seed(seed: u64) -> Self {
        Self {
            seed,
            ..Self::default()
        }
    }
}

/// Deterministic peer implementation for simulation testing.
///
/// Provides fully reproducible peer behavior for:
/// - Bug reproduction with identical conditions
/// - Regression testing with deterministic scenarios
/// - Performance testing with controlled network conditions
/// - Edge case testing with failure injection
pub struct DeterministicPeers<P: PieceStore> {
    config: DeterministicConfig,
    piece_store: Arc<P>,
    peers: Arc<RwLock<HashMap<SocketAddr, SimulatedPeer>>>,
    message_sender: mpsc::UnboundedSender<PeerMessageEvent>,
    message_receiver: Arc<Mutex<mpsc::UnboundedReceiver<PeerMessageEvent>>>,
    stats: Arc<RwLock<SimulationStats>>,
    rng_state: Arc<Mutex<u64>>, // Simple LCG for deterministic randomness
    next_peer_id: Arc<Mutex<u32>>,
}

/// Simulated peer for deterministic testing.
#[derive(Debug, Clone)]
struct SimulatedPeer {
    address: SocketAddr,
    info_hash: InfoHash,
    peer_id: PeerId,
    status: ConnectionStatus,
    connected_at: Option<Instant>,
    last_activity: Instant,
    available_pieces: Vec<bool>,
    upload_rate_bps: u64,
    bytes_downloaded: u64,
    bytes_uploaded: u64,
}

/// Statistics for deterministic simulation tracking.
#[derive(Debug, Default, Clone)]
pub struct SimulationStats {
    pub connections_attempted: u64,
    pub connections_successful: u64,
    pub connections_failed: u64,
    pub messages_sent: u64,
    pub messages_lost: u64,
    pub pieces_served: u64,
    pub bytes_served: u64,
    pub simulation_start: Option<Instant>,
}

impl SimulatedPeer {
    /// Creates a new simulated peer with specified configuration.
    fn new(
        address: SocketAddr,
        info_hash: InfoHash,
        peer_id: PeerId,
        piece_count: u32,
        upload_rate_bps: u64,
    ) -> Self {
        // In simulation, peers have all pieces available (seeders)
        let available_pieces = (0..piece_count).map(|_| true).collect();

        Self {
            address,
            info_hash,
            peer_id,
            status: ConnectionStatus::Connecting,
            connected_at: None,
            last_activity: Instant::now(),
            available_pieces,
            upload_rate_bps,
            bytes_downloaded: 0,
            bytes_uploaded: 0,
        }
    }

    /// Converts to PeerInfo for external interface.
    fn as_peer_info(&self) -> PeerInfo {
        PeerInfo {
            address: self.address,
            status: self.status.clone(),
            connected_at: self.connected_at,
            last_activity: self.last_activity,
            bytes_downloaded: self.bytes_downloaded,
            bytes_uploaded: self.bytes_uploaded,
        }
    }

    /// Checks if peer has a specific piece.
    fn has_piece(&self, piece_index: PieceIndex) -> bool {
        let index = piece_index.as_u32() as usize;
        index < self.available_pieces.len() && self.available_pieces[index]
    }

    /// Marks peer as connected.
    fn connect(&mut self) {
        self.status = ConnectionStatus::Connected;
        self.connected_at = Some(Instant::now());
        self.last_activity = Instant::now();
    }

    /// Updates activity timestamp.
    fn update_activity(&mut self) {
        self.last_activity = Instant::now();
    }
}

impl<P: PieceStore> DeterministicPeers<P> {
    /// Creates a new deterministic peer manager with specified configuration.
    pub fn new(config: DeterministicConfig, piece_store: Arc<P>) -> Self {
        let (message_sender, message_receiver) = mpsc::unbounded_channel();

        Self {
            config: config.clone(),
            piece_store,
            peers: Arc::new(RwLock::new(HashMap::new())),
            message_sender,
            message_receiver: Arc::new(Mutex::new(message_receiver)),
            stats: Arc::new(RwLock::new(SimulationStats {
                simulation_start: Some(Instant::now()),
                ..Default::default()
            })),
            rng_state: Arc::new(Mutex::new(config.seed)),
            next_peer_id: Arc::new(Mutex::new(1)),
        }
    }

    /// Creates deterministic peers with ideal network conditions.
    pub fn new_ideal(piece_store: Arc<P>) -> Self {
        Self::new(DeterministicConfig::ideal(), piece_store)
    }

    /// Creates deterministic peers with poor network conditions.
    pub fn new_poor(piece_store: Arc<P>) -> Self {
        Self::new(DeterministicConfig::poor(), piece_store)
    }

    /// Returns current simulation statistics.
    pub async fn simulation_stats(&self) -> SimulationStats {
        self.stats.read().await.clone()
    }

    /// Returns configuration used for this simulation.
    pub fn config(&self) -> &DeterministicConfig {
        &self.config
    }

    /// Generates deterministic random number using Linear Congruential Generator.
    async fn next_random(&self) -> f64 {
        let mut rng = self.rng_state.lock().await;
        *rng = rng.wrapping_mul(1103515245).wrapping_add(12345);
        (*rng as f64) / (u64::MAX as f64)
    }

    /// Generates a unique peer ID deterministically.
    async fn generate_peer_id(&self) -> PeerId {
        let mut counter = self.next_peer_id.lock().await;
        let id = *counter;
        *counter += 1;
        PeerId::new([
            b'R',
            b'T', // Riptide
            (id >> 24) as u8,
            (id >> 16) as u8,
            (id >> 8) as u8,
            id as u8,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
        ])
    }

    /// Checks if connection should fail based on configuration.
    async fn should_connection_fail(&self) -> bool {
        let random = self.next_random().await;
        random < self.config.connection_failure_rate
    }

    /// Checks if message should be lost based on configuration.
    async fn should_message_be_lost(&self) -> bool {
        let random = self.next_random().await;
        random < self.config.message_loss_rate
    }

    /// Gets deterministic message delay.
    fn message_delay(&self) -> Duration {
        Duration::from_millis(self.config.message_delay_ms)
    }
}

#[async_trait]
impl<P: PieceStore + Send + Sync + 'static> PeerManager for DeterministicPeers<P> {
    /// Connects to a peer with deterministic behavior and failure simulation.
    async fn connect_peer(
        &mut self,
        peer_address: SocketAddr,
        info_hash: InfoHash,
        _peer_id: PeerId,
    ) -> Result<(), TorrentError> {
        // Update statistics
        {
            let mut stats = self.stats.write().await;
            stats.connections_attempted += 1;
        }

        // Check for deterministic connection failure
        if self.should_connection_fail().await {
            let mut stats = self.stats.write().await;
            stats.connections_failed += 1;
            return Err(TorrentError::PeerConnectionError {
                reason: format!("Simulated connection failure for {}", peer_address),
            });
        }

        // Simulate connection delay
        sleep(self.message_delay()).await;

        let peer_id = self.generate_peer_id().await;
        let piece_count = 100; // Default piece count for simulation
        let upload_rate = self.config.upload_rate_bps;

        let mut peer =
            SimulatedPeer::new(peer_address, info_hash, peer_id, piece_count, upload_rate);
        peer.connect();

        {
            let mut peers = self.peers.write().await;
            peers.insert(peer_address, peer);
        }

        {
            let mut stats = self.stats.write().await;
            stats.connections_successful += 1;
        }

        Ok(())
    }

    /// Sends a message to a peer with deterministic delays and loss simulation.
    async fn send_message(
        &mut self,
        peer_address: SocketAddr,
        message: PeerMessage,
    ) -> Result<(), TorrentError> {
        // Update statistics
        {
            let mut stats = self.stats.write().await;
            stats.messages_sent += 1;
        }

        // Check for deterministic message loss
        if self.should_message_be_lost().await {
            let mut stats = self.stats.write().await;
            stats.messages_lost += 1;
            return Ok(()); // Message lost, but not an error
        }

        // Simulate message delay
        sleep(self.message_delay()).await;

        // Update peer activity
        {
            let mut peers = self.peers.write().await;
            if let Some(peer) = peers.get_mut(&peer_address) {
                peer.update_activity();
            }
        }

        // Process specific message types
        match message {
            PeerMessage::Piece {
                piece_index,
                offset,
                data,
            } => {
                let bytes_transferred = data.len() as u64;

                // Update statistics
                {
                    let mut stats = self.stats.write().await;
                    stats.bytes_served += bytes_transferred;
                    stats.pieces_served += 1;
                }

                // Send piece data through message channel
                let event = PeerMessageEvent {
                    peer_address,
                    message: PeerMessage::Piece {
                        piece_index,
                        offset,
                        data,
                    },
                    received_at: Instant::now(),
                };

                self.message_sender
                    .send(event)
                    .map_err(|_| TorrentError::ProtocolError {
                        message: "Failed to send piece data".to_string(),
                    })?;
            }
            _ => {
                // Handle other message types as needed
            }
        }

        Ok(())
    }

    /// Disconnects from a peer.
    async fn disconnect_peer(&mut self, peer_address: SocketAddr) -> Result<(), TorrentError> {
        let mut peers = self.peers.write().await;
        if let Some(mut peer) = peers.remove(&peer_address) {
            peer.status = ConnectionStatus::Disconnected;
        }
        Ok(())
    }

    /// Returns information about connected peers.
    async fn connected_peers(&self) -> Vec<PeerInfo> {
        let peers = self.peers.read().await;
        peers.values().map(|peer| peer.as_peer_info()).collect()
    }

    /// Receives the next peer message event.
    async fn receive_message(&mut self) -> Result<PeerMessageEvent, TorrentError> {
        let mut receiver = self.message_receiver.lock().await;
        receiver
            .recv()
            .await
            .ok_or_else(|| TorrentError::PeerConnectionError {
                reason: "Message channel closed".to_string(),
            })
    }

    /// Returns count of active peer connections.
    async fn connection_count(&self) -> usize {
        self.peers.read().await.len()
    }

    /// Returns upload statistics for all connected peers.
    async fn upload_stats(&self) -> (u64, u64) {
        let stats = self.stats.read().await;
        let upload_rate = if let Some(start_time) = stats.simulation_start {
            let elapsed = start_time.elapsed().as_secs();
            if elapsed > 0 {
                stats.bytes_served / elapsed
            } else {
                0
            }
        } else {
            0
        };
        (stats.bytes_served, upload_rate)
    }

    /// Disconnects all peers and shuts down simulation.
    async fn shutdown(&mut self) -> Result<(), TorrentError> {
        let mut peers = self.peers.write().await;
        for peer in peers.values_mut() {
            peer.status = ConnectionStatus::Disconnected;
        }
        peers.clear();

        // Log final statistics
        let stats = self.stats.read().await;
        tracing::info!(
            "DeterministicPeers: Simulation complete - {} connections, {} messages, {} pieces served",
            stats.connections_successful,
            stats.messages_sent,
            stats.pieces_served
        );

        Ok(())
    }

    /// Configure upload manager (no-op for simulation).
    async fn configure_upload_manager(
        &mut self,
        _info_hash: InfoHash,
        _piece_size: u64,
        _total_bandwidth: u64,
    ) -> Result<(), TorrentError> {
        // No-op for deterministic simulation
        Ok(())
    }

    /// Update streaming position (no-op for simulation).
    async fn update_streaming_position(
        &mut self,
        _info_hash: InfoHash,
        _byte_position: u64,
    ) -> Result<(), TorrentError> {
        // No-op for deterministic simulation
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use riptide_core::torrent::test_data::create_test_piece_store;

    use super::*;

    #[tokio::test]
    async fn test_deterministic_peer_creation() {
        let piece_store = create_test_piece_store();
        let config = DeterministicConfig::default();
        let peers = DeterministicPeers::new(config, piece_store);

        assert_eq!(peers.config().seed, 12345);
        assert_eq!(peers.config().message_delay_ms, 50);
    }

    #[tokio::test]
    async fn test_ideal_network_conditions() {
        let piece_store = create_test_piece_store();
        let peers = DeterministicPeers::new_ideal(piece_store);

        assert_eq!(peers.config().connection_failure_rate, 0.0);
        assert_eq!(peers.config().message_loss_rate, 0.0);
        assert_eq!(peers.config().message_delay_ms, 1);
    }

    #[tokio::test]
    async fn test_poor_network_conditions() {
        let piece_store = create_test_piece_store();
        let peers = DeterministicPeers::new_poor(piece_store);

        assert_eq!(peers.config().connection_failure_rate, 0.15);
        assert_eq!(peers.config().message_loss_rate, 0.05);
        assert_eq!(peers.config().message_delay_ms, 200);
    }

    #[tokio::test]
    async fn test_deterministic_randomness() {
        let piece_store = create_test_piece_store();
        let config = DeterministicConfig::with_seed(42);
        let peers = DeterministicPeers::new(config, piece_store);

        // Same seed should produce same random sequence
        let random1 = peers.next_random().await;
        let random2 = peers.next_random().await;

        // Create new instance with same seed
        let peers2 = DeterministicPeers::new(
            DeterministicConfig::with_seed(42),
            create_test_piece_store(),
        );
        let random1_repeat = peers2.next_random().await;
        let random2_repeat = peers2.next_random().await;

        assert_eq!(random1, random1_repeat);
        assert_eq!(random2, random2_repeat);
    }

    #[tokio::test]
    async fn test_peer_connection_success() {
        let piece_store = create_test_piece_store();
        let config = DeterministicConfig::ideal(); // No failures
        let mut peers = DeterministicPeers::new(config, piece_store);

        let peer_addr = "127.0.0.1:8080".parse().unwrap();
        let info_hash = InfoHash::new([1u8; 20]);
        let peer_id = PeerId::new([0u8; 20]);

        let result = peers.connect_peer(peer_addr, info_hash, peer_id).await;
        assert!(result.is_ok());

        let connected = peers.connected_peers().await;
        assert_eq!(connected.len(), 1);
        assert_eq!(connected[0].address, peer_addr);

        let stats = peers.simulation_stats().await;
        assert_eq!(stats.connections_attempted, 1);
        assert_eq!(stats.connections_successful, 1);
        assert_eq!(stats.connections_failed, 0);
    }
}
