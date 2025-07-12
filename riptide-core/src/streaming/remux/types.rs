//! Type definitions for the remux streaming system

use std::path::PathBuf;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::torrent::InfoHash;

/// Container format for video files
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ContainerFormat {
    /// MPEG-4 Part 14 container (.mp4)
    Mp4,
    /// Matroska Video container (.mkv)
    Mkv,
    /// Audio Video Interleave container (.avi)
    Avi,
    /// QuickTime movie container (.mov)
    Mov,
    /// WebM container (.webm)
    WebM,
    /// Container format could not be determined
    Unknown,
}

/// Errors that can occur during streaming operations
#[derive(Debug, thiserror::Error)]
pub enum StrategyError {
    /// Container format is not supported for streaming
    #[error("Container format not supported: {format}")]
    UnsupportedFormat {
        /// Name of the unsupported container format
        format: String,
    },

    /// Failed to automatically detect container format from file headers
    #[error("Failed to detect container format from headers")]
    FormatDetectionFailed,

    /// Stream is not ready for serving content
    #[error("Streaming not ready: {reason}")]
    StreamingNotReady {
        /// Specific reason why streaming is not ready
        reason: String,
    },

    /// FFmpeg remuxing process failed
    #[error("Remuxing failed: {reason}")]
    RemuxingFailed {
        /// Specific reason for remuxing failure
        reason: String,
    },

    /// HTTP range request is invalid or out of bounds
    #[error("Invalid range request: {range:?}")]
    InvalidRange {
        /// The invalid byte range that was requested
        range: std::ops::Range<u64>,
    },

    /// FFmpeg process encountered an error
    #[error("FFmpeg error: {reason}")]
    FfmpegError {
        /// Error message from FFmpeg process
        reason: String,
    },

    /// Underlying I/O operation failed
    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),

    /// Error accessing or storing piece data
    #[error("Piece storage error: {reason}")]
    PieceStorageError {
        /// Specific reason for storage failure
        reason: String,
    },

    /// Required pieces are not available for requested range
    #[error("Missing pieces for range")]
    MissingPieces {
        /// Number of pieces that are missing
        missing_count: usize,
    },

    /// I/O error occurred during a specific operation
    #[error("IO error during {operation}: {source}")]
    IoErrorWithOperation {
        /// Description of the operation that failed
        operation: String,
        /// The underlying I/O error
        #[source]
        source: std::io::Error,
    },

    /// Requested torrent session does not exist
    #[error("Torrent not found")]
    TorrentNotFound,

    /// Source file format is not supported for streaming
    #[error("Unsupported source format")]
    UnsupportedSource,

    /// Failed to add torrent to engine
    #[error("Failed to add torrent: {reason}")]
    TorrentAddFailed {
        /// Specific reason for torrent addition failure
        reason: String,
    },

    /// Failed to start streaming server
    #[error("Server failed to start: {reason}")]
    ServerStartFailed {
        /// Specific reason for server startup failure
        reason: String,
    },
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
    /// Info hash of the torrent being streamed
    pub info_hash: InfoHash,
    /// Unique identifier for this streaming session
    pub session_id: u64,
    /// Container format for the output stream
    pub format: ContainerFormat,
}

/// Streaming data with metadata
#[derive(Debug, Clone)]
pub struct StreamData {
    /// The actual streaming data bytes
    pub data: Vec<u8>,
    /// MIME type for HTTP Content-Type header
    pub content_type: String,
    /// Total size of the complete stream (if known)
    pub total_size: Option<u64>,
    /// Start byte position of this data chunk
    pub range_start: u64,
    /// End byte position of this data chunk
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
    /// Current readiness state of the stream
    pub readiness: StreamReadiness,
    /// Processing progress from 0.0 to 1.0 (if applicable)
    pub progress: Option<f32>,
    /// Estimated time remaining for processing
    pub estimated_time_remaining: Option<Duration>,
    /// Error message if processing failed
    pub error_message: Option<String>,
    /// Timestamp of last activity (not serialized)
    #[serde(skip)]
    pub last_activity: Instant,
}

/// Complete remuxing session data
#[derive(Debug)]
pub struct RemuxSession {
    /// Info hash of the torrent being remuxed
    pub info_hash: InfoHash,
    /// Current state of the remuxing process
    pub state: super::state::RemuxState,
    /// Running FFmpeg process handle (if active)
    pub ffmpeg_handle: Option<std::process::Child>,
    /// Path to the output remuxed file
    pub output_path: Option<PathBuf>,
    /// Progress tracking information
    pub progress: super::state::RemuxProgress,
    /// When this session was created
    pub created_at: Instant,
    /// Timestamp of last activity on this session
    pub last_activity: Instant,
    /// Unique identifier for this session
    pub session_id: u64,
}

impl RemuxSession {
    /// Creates a new remux session in the waiting state.
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
