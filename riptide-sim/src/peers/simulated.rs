//! Deterministic peer implementation for simulation testing and bug reproduction.
//!
//! Provides fully deterministic, reproducible peer behavior with configurable
//! network conditions, failure injection, and precise timing control.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use bytes::Bytes;
use riptide_core::torrent::{
    ConnectionStatus, InfoHash, PeerId, PeerInfo, PeerManager, PeerMessage, PeerMessageEvent,
    PieceIndex, PieceStore, TorrentError,
};
use tokio::sync::{Mutex, RwLock, mpsc};
use tokio::time::sleep;

/// Simulation speed configuration for controlling peer behavior timing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SimulationSpeed {
    /// Instant responses with minimal delays for fast development iteration
    Instant,
    /// Realistic network timing with delays and failures for comprehensive testing
    Realistic,
}

/// Configuration for simulated peer simulation.
#[derive(Debug, Clone)]
pub struct SimulatedConfig {
    /// Speed mode controlling delays and failure rates
    pub simulation_speed: SimulationSpeed,
    /// Base delay for message responses in milliseconds (used in Realistic mode)
    pub message_delay_ms: u64,
    /// Probability of connection failure (0.0 to 1.0, used in Realistic mode)
    pub connection_failure_rate: f64,
    /// Probability of message loss (0.0 to 1.0, used in Realistic mode)
    pub message_loss_rate: f64,
    /// Maximum number of simultaneous connections
    pub max_connections: usize,
    /// Upload rate per peer in bytes per second
    pub upload_rate_bps: u64,
    /// Target streaming rate for development mode (bytes per second)
    pub streaming_rate_bps: u64,
    /// Random seed for deterministic behavior
    pub seed: u64,
}

impl Default for SimulatedConfig {
    fn default() -> Self {
        Self {
            simulation_speed: SimulationSpeed::Realistic,
            message_delay_ms: 50,          // Realistic internet delay
            connection_failure_rate: 0.05, // 5% connection failures
            message_loss_rate: 0.01,       // 1% message loss
            max_connections: 50,
            upload_rate_bps: 1024 * 1024,        // 1 MB/s
            streaming_rate_bps: 8 * 1024 * 1024, // 8 MB/s for development
            seed: 12345,                         // Fixed seed for reproducibility
        }
    }
}

impl SimulatedConfig {
    /// Configuration for instant responses (development mode).
    pub fn instant() -> Self {
        Self {
            simulation_speed: SimulationSpeed::Instant,
            message_delay_ms: 1,
            connection_failure_rate: 0.0,
            message_loss_rate: 0.0,
            max_connections: 100,
            upload_rate_bps: 10 * 1024 * 1024,   // 10 MB/s
            streaming_rate_bps: 8 * 1024 * 1024, // 8 MB/s for development
            seed: 12345,
        }
    }

    /// Configuration for ideal network conditions (no failures, minimal delay).
    pub fn ideal() -> Self {
        Self {
            simulation_speed: SimulationSpeed::Realistic,
            message_delay_ms: 1,
            connection_failure_rate: 0.0,
            message_loss_rate: 0.0,
            max_connections: 100,
            upload_rate_bps: 10 * 1024 * 1024,   // 10 MB/s
            streaming_rate_bps: 8 * 1024 * 1024, // 8 MB/s for development
            seed: 12345,
        }
    }

