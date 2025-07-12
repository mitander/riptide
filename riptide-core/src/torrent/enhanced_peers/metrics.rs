//! Connection metrics and quality tracking

use std::collections::HashMap;
use std::net::SocketAddr;
use std::time::Instant;

/// Connection performance metrics
#[derive(Debug, Clone, Default)]
pub struct ConnectionMetrics {
    /// Bytes uploaded in current session
    pub bytes_uploaded: u64,
    /// Bytes downloaded in current session
    pub bytes_downloaded: u64,
    /// Total bytes uploaded across all sessions
    pub total_uploaded: u64,
    /// Total bytes downloaded across all sessions
    pub total_downloaded: u64,
    /// Current upload rate in bytes per second
    pub upload_rate_bytes_per_sec: u32,
    /// Current download rate in bytes per second
    pub download_rate_bytes_per_sec: u32,
    /// Number of pieces successfully completed
    pub pieces_completed: u32,
    /// Number of pieces that failed verification
    pub pieces_failed: u32,
    /// Average time to complete a piece in milliseconds
    pub average_piece_time_ms: u32,
}

/// Connection health monitoring
#[derive(Debug, Clone)]
pub struct ConnectionHealth {
    /// Whether the peer is currently responsive
    pub is_responsive: bool,
    /// Number of consecutive failures detected
    pub consecutive_failures: u32,
    /// Timestamp of last successful response
    pub last_response_time: Option<Instant>,
    /// Average round-trip latency in milliseconds
    pub average_latency_ms: u32,
    /// Packet loss rate as a fraction (0.0 to 1.0)
    pub packet_loss_rate: f32,
    /// Connection stability metric (0.0 to 1.0)
    pub connection_stability: f32,
}

/// Behavioral pattern flags for peer assessment
#[derive(Debug, Clone)]
pub struct BehavioralFlags {
    /// Whether the peer has all pieces (is a seeder)
    pub is_seed: bool,
    /// Whether the peer exhibits suspicious behavior
    pub appears_malicious: bool,
    /// Whether the peer respects choking protocol
    pub honors_choking: bool,
    /// Whether the peer supports BitTorrent fast extension
    pub supports_fast_extension: bool,
    /// Whether the peer prefers encrypted connections
    pub prefers_encryption: bool,
}

/// Peer quality tracker for performance-based selection
#[derive(Debug)]
pub struct PeerQualityTracker {
    _peer_rankings: HashMap<SocketAddr, PeerRanking>,
    _global_stats: GlobalPeerStats,
}

/// Individual peer performance ranking
#[derive(Debug, Clone)]
pub struct PeerRanking {
    /// Overall quality score (0.0 to 1.0)
    pub quality_score: f64,
    /// Reliability score based on connection stability
    pub reliability_score: f64,
    /// Speed score based on download performance
    pub speed_score: f64,
    /// When this ranking was last updated
    pub last_updated: Instant,
}

/// Global peer statistics
#[derive(Debug, Default)]
pub struct GlobalPeerStats {
    /// Total number of unique peers encountered
    pub total_peers_seen: u64,
    /// Average download speed across all peers
    pub average_download_speed: f64,
    /// Highest recorded download speed from any peer
    pub best_peer_speed: f64,
    /// Rate at which peers connect and disconnect
    pub peer_churn_rate: f64,
}

/// Exponential moving average for rate calculations
#[derive(Debug)]
pub struct ExponentialMovingAverage {
    value: f64,
    alpha: f64,
}

impl ConnectionMetrics {
    /// Update download metrics
    pub fn update_download(&mut self, bytes: u64, rate: u32) {
        self.bytes_downloaded += bytes;
        self.total_downloaded += bytes;
        self.download_rate_bytes_per_sec = rate;
    }

    /// Update upload metrics
    pub fn update_upload(&mut self, bytes: u64, rate: u32) {
        self.bytes_uploaded += bytes;
        self.total_uploaded += bytes;
        self.upload_rate_bytes_per_sec = rate;
    }

    /// Record piece completion
    pub fn record_piece_success(&mut self, time_ms: u32) {
        self.pieces_completed += 1;
        self.average_piece_time_ms = (self.average_piece_time_ms * (self.pieces_completed - 1)
            + time_ms)
            / self.pieces_completed;
    }

    /// Record piece failure
    pub fn record_piece_failure(&mut self) {
        self.pieces_failed += 1;
    }
}

impl ConnectionHealth {
    /// Record successful operation
    pub fn record_success(&mut self) {
        self.is_responsive = true;
        self.consecutive_failures = 0;
        self.last_response_time = Some(Instant::now());
    }

    /// Record failed operation
    pub fn record_failure(&mut self) {
        self.consecutive_failures += 1;
        if self.consecutive_failures > 3 {
            self.is_responsive = false;
        }
    }

    /// Check if connection is healthy
    pub fn is_healthy(&self) -> bool {
        self.is_responsive && self.consecutive_failures < 3
    }

    /// Calculate health score (0.0 to 1.0)
    pub fn health_score(&self) -> f64 {
        if !self.is_responsive {
            return 0.0;
        }

        let failure_penalty = (self.consecutive_failures as f64 * 0.2).min(0.8);
        let latency_penalty = (self.average_latency_ms as f64 / 1000.0).min(0.5);
        let loss_penalty = self.packet_loss_rate as f64;

        (1.0 - failure_penalty - latency_penalty - loss_penalty).max(0.0)
    }
}

impl PeerQualityTracker {
    /// Create a new peer quality tracker
    pub fn new() -> Self {
        Self {
            _peer_rankings: HashMap::new(),
            _global_stats: GlobalPeerStats::default(),
        }
    }
}

impl Default for PeerQualityTracker {
    fn default() -> Self {
        Self::new()
    }
}

impl ExponentialMovingAverage {
    /// Create new EMA with given smoothing factor
    pub fn new(alpha: f64) -> Self {
        Self { value: 0.0, alpha }
    }

    /// Update the average with a new value
    pub fn update(&mut self, new_value: f64) {
        self.value = self.alpha * new_value + (1.0 - self.alpha) * self.value;
    }

    /// Returns the current average value for testing purposes.
    ///
    /// This function is only available in test builds and provides
    /// access to the internal averaged value for verification.
    #[cfg(test)]
    pub fn current_average(&self) -> f64 {
        self.value
    }
}

impl Default for ConnectionHealth {
    fn default() -> Self {
        Self {
            is_responsive: true,
            consecutive_failures: 0,
            last_response_time: None,
            average_latency_ms: 0,
            packet_loss_rate: 0.0,
            connection_stability: 1.0,
        }
    }
}

impl Default for BehavioralFlags {
    fn default() -> Self {
        Self {
            is_seed: false,
            appears_malicious: false,
            honors_choking: true,
            supports_fast_extension: false,
            prefers_encryption: false,
        }
    }
}
