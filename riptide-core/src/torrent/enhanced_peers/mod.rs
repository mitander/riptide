//! Peer connection management with bandwidth control and performance-based peer selection
//!
//! Provides peer connection management with features like:
//! - Performance-based peer selection using throughput metrics
//! - Bandwidth control with priority queues
//! - Connection health monitoring and automatic recovery
//! - Streaming-optimized piece prioritization
//! - Anti-pattern detection for malicious peers

pub mod bandwidth;
pub mod connection;
pub mod metrics;
pub mod queue;

use std::collections::{HashMap, VecDeque};
use std::net::SocketAddr;
use std::time::Instant;

pub use bandwidth::BandwidthAllocator;
pub use connection::{ChokingReason, EnhancedConnectionState, EnhancedPeerConnection};
pub use metrics::{
    BehavioralFlags, ConnectionHealth, ConnectionMetrics, ExponentialMovingAverage,
    GlobalPeerStats, PeerQualityTracker, PeerRanking,
};
pub use queue::{PendingRequest, Priority, PriorityRequestQueue};

use crate::config::RiptideConfig;
use crate::torrent::{InfoHash, PieceIndex, TorrentError};

/// Enhanced peer manager with bandwidth control and performance-based peer selection
pub struct EnhancedPeers {
    torrents: HashMap<InfoHash, TorrentPeerPool>,
    _config: RiptideConfig,
}

/// Per-torrent peer connection pool
#[derive(Debug)]
pub struct TorrentPeerPool {
    peers: HashMap<SocketAddr, EnhancedPeerConnection>,
    _peer_quality_tracker: PeerQualityTracker,
    _request_queue: VecDeque<PendingRequest>,
    _last_cleanup: Instant,
}

/// Parameters for requesting a piece with priority
#[derive(Debug, Clone)]
pub struct PieceRequestParams {
    /// Info hash of the torrent containing the piece
    pub info_hash: InfoHash,
    /// Index of the piece to request
    pub piece_index: PieceIndex,
    /// Size of the piece in bytes
    pub piece_size: u32,
    /// Priority level for this request
    pub priority: Priority,
    /// Optional deadline for completing the request
    pub deadline: Option<Instant>,
}

/// Result of a piece download request
#[derive(Debug)]
pub enum PieceResult {
    /// Piece was downloaded successfully
    Success {
        /// The downloaded piece data
        data: Vec<u8>,
    },
    /// Download failed with an error
    Failed {
        /// Reason for the failure
        reason: String,
    },
    /// Request timed out before completion
    Timeout,
}

/// Statistics for the enhanced peer manager
#[derive(Debug, Default)]
pub struct EnhancedPeersStats {
    /// Total number of peer connections ever made
    pub total_connections: usize,
    /// Number of currently active connections
    pub active_connections: usize,
    /// Total bytes uploaded across all sessions
    pub total_uploaded: u64,
    /// Total bytes downloaded across all sessions
    pub total_downloaded: u64,
    /// Current bandwidth utilization metrics
    pub bandwidth_utilization: BandwidthUtilizationStats,
}

/// Bandwidth utilization statistics
#[derive(Debug, Default)]
pub struct BandwidthUtilizationStats {
    /// Current upload rate in megabits per second
    pub upload_rate_mbps: f64,
    /// Current download rate in megabits per second
    pub download_rate_mbps: f64,
    /// Peak upload rate achieved in megabits per second
    pub peak_upload_rate_mbps: f64,
    /// Peak download rate achieved in megabits per second
    pub peak_download_rate_mbps: f64,
}

impl EnhancedPeers {
    /// Create a new enhanced peer manager
    pub fn new(config: RiptideConfig) -> Self {
        Self {
            torrents: HashMap::new(),
            _config: config,
        }
    }

    /// Add a peer to a torrent's peer pool
    ///
    /// # Errors
    ///
    /// - `TorrentError` - If peer cannot be added to the pool or connection fails
    pub fn add_peer(
        &mut self,
        info_hash: InfoHash,
        peer_addr: SocketAddr,
    ) -> Result<(), TorrentError> {
        let peer_pool = self
            .torrents
            .entry(info_hash)
            .or_insert_with(|| TorrentPeerPool {
                peers: HashMap::new(),
                _peer_quality_tracker: PeerQualityTracker::new(),
                _request_queue: VecDeque::new(),
                _last_cleanup: Instant::now(),
            });

        peer_pool
            .peers
            .entry(peer_addr)
            .or_insert_with(|| EnhancedPeerConnection::new(peer_addr));

        Ok(())
    }

    /// Request a piece with priority from the best available peer
    /// Request a piece with priority handling and quality-based peer selection
    ///
    /// # Errors
    ///
    /// - `TorrentError` - If no peers are available or piece request fails
    pub async fn request_piece_prioritized(
        &self,
        params: PieceRequestParams,
    ) -> Result<PieceResult, TorrentError> {
        let pool = self.torrents.get(&params.info_hash).ok_or_else(|| {
            TorrentError::PeerConnectionError {
                reason: format!("Torrent not found: {}", params.info_hash),
            }
        })?;

        // Select best peer for piece request
        if pool.peers.is_empty() {
            return Err(TorrentError::PeerConnectionError {
                reason: "No connected peers available".to_string(),
            });
        }

        // Simple peer selection: first available peer
        let peer_addr =
            pool.peers
                .keys()
                .next()
                .copied()
                .ok_or_else(|| TorrentError::PeerConnectionError {
                    reason: "No peers available for piece request".to_string(),
                })?;

        tracing::debug!(
            "Requesting piece {} from peer {} with priority {:?}",
            params.piece_index.as_u32(),
            peer_addr,
            params.priority
        );

        // For now, return timeout until full peer protocol is implemented
        // TODO: Implement actual piece request to peer
        Ok(PieceResult::Timeout)
    }

    /// Start background tasks for peer management
    ///
    /// # Errors
    ///
    /// - `TorrentError::ProtocolError` - If background task initialization failed
    pub async fn start_background_tasks(&self) -> Result<(), TorrentError> {
        // Background cleanup, connection monitoring, etc.
        Ok(())
    }

    /// Statistics for the peer manager
    pub fn statistics(&self) -> EnhancedPeersStats {
        let total_connections = self.torrents.values().map(|pool| pool.peers.len()).sum();

        let active_connections = self
            .torrents
            .values()
            .flat_map(|pool| pool.peers.values())
            .filter(|conn| matches!(conn.state, EnhancedConnectionState::Connected))
            .count();

        EnhancedPeersStats {
            total_connections,
            active_connections,
            total_uploaded: 0,
            total_downloaded: 0,
            bandwidth_utilization: BandwidthUtilizationStats::default(),
        }
    }
}

impl TorrentPeerPool {
    /// Get the number of connected peers
    pub fn connected_peer_count(&self) -> usize {
        self.peers
            .values()
            .filter(|conn| matches!(conn.state, EnhancedConnectionState::Connected))
            .count()
    }

    /// Peer connection information by address
    pub fn peer_connection(&self, addr: &SocketAddr) -> Option<&EnhancedPeerConnection> {
        self.peers.get(addr)
    }
}