    /// Configuration for poor network conditions (high latency, failures).
    pub fn poor() -> Self {
        Self {
            simulation_speed: SimulationSpeed::Realistic,
            message_delay_ms: 200,
            connection_failure_rate: 0.15, // 15% connection failures
            message_loss_rate: 0.05,       // 5% message loss
            max_connections: 20,
            upload_rate_bps: 256 * 1024,    // 256 KB/s
            streaming_rate_bps: 512 * 1024, // 512 KB/s for poor conditions
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

/// Simulated peer implementation for simulation testing.
///
/// Provides fully reproducible peer behavior for:
/// - Bug reproduction with identical conditions
/// - Regression testing with deterministic scenarios
/// - Performance testing with controlled network conditions
/// - Edge case testing with failure injection
/// - Fast development iteration with instant responses
pub struct SimulatedPeers<P: PieceStore> {
    /// Configuration parameters for simulation behavior
    config: SimulatedConfig,
    /// Storage backend for piece data
    piece_store: Arc<P>,
    /// Active simulated peers indexed by address
    peers: Arc<RwLock<HashMap<SocketAddr, SimulatedPeer>>>,
    /// Channel for sending peer message events
    message_sender: mpsc::UnboundedSender<PeerMessageEvent>,
    /// Channel for receiving peer message events
    message_receiver: Arc<Mutex<mpsc::UnboundedReceiver<PeerMessageEvent>>>,
    /// Simulation statistics and metrics
    stats: Arc<RwLock<SimulationStats>>,
    /// RNG state for deterministic random behavior
    rng_state: Arc<Mutex<u64>>,
    /// Counter for generating unique peer IDs
    next_peer_id: Arc<Mutex<u32>>,
    /// Rate limiting for development mode streaming
    rate_limiter: Arc<Mutex<StreamingRateLimiter>>,
    /// Current streaming position for each torrent (development mode)
    streaming_positions: Arc<RwLock<HashMap<InfoHash, u64>>>,
}

/// Realistic streaming rate limiter for development mode.
///
/// Simulates real-world BitTorrent performance for development testing
/// while maintaining fast iteration speeds.
#[derive(Debug)]
struct StreamingRateLimiter {
    /// Target transfer rate in bytes per second
    target_bytes_per_second: u64,
    /// Timestamp of last data transfer
    last_transfer_time: Instant,
    /// Bytes transferred in the current second
    bytes_this_second: u64,
}

impl StreamingRateLimiter {
    /// Creates rate limiter with configurable streaming rate.
    fn new(target_bytes_per_second: u64) -> Self {
        Self {
            target_bytes_per_second,
            last_transfer_time: Instant::now(),
            bytes_this_second: 0,
        }
    }

    /// Applies rate limiting for realistic streaming simulation.
    async fn apply_rate_limit(&mut self, bytes_transferred: u64) {
        let now = Instant::now();
        let elapsed = now.duration_since(self.last_transfer_time);

        // Reset counter every second
        if elapsed >= Duration::from_secs(1) {
            self.bytes_this_second = 0;
            self.last_transfer_time = now;
        }

        self.bytes_this_second += bytes_transferred;

        // Apply gentle throttling if exceeding target rate
        if self.bytes_this_second > self.target_bytes_per_second {
            let delay_ms = 50; // Light throttling for realism
            sleep(Duration::from_millis(delay_ms)).await;
        }
    }

    /// Updates the target transfer rate.
    fn update_target_rate(&mut self, bytes_per_second: u64) {
        self.target_bytes_per_second = bytes_per_second;
    }
}

/// Simulated peer for deterministic testing.
#[derive(Debug, Clone)]
struct SimulatedPeer {
    /// Network address of this peer
    address: SocketAddr,
    /// Info hash of the torrent this peer serves
    info_hash: InfoHash,
    /// Unique identifier for this peer
    peer_id: PeerId,
    /// Current connection status
    status: ConnectionStatus,
    /// Timestamp when connection was established
    connected_at: Option<Instant>,
    /// Last time this peer was active
    last_activity: Instant,
    /// Bitfield of which pieces this peer has available
    available_pieces: Vec<bool>,
    /// Total bytes downloaded from this peer
    bytes_downloaded: u64,
    /// Total bytes uploaded to this peer
    bytes_uploaded: u64,
}

/// Statistics for deterministic simulation tracking.
#[derive(Debug, Default, Clone)]
pub struct SimulationStats {
    /// Total number of connection attempts made
    pub connections_attempted: u64,
    /// Number of successful connections established
    pub connections_successful: u64,
    /// Number of connection attempts that failed
    pub connections_failed: u64,
    /// Total protocol messages sent
    pub messages_sent: u64,
    /// Number of messages lost due to network simulation
    pub messages_lost: u64,
    /// Total pieces served to peers
    pub pieces_served: u64,
    /// Total bytes served to peers
    pub bytes_served: u64,
    /// Timestamp when simulation began
    pub simulation_start: Option<Instant>,
}

impl SimulatedPeer {
    /// Creates a new simulated peer with specified configuration.
    fn new(address: SocketAddr, info_hash: InfoHash, peer_id: PeerId, piece_count: u32) -> Self {
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

impl<P: PieceStore> SimulatedPeers<P> {
    /// Creates new simulated peer manager with specified configuration.
    pub fn new(config: SimulatedConfig, piece_store: Arc<P>) -> Self {
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
            rate_limiter: Arc::new(Mutex::new(StreamingRateLimiter::new(
                config.streaming_rate_bps,
            ))),
            streaming_positions: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Creates simulated peer manager with instant responses for development.
    pub fn new_instant(piece_store: Arc<P>) -> Self {
        Self::new(SimulatedConfig::instant(), piece_store)
    }

    /// Creates simulated peer manager with ideal network conditions.
    pub fn new_ideal(piece_store: Arc<P>) -> Self {
        Self::new(SimulatedConfig::ideal(), piece_store)
    }

    /// Creates simulated peer manager with poor network conditions.
    pub fn new_poor(piece_store: Arc<P>) -> Self {
        Self::new(SimulatedConfig::poor(), piece_store)
    }

    /// Returns current simulation statistics.
    pub async fn simulation_stats(&self) -> SimulationStats {
        self.stats.read().await.clone()
    }

    /// Returns configuration used for this simulation.
    pub fn config(&self) -> &SimulatedConfig {
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
        match self.config.simulation_speed {
            SimulationSpeed::Instant => false,
            SimulationSpeed::Realistic => {
                let random = self.next_random().await;
                random < self.config.connection_failure_rate
            }
        }
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
impl<P: PieceStore + Send + Sync + 'static> PeerManager for SimulatedPeers<P> {
    /// Connects to a peer with simulated behavior and failure simulation.
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
                reason: format!("Simulated connection failure for {peer_address}"),
            });
        }

        // Simulate connection delay
        sleep(self.message_delay()).await;

        let peer_id = self.generate_peer_id().await;
        let piece_count = self.piece_store.piece_count(info_hash).unwrap_or(100);

        let mut peer = SimulatedPeer::new(peer_address, info_hash, peer_id, piece_count);
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

        // Update peer activity and log with peer identification
        {
            let mut peers = self.peers.write().await;
            if let Some(peer) = peers.get_mut(&peer_address) {
                peer.update_activity();
                tracing::trace!(
                    "Updated activity for peer {} (id: {:?})",
                    peer_address,
                    peer.peer_id
                );
            }
        }

        // Process specific message types
        match message {
            PeerMessage::Request {
                piece_index,
                offset,
                length,
            } => {
                // Get piece data from piece store - use the peer's info_hash
                let peer_info_hash = {
                    let peers = self.peers.read().await;
                    peers.get(&peer_address).map(|p| p.info_hash)
                };

                if let Some(info_hash) = peer_info_hash {
                    // Check if peer has the requested piece before serving
                    let peer_has_piece = {
                        let peers = self.peers.read().await;
                        peers
                            .get(&peer_address)
                            .map(|p| p.has_piece(piece_index))
                            .unwrap_or(false)
                    };

                    if !peer_has_piece {
                        tracing::debug!(
                            "Peer {} does not have piece {}, ignoring request",
                            peer_address,
                            piece_index
                        );
                        return Ok(());
                    }

                    match self.piece_store.piece_data(info_hash, piece_index).await {
                        Ok(piece_data) => {
                            // Extract requested range
                            let start = offset as usize;
                            let end = std::cmp::min(start + length as usize, piece_data.len());
                            let data = Bytes::from(piece_data[start..end].to_vec());

                            let bytes_transferred = data.len() as u64;

                            // Update peer upload stats
                            {
                                let mut peers = self.peers.write().await;
                                if let Some(peer) = peers.get_mut(&peer_address) {
                                    peer.bytes_uploaded += bytes_transferred;
                                    tracing::debug!(
                                        "Peer {} (id: {:?}) uploaded {} bytes for piece {}",
                                        peer_address,
                                        peer.peer_id,
                                        bytes_transferred,
                                        piece_index
                                    );
                                }
                            }

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

                            if self.message_sender.send(event).is_err() {
                                return Err(TorrentError::PeerConnectionError {
                                    reason: "Message channel closed".to_string(),
                                });
                            }
                        }
                        Err(_) => {
                            // Piece not available, ignore request
                        }
                    }
                } else {
                    // Peer not found, ignore request
                }
            }
            PeerMessage::Piece {
                piece_index,
                offset,
                data,
            } => {
                let bytes_transferred = data.len() as u64;

                // Update peer download stats
                {
                    let mut peers = self.peers.write().await;
                    if let Some(peer) = peers.get_mut(&peer_address) {
                        peer.bytes_downloaded += bytes_transferred;
                        tracing::debug!(
                            "Peer {} (id: {:?}) downloaded {} bytes for piece {}",
                            peer_address,
                            peer.peer_id,
                            bytes_transferred,
                            piece_index
                        );
                    }
                }

                // Apply rate limiting for development mode (Instant)
                if self.config.simulation_speed == SimulationSpeed::Instant {
                    let mut rate_limiter = self.rate_limiter.lock().await;
                    rate_limiter.apply_rate_limit(bytes_transferred).await;
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

                if self.message_sender.send(event).is_err() {
                    return Err(TorrentError::PeerConnectionError {
                        reason: "Message channel closed".to_string(),
                    });
                }
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
            "SimulatedPeers: Simulation complete - {} connections, {} messages, {} pieces served",
            stats.connections_successful,
            stats.messages_sent,
            stats.pieces_served
        );

        Ok(())
    }

    /// Configure upload manager with rate limiting support for development mode.
    async fn configure_upload(
        &mut self,
        info_hash: InfoHash,
        piece_size: u64,
        total_bandwidth: u64,
    ) -> Result<(), TorrentError> {
        // Update rate limiter in Instant mode (development)
        if self.config.simulation_speed == SimulationSpeed::Instant {
            let mut rate_limiter = self.rate_limiter.lock().await;
            rate_limiter.update_target_rate(total_bandwidth.min(50 * 1024 * 1024)); // Cap at 50 MB/s

            tracing::debug!(
                "SimulatedPeers: Configure upload for {} - piece_size: {}, bandwidth: {}",
                info_hash,
                piece_size,
                total_bandwidth
            );
        }
        Ok(())
    }

    /// Update streaming position with tracking support for development mode.
    async fn update_streaming_position(
        &mut self,
        info_hash: InfoHash,
        byte_position: u64,
    ) -> Result<(), TorrentError> {
        // Track streaming position in Instant mode (development)
        if self.config.simulation_speed == SimulationSpeed::Instant {
            let mut positions = self.streaming_positions.write().await;
            positions.insert(info_hash, byte_position);

            tracing::debug!(
                "SimulatedPeers: Update streaming position for {} to byte {}",
                info_hash,
                byte_position
            );
        }
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
        let config = SimulatedConfig::default();
        let peers = SimulatedPeers::new(config, piece_store);

        assert_eq!(peers.config().seed, 12345);
        assert_eq!(peers.config().message_delay_ms, 50);
    }

    #[tokio::test]
    async fn test_ideal_network_conditions() {
        let piece_store = create_test_piece_store();
        let peers = SimulatedPeers::new_ideal(piece_store);

        assert_eq!(peers.config().connection_failure_rate, 0.0);
        assert_eq!(peers.config().message_loss_rate, 0.0);
        assert_eq!(peers.config().message_delay_ms, 1);
    }

    #[tokio::test]
    async fn test_poor_network_conditions() {
        let piece_store = create_test_piece_store();
        let peers = SimulatedPeers::new_poor(piece_store);

        assert_eq!(peers.config().connection_failure_rate, 0.15);
        assert_eq!(peers.config().message_loss_rate, 0.05);
        assert_eq!(peers.config().message_delay_ms, 200);
    }

    #[tokio::test]
    async fn test_deterministic_randomness() {
        let piece_store = create_test_piece_store();
        let config = SimulatedConfig::with_seed(42);
        let peers = SimulatedPeers::new(config, piece_store);

        // Same seed should produce same random sequence
        let random1 = peers.next_random().await;
        let random2 = peers.next_random().await;

        // Create new instance with same seed
        let peers2 = SimulatedPeers::new(SimulatedConfig::with_seed(42), create_test_piece_store());
        let random1_repeat = peers2.next_random().await;
        let random2_repeat = peers2.next_random().await;

        assert_eq!(random1, random1_repeat);
        assert_eq!(random2, random2_repeat);
    }

    #[tokio::test]
    async fn test_peer_connection_success() {
        let piece_store = create_test_piece_store();
        let config = SimulatedConfig::ideal(); // No failures
        let mut peers = SimulatedPeers::new(config, piece_store);

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
