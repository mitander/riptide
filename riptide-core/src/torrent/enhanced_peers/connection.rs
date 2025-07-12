//! Peer connection state and management

use std::net::SocketAddr;
use std::time::Instant;

use super::metrics::{BehavioralFlags, ConnectionHealth, ConnectionMetrics};

/// Enhanced peer connection with detailed state tracking
#[derive(Debug)]
pub struct EnhancedPeerConnection {
    /// Network address of the peer
    pub peer_address: SocketAddr,
    /// Current connection state with detailed status
    pub state: EnhancedConnectionState,
    /// Performance and transfer metrics
    pub metrics: ConnectionMetrics,
    /// Connection health indicators
    pub health: ConnectionHealth,
    /// Behavioral analysis flags
    pub behavioral_flags: BehavioralFlags,
    /// When the connection was established
    pub connected_at: Option<Instant>,
    /// When the last message was received
    pub last_message_at: Option<Instant>,
}

/// Enhanced connection state with detailed status
#[derive(Debug, PartialEq, Eq)]
pub enum EnhancedConnectionState {
    /// Peer is not connected
    Disconnected,
    /// Attempting to establish connection
    Connecting,
    /// Performing BitTorrent handshake
    Handshaking,
    /// Fully connected and operational
    Connected,
    /// Connection is choked for specified reason
    Choked(ChokingReason),
    /// Connection failed with error details
    Error(String),
}

/// Reason for connection being choked
#[derive(Debug, PartialEq, Eq)]
pub enum ChokingReason {
    /// Remote peer has choked this connection
    PeerChoked,
    /// Local client has choked this peer
    LocalChoked,
    /// Connection exceeds bandwidth limits
    BandwidthLimit,
    /// Connection quality below threshold
    QualityThreshold,
    /// Connection timed out
    Timeout,
}

impl EnhancedPeerConnection {
    /// Create a new enhanced peer connection
    pub fn new(peer_address: SocketAddr) -> Self {
        Self {
            peer_address,
            state: EnhancedConnectionState::Disconnected,
            metrics: ConnectionMetrics::default(),
            health: ConnectionHealth::default(),
            behavioral_flags: BehavioralFlags::default(),
            connected_at: None,
            last_message_at: None,
        }
    }

    /// Check if connection is active
    pub fn is_active(&self) -> bool {
        matches!(self.state, EnhancedConnectionState::Connected)
    }

    /// Check if connection is usable for requests
    pub fn is_usable(&self) -> bool {
        self.is_active() && self.health.is_healthy()
    }

    /// Update connection state
    pub fn update_connection_state(&mut self, new_state: EnhancedConnectionState) {
        self.state = new_state;
        if matches!(self.state, EnhancedConnectionState::Connected) {
            self.connected_at = Some(Instant::now());
        }
    }

    /// Record a successful message exchange
    pub fn record_message(&mut self) {
        self.last_message_at = Some(Instant::now());
        self.health.record_success();
    }

    /// Record a failed operation
    pub fn record_failure(&mut self, reason: &str) {
        self.health.record_failure();
        if self.health.consecutive_failures > 3 {
            self.state = EnhancedConnectionState::Error(reason.to_string());
        }
    }

    /// Get connection uptime in seconds
    pub fn uptime_seconds(&self) -> Option<u64> {
        self.connected_at
            .map(|connected| connected.elapsed().as_secs())
    }

    /// Calculate connection quality score (0.0 to 1.0)
    pub fn quality_score(&self) -> f64 {
        if !self.is_active() {
            return 0.0;
        }

        let health_score = self.health.health_score();
        let throughput_score = if self.metrics.total_downloaded > 0 {
            (self.metrics.download_rate_bytes_per_sec as f64 / 1_000_000.0).min(1.0)
        } else {
            0.5 // Neutral score for new connections
        };

        (health_score + throughput_score) / 2.0
    }
}

impl Default for EnhancedConnectionState {
    fn default() -> Self {
        Self::Disconnected
    }
}
