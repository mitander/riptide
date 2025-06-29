//! Core types and structures for streaming coordination

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde::Serialize;
use tokio::sync::RwLock;

use crate::streaming::range_handler::{FileInfo, PiecePriority};
use crate::torrent::{
    EnhancedPeerManager, InfoHash, NetworkPeerManager, TorrentEngine, TorrentError, TrackerManager,
};

/// Coordinates streaming sessions between HTTP requests and BitTorrent backend.
///
/// Manages active streaming sessions, prioritizes piece downloads for streaming
/// performance, and maintains streaming buffers for smooth playback.
pub struct StreamCoordinator {
    // TODO: Use for querying torrent metadata and piece availability
    // Currently metadata is mocked - will integrate with engine for:
    // - Real-time piece availability checks
    // - Torrent metadata queries
    // - Download progress tracking
    pub(super) _torrent_engine: Arc<RwLock<TorrentEngine<NetworkPeerManager, TrackerManager>>>,
    pub(super) peer_manager: Arc<RwLock<EnhancedPeerManager>>,
    pub(super) active_sessions: Arc<RwLock<HashMap<InfoHash, StreamingSession>>>,
    pub(super) registered_torrents: Arc<RwLock<HashMap<InfoHash, TorrentMetadata>>>,
}

/// Active streaming session for a torrent.
///
/// Tracks streaming state, buffer requirements, and performance metrics
/// for piece prioritization and bandwidth allocation.
#[derive(Debug, Clone)]
pub struct StreamingSession {
    pub info_hash: InfoHash,
    pub current_position: u64,
    pub total_size: u64,
    pub buffer_state: StreamingBufferState,
    pub active_ranges: Vec<ActiveRange>,
    pub session_start: Instant,
    pub last_activity: Instant,
    pub bytes_served: u64,
    pub performance_metrics: StreamingPerformanceMetrics,
}

/// Current state of streaming buffer.
#[derive(Debug, Clone)]
pub struct StreamingBufferState {
    pub current_piece: u32,
    pub buffered_pieces: Vec<u32>,
    pub critical_pieces: Vec<u32>,
    pub prefetch_pieces: Vec<u32>,
    pub buffer_health: f64, // 0.0 (empty) to 1.0 (full)
}

/// Active byte range being streamed.
#[derive(Debug, Clone)]
pub struct ActiveRange {
    pub start: u64,
    pub end: u64,
    pub priority: PiecePriority,
    pub requested_at: Instant,
    pub estimated_completion: Option<Instant>,
}

/// Performance metrics for streaming.
#[derive(Debug, Clone)]
pub struct StreamingPerformanceMetrics {
    pub average_response_time: Duration,
    pub throughput_mbps: f64,
    pub buffer_underruns: u32,
    pub seek_count: u32,
    pub total_requests: u32,
}

/// Metadata for registered torrents.
#[derive(Debug, Clone, Serialize)]
pub struct TorrentMetadata {
    #[serde(with = "hex_serde")]
    pub info_hash: InfoHash,
    pub name: String,
    pub total_size: u64,
    pub piece_size: u32,
    pub total_pieces: u32,
    pub files: Vec<FileInfo>,
    #[serde(with = "serde_instant")]
    pub added_at: Instant,
    pub source: String,
}

pub(super) mod hex_serde {
    use serde::{Serialize, Serializer};

    use crate::torrent::InfoHash;

    /// Serialize InfoHash as hex string for JSON output.
    ///
    /// # Errors
    /// - Returns serializer error if hex encoding fails
    pub fn serialize<S>(info_hash: &InfoHash, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        hex::encode(info_hash.as_bytes()).serialize(serializer)
    }
}

pub(super) mod serde_instant {
    use std::time::{Instant, SystemTime, UNIX_EPOCH};

    use serde::{Serialize, Serializer};

    /// Serialize Instant as milliseconds since UNIX epoch.
    ///
    /// Converts Instant to SystemTime for serialization. Note: May not be accurate
    /// across system suspends or clock adjustments.
    ///
    /// # Errors
    /// - Returns serializer error if timestamp conversion fails
    pub fn serialize<S>(instant: &Instant, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        // Convert to seconds since Unix epoch for JSON compatibility
        // Note: This is approximate since Instant is not tied to wall clock
        let now_instant = Instant::now();
        let now_system = SystemTime::now();
        let duration_since_epoch = now_system.duration_since(UNIX_EPOCH).unwrap_or_default();
        let instant_duration =
            duration_since_epoch.saturating_sub(now_instant.duration_since(*instant));
        instant_duration.as_secs().serialize(serializer)
    }
}

/// Overall streaming statistics.
#[derive(Debug, Clone, Serialize)]
pub struct StreamingStats {
    pub active_sessions: usize,
    pub total_bytes_served: u64,
    pub average_buffer_health: f64,
    pub total_torrents: usize,
    pub successful_streams: u64,
}

/// Streaming service errors.
#[derive(Debug, thiserror::Error)]
pub enum StreamingError {
    #[error("Server failed to start on {address}: {reason}")]
    ServerStartFailed {
        address: std::net::SocketAddr,
        reason: String,
    },

    #[error("Torrent not found: {info_hash:?}")]
    TorrentNotFound { info_hash: InfoHash },

    #[error("File {file_index} not found in torrent {info_hash:?}")]
    FileNotFound {
        info_hash: InfoHash,
        file_index: usize,
    },

    #[error("Failed to add torrent: {reason}")]
    TorrentAddFailed { reason: String },

    #[error("Unsupported torrent source")]
    UnsupportedSource,

    #[error("Range request not satisfiable: {reason}")]
    UnsupportedRange { reason: String },

    #[error("Failed to read data: {reason}")]
    DataReadError { reason: String },

    #[error("HTTP response error: {reason}")]
    ResponseError { reason: String },

    #[error("Streaming session error: {reason}")]
    SessionError { reason: String },

    #[error("Torrent engine error")]
    TorrentEngine(#[from] TorrentError),
}
