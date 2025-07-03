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

pub use bandwidth::{BandwidthAllocator, GlobalBandwidthManager};
pub use connection::{ChokingReason, EnhancedConnectionState, EnhancedPeerConnection};
pub use metrics::{
    BehavioralFlags, ConnectionHealth, ConnectionMetrics, ExponentialMovingAverage,
    GlobalPeerStats, PeerQualityTracker, PeerRanking,
};
pub use queue::{PendingRequest, Priority, PriorityRequestQueue};

use crate::config::RiptideConfig;
use crate::torrent::{InfoHash, PieceIndex, TorrentError};

/// Enhanced peer manager with bandwidth control and performance-based peer selection
pub struct EnhancedPeerManager {
    torrents: HashMap<InfoHash, TorrentPeerPool>,
    _bandwidth_manager: GlobalBandwidthManager,
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
    pub info_hash: InfoHash,
    pub piece_index: PieceIndex,
    pub piece_size: u32,
    pub priority: Priority,
    pub deadline: Option<Instant>,
}

/// Result of a piece download request
#[derive(Debug)]
pub enum PieceResult {
    Success { data: Vec<u8> },
    Failed { reason: String },
    Timeout,
}

/// Statistics for the enhanced peer manager
#[derive(Debug, Default)]
pub struct EnhancedPeerManagerStats {
    pub total_connections: usize,
    pub active_connections: usize,
    pub total_uploaded: u64,
    pub total_downloaded: u64,
    pub bandwidth_utilization: BandwidthUtilizationStats,
}

/// Bandwidth utilization statistics
#[derive(Debug, Default)]
pub struct BandwidthUtilizationStats {
    pub upload_rate_mbps: f64,
    pub download_rate_mbps: f64,
    pub peak_upload_rate_mbps: f64,
    pub peak_download_rate_mbps: f64,
}

impl EnhancedPeerManager {
    /// Create a new enhanced peer manager
    pub fn new(config: RiptideConfig) -> Self {
        Self {
            torrents: HashMap::new(),
            _bandwidth_manager: GlobalBandwidthManager::new(),
            _config: config,
        }
    }

    /// Add a peer to a torrent's peer pool
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
    pub async fn request_piece_prioritized(
        &self,
        params: PieceRequestParams,
    ) -> Result<PieceResult, TorrentError> {
        let _pool = self.torrents.get(&params.info_hash).ok_or_else(|| {
            TorrentError::PeerConnectionError {
                reason: format!("Torrent not found: {}", params.info_hash),
            }
        })?;

        // Simplified implementation for now - return timeout
        Ok(PieceResult::Timeout)
    }

    /// Start background tasks for peer management
    ///
    /// # Errors
    /// - `TorrentError::ProtocolError` - Background task initialization failed
    pub async fn start_background_tasks(&self) -> Result<(), TorrentError> {
        // Background cleanup, connection monitoring, etc.
        Ok(())
    }

    /// Get enhanced statistics
    pub async fn get_enhanced_stats(&self) -> EnhancedPeerManagerStats {
        self.statistics()
    }

    /// Statistics for the peer manager
    pub fn statistics(&self) -> EnhancedPeerManagerStats {
        let total_connections = self.torrents.values().map(|pool| pool.peers.len()).sum();

        let active_connections = self
            .torrents
            .values()
            .flat_map(|pool| pool.peers.values())
            .filter(|conn| matches!(conn.state, EnhancedConnectionState::Connected))
            .count();

        EnhancedPeerManagerStats {
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
