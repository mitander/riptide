//! Type definitions for the remux streaming system

use std::path::PathBuf;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::torrent::InfoHash;

/// Container format for video files
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ContainerFormat {
    Mp4,
    Mkv,
    Avi,
    Mov,
    WebM,
    Unknown,
}

/// Errors that can occur during streaming operations
#[derive(Debug, thiserror::Error)]
pub enum StrategyError {
    #[error("Container format not supported: {format}")]
    UnsupportedFormat { format: String },

    #[error("Failed to detect container format from headers")]
    FormatDetectionFailed,

    #[error("Streaming not ready: {reason}")]
    StreamingNotReady { reason: String },

    #[error("Remuxing failed: {reason}")]
    RemuxingFailed { reason: String },

    #[error("Invalid range request: {range:?}")]
    InvalidRange { range: std::ops::Range<u64> },

    #[error("FFmpeg error: {reason}")]
    FfmpegError { reason: String },

    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),

    #[error("Piece storage error: {reason}")]
    PieceStorageError { reason: String },

    #[error("Missing pieces for range")]
    MissingPieces { missing_count: usize },

    #[error("IO error during {operation}: {source}")]
    IoErrorWithOperation {
        operation: String,
        #[source]
        source: std::io::Error,
    },

    #[error("Torrent not found")]
    TorrentNotFound,

    #[error("Unsupported source format")]
    UnsupportedSource,

    #[error("Failed to add torrent: {reason}")]
    TorrentAddFailed { reason: String },

    #[error("Server failed to start: {reason}")]
    ServerStartFailed { reason: String },
}

/// Legacy alias for StreamingError
pub type StreamingError = StrategyError;

/// Result type for streaming operations
pub type StreamingResult<T> = Result<T, StrategyError>;

/// Configuration for remuxing operations
#[derive(Debug, Clone)]
pub struct RemuxConfig {
    /// Maximum number of concurrent remuxing sessions
    pub max_concurrent_sessions: usize,
    /// Directory for storing remuxed files
    pub cache_dir: PathBuf,
    /// Minimum head data size required before starting remux
    pub min_head_size: u64,
    /// Minimum tail data size required before starting remux
    pub min_tail_size: u64,
    /// Timeout for remuxing operations
    pub remux_timeout: Duration,
    /// FFmpeg binary path
    pub ffmpeg_path: PathBuf,
    /// Clean up completed sessions after this duration
    pub cleanup_after: Duration,
}

impl Default for RemuxConfig {
    fn default() -> Self {
        Self {
            max_concurrent_sessions: 3,
            cache_dir: PathBuf::from("/tmp/riptide_remux"),
            min_head_size: 3 * 1024 * 1024,              // 3MB
            min_tail_size: 2 * 1024 * 1024,              // 2MB
            remux_timeout: Duration::from_secs(30 * 60), // 30 minutes
            ffmpeg_path: PathBuf::from("ffmpeg"),
            cleanup_after: Duration::from_secs(60 * 60), // 1 hour
        }
    }
}

/// Handle to a streaming session
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct StreamHandle {
    pub info_hash: InfoHash,
    pub session_id: u64,
    pub format: ContainerFormat,
}

/// Streaming data with metadata
#[derive(Debug, Clone)]
pub struct StreamData {
    pub data: Vec<u8>,
    pub content_type: String,
    pub total_size: Option<u64>,
    pub range_start: u64,
    pub range_end: u64,
}

/// Readiness state for streaming
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum StreamReadiness {
    /// Stream is ready for serving
    Ready,
    /// Stream is being processed
    Processing,
    /// Waiting for more data to become available
    WaitingForData,
    /// Failed but can be retried
    CanRetry,
    /// Failed permanently
    Failed,
}

/// Overall streaming status with progress information
#[derive(Debug, Clone, Serialize)]
pub struct StreamingStatus {
    pub readiness: StreamReadiness,
    pub progress: Option<f32>, // 0.0 to 1.0
    pub estimated_time_remaining: Option<Duration>,
    pub error_message: Option<String>,
    #[serde(skip)] // Skip serialization for Instant
    pub last_activity: Instant,
}

/// Complete remuxing session data
#[derive(Debug)]
pub struct RemuxSession {
    pub info_hash: InfoHash,
    pub state: super::state::RemuxState,
    pub ffmpeg_handle: Option<std::process::Child>,
    pub output_path: Option<PathBuf>,
    pub progress: super::state::RemuxProgress,
    pub created_at: Instant,
    pub last_activity: Instant,
    pub session_id: u64,
}

impl RemuxSession {
    pub fn new(info_hash: InfoHash, session_id: u64) -> Self {
        let now = Instant::now();
        Self {
            info_hash,
            state: super::state::RemuxState::WaitingForHeadAndTail,
            ffmpeg_handle: None,
            output_path: None,
            progress: super::state::RemuxProgress::default(),
            created_at: now,
            last_activity: now,
            session_id,
        }
    }

    /// Update the last activity timestamp
    pub fn touch(&mut self) {
        self.last_activity = Instant::now();
    }

    /// Check if the session has been inactive for too long
    pub fn is_stale(&self, timeout: Duration) -> bool {
        self.last_activity.elapsed() > timeout
    }
}
