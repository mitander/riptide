//! Stream coordinator for managing torrent-based streaming sessions
//!
//! Coordinates between HTTP requests and BitTorrent downloading to provide
//! seamless media streaming with intelligent buffering and piece prioritization.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde::Serialize;
use tokio::sync::RwLock;

use super::range_handler::FileInfo;
use super::{ContentInfo, RangeHandler};
use crate::torrent::{
    EnhancedPeerManager, InfoHash, PieceIndex, Priority, TorrentEngine, TorrentError,
};

/// Coordinates streaming sessions between HTTP requests and BitTorrent backend.
///
/// Manages active streaming sessions, prioritizes piece downloads for optimal
/// streaming performance, and maintains streaming buffers for smooth playback.
pub struct StreamCoordinator {
    torrent_engine: Arc<RwLock<TorrentEngine>>,
    peer_manager: Arc<RwLock<EnhancedPeerManager>>,
    active_sessions: Arc<RwLock<HashMap<InfoHash, StreamingSession>>>,
    registered_torrents: Arc<RwLock<HashMap<InfoHash, TorrentMetadata>>>,
}

/// Active streaming session for a torrent.
///
/// Tracks streaming state, buffer requirements, and performance metrics
/// for optimal piece prioritization and bandwidth allocation.
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
    pub priority: super::range_handler::PiecePriority,
    pub requested_at: Instant,
    pub estimated_completion: Option<Instant>,
}

/// Performance metrics for streaming optimization.
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

mod hex_serde {
    use serde::{Serialize, Serializer};

    use super::InfoHash;

    pub fn serialize<S>(info_hash: &InfoHash, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        hex::encode(info_hash.as_bytes()).serialize(serializer)
    }
}

mod serde_instant {
    use std::time::{Instant, SystemTime, UNIX_EPOCH};

    use serde::{Serialize, Serializer};

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

impl StreamCoordinator {
    /// Creates new stream coordinator with engine and peer manager.
    pub fn new(
        torrent_engine: Arc<RwLock<TorrentEngine>>,
        peer_manager: Arc<RwLock<EnhancedPeerManager>>,
    ) -> Self {
        Self {
            torrent_engine,
            peer_manager,
            active_sessions: Arc::new(RwLock::new(HashMap::new())),
            registered_torrents: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Register torrent for streaming.
    ///
    /// # Errors
    /// - `StreamingError::TorrentEngine` - Failed to get torrent metadata
    pub async fn register_torrent(
        &mut self,
        info_hash: InfoHash,
        source: String,
    ) -> Result<(), StreamingError> {
        let _engine = self.torrent_engine.read().await;

        // Get torrent metadata from engine
        let metadata = self.create_torrent_metadata(info_hash, source).await?;

        let mut torrents = self.registered_torrents.write().await;
        torrents.insert(info_hash, metadata);

        Ok(())
    }

    /// Get content information for a torrent.
    ///
    /// # Errors
    /// - `StreamingError::TorrentNotFound` - Torrent not registered
    pub async fn get_content_info(
        &self,
        info_hash: [u8; 20],
    ) -> Result<ContentInfo, StreamingError> {
        let info_hash = InfoHash::new(info_hash);
        let torrents = self.registered_torrents.read().await;

        let metadata = torrents
            .get(&info_hash)
            .ok_or(StreamingError::TorrentNotFound { info_hash })?;

        Ok(ContentInfo::from_torrent_metadata(
            metadata.name.clone(),
            metadata.total_size,
            metadata.piece_size,
            metadata.total_pieces,
            metadata.files.clone(),
        ))
    }

    /// Get file information for a specific file in a torrent.
    ///
    /// # Errors
    /// - `StreamingError::TorrentNotFound` - Torrent not registered
    /// - `StreamingError::FileNotFound` - File index out of bounds
    pub async fn get_file_info(
        &self,
        info_hash: [u8; 20],
        file_index: usize,
    ) -> Result<FileInfo, StreamingError> {
        let info_hash = InfoHash::new(info_hash);
        let torrents = self.registered_torrents.read().await;

        let metadata = torrents
            .get(&info_hash)
            .ok_or(StreamingError::TorrentNotFound { info_hash })?;

        metadata
            .files
            .get(file_index)
            .cloned()
            .ok_or(StreamingError::FileNotFound {
                info_hash,
                file_index,
            })
    }

    /// Read a byte range from torrent content.
    ///
    /// # Errors
    /// - `StreamingError::TorrentNotFound` - Torrent not registered
    /// - `StreamingError::DataReadError` - Failed to read data
    pub async fn read_range(
        &self,
        info_hash: [u8; 20],
        start: u64,
        length: u64,
    ) -> Result<Vec<u8>, StreamingError> {
        let info_hash = InfoHash::new(info_hash);

        // Get or create streaming session
        let _session = self.get_or_create_session(info_hash, start).await?;

        // Calculate required pieces for this range
        let range_handler = self.create_range_handler(info_hash).await?;
        let required_pieces = range_handler.calculate_required_pieces(start, length);

        // Prioritize pieces for streaming
        self.prioritize_pieces_for_streaming(info_hash, &required_pieces, start)
            .await?;

        // Read data from available pieces
        self.read_pieces_data(info_hash, start, length).await
    }

    /// Read a byte range from a specific file within a torrent.
    ///
    /// # Errors
    /// - `StreamingError::FileNotFound` - File not found
    /// - `StreamingError::DataReadError` - Failed to read data
    pub async fn read_file_range(
        &self,
        info_hash: [u8; 20],
        file_index: usize,
        start: u64,
        length: u64,
    ) -> Result<Vec<u8>, StreamingError> {
        let info_hash = InfoHash::new(info_hash);
        let file_info = self
            .get_file_info(*info_hash.as_bytes(), file_index)
            .await?;

        // Calculate absolute offset within torrent
        let absolute_start = file_info.offset + start;
        let bounded_length = length.min(file_info.size - start);

        self.read_range(*info_hash.as_bytes(), absolute_start, bounded_length)
            .await
    }

    /// List all registered torrents.
    pub async fn list_torrents(&self) -> Result<Vec<TorrentMetadata>, StreamingError> {
        let torrents = self.registered_torrents.read().await;
        Ok(torrents.values().cloned().collect())
    }

    /// Get streaming statistics.
    pub async fn get_stats(&self) -> StreamingStats {
        let sessions = self.active_sessions.read().await;
        let torrents = self.registered_torrents.read().await;

        let active_sessions = sessions.len();
        let total_bytes_served = sessions.values().map(|s| s.bytes_served).sum();
        let average_buffer_health = if sessions.is_empty() {
            0.0
        } else {
            sessions
                .values()
                .map(|s| s.buffer_state.buffer_health)
                .sum::<f64>()
                / sessions.len() as f64
        };

        StreamingStats {
            active_sessions,
            total_bytes_served,
            average_buffer_health,
            total_torrents: torrents.len(),
            successful_streams: sessions
                .values()
                .map(|s| s.performance_metrics.total_requests as u64)
                .sum(),
        }
    }

    /// Get or create streaming session for torrent.
    async fn get_or_create_session(
        &self,
        info_hash: InfoHash,
        position: u64,
    ) -> Result<StreamingSession, StreamingError> {
        let mut sessions = self.active_sessions.write().await;

        if let Some(session) = sessions.get_mut(&info_hash) {
            session.current_position = position;
            session.last_activity = Instant::now();
            Ok(session.clone())
        } else {
            let torrents = self.registered_torrents.read().await;
            let metadata = torrents
                .get(&info_hash)
                .ok_or(StreamingError::TorrentNotFound { info_hash })?;

            let session = StreamingSession::new(info_hash, metadata.total_size, position);
            sessions.insert(info_hash, session.clone());
            Ok(session)
        }
    }

    /// Create range handler for torrent.
    async fn create_range_handler(
        &self,
        info_hash: InfoHash,
    ) -> Result<RangeHandler, StreamingError> {
        let torrents = self.registered_torrents.read().await;
        let metadata = torrents
            .get(&info_hash)
            .ok_or(StreamingError::TorrentNotFound { info_hash })?;

        Ok(RangeHandler::new(
            metadata.piece_size,
            metadata.total_pieces,
            metadata.total_size,
        ))
    }

    /// Prioritize pieces for optimal streaming performance.
    async fn prioritize_pieces_for_streaming(
        &self,
        info_hash: InfoHash,
        required_pieces: &[super::range_handler::PieceRange],
        _current_position: u64,
    ) -> Result<(), StreamingError> {
        let peer_manager = self.peer_manager.read().await;

        // Request pieces with streaming priorities
        for piece_range in required_pieces {
            let priority = match piece_range.priority {
                super::range_handler::PiecePriority::Critical => Priority::Critical,
                super::range_handler::PiecePriority::High => Priority::High,
                super::range_handler::PiecePriority::Normal => Priority::Normal,
                super::range_handler::PiecePriority::Low => Priority::Background,
            };

            let _result = peer_manager
                .request_piece_prioritized(
                    info_hash,
                    piece_range.piece_index.into(),
                    32768, // Default piece size
                    priority,
                    Some(Instant::now() + Duration::from_secs(10)),
                )
                .await;
        }

        Ok(())
    }

    /// Read actual piece data from torrent engine.
    async fn read_pieces_data(
        &self,
        _info_hash: InfoHash,
        _start: u64,
        length: u64,
    ) -> Result<Vec<u8>, StreamingError> {
        // For now, return mock data - in production this would:
        // 1. Check which pieces are available
        // 2. Read piece data from storage
        // 3. Extract the requested byte range
        // 4. Handle partial pieces at range boundaries

        Ok(vec![0u8; length as usize])
    }

    /// Create torrent metadata from engine.
    async fn create_torrent_metadata(
        &self,
        info_hash: InfoHash,
        source: String,
    ) -> Result<TorrentMetadata, StreamingError> {
        // For now, create minimal metadata - in production this would:
        // 1. Query torrent engine for actual metadata
        // 2. Parse file list and calculate offsets
        // 3. Determine piece size and count

        Ok(TorrentMetadata {
            info_hash,
            name: "Sample Torrent".to_string(),
            total_size: 1_000_000_000, // 1GB
            piece_size: 32768,         // 32KB
            total_pieces: 30518,       // ~1GB / 32KB
            files: vec![FileInfo::from_torrent_file(
                "sample_movie.mp4".to_string(),
                1_000_000_000,
                0,
            )],
            added_at: Instant::now(),
            source,
        })
    }
}

impl StreamingSession {
    /// Create new streaming session.
    fn new(info_hash: InfoHash, total_size: u64, position: u64) -> Self {
        let now = Instant::now();

        Self {
            info_hash,
            current_position: position,
            total_size,
            buffer_state: StreamingBufferState::new(),
            active_ranges: Vec::new(),
            session_start: now,
            last_activity: now,
            bytes_served: 0,
            performance_metrics: StreamingPerformanceMetrics::new(),
        }
    }
}

impl StreamingBufferState {
    fn new() -> Self {
        Self {
            current_piece: 0,
            buffered_pieces: Vec::new(),
            critical_pieces: Vec::new(),
            prefetch_pieces: Vec::new(),
            buffer_health: 0.0,
        }
    }
}

impl StreamingPerformanceMetrics {
    fn new() -> Self {
        Self {
            average_response_time: Duration::from_millis(0),
            throughput_mbps: 0.0,
            buffer_underruns: 0,
            seek_count: 0,
            total_requests: 0,
        }
    }
}

// Convert PieceIndex from u32 for compatibility
impl From<u32> for PieceIndex {
    fn from(index: u32) -> Self {
        PieceIndex::new(index)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::RiptideConfig;
    use crate::torrent::test_data::create_test_info_hash;

    #[tokio::test]
    async fn test_stream_coordinator_creation() {
        let config = RiptideConfig::default();
        let torrent_engine = Arc::new(RwLock::new(TorrentEngine::new(config.clone())));
        let peer_manager = Arc::new(RwLock::new(crate::torrent::EnhancedPeerManager::new(
            config,
        )));

        let coordinator = StreamCoordinator::new(torrent_engine, peer_manager);
        let stats = coordinator.get_stats().await;

        assert_eq!(stats.active_sessions, 0);
        assert_eq!(stats.total_torrents, 0);
    }

    #[tokio::test]
    async fn test_torrent_registration() {
        let config = RiptideConfig::default();
        let torrent_engine = Arc::new(RwLock::new(TorrentEngine::new(config.clone())));
        let peer_manager = Arc::new(RwLock::new(crate::torrent::EnhancedPeerManager::new(
            config,
        )));

        let mut coordinator = StreamCoordinator::new(torrent_engine, peer_manager);
        let info_hash = create_test_info_hash();

        let result = coordinator
            .register_torrent(info_hash, "magnet:?xt=test".to_string())
            .await;

        assert!(result.is_ok());

        let content_info = coordinator
            .get_content_info(*info_hash.as_bytes())
            .await
            .unwrap();
        assert_eq!(content_info.name, "Sample Torrent");
        assert_eq!(content_info.total_size, 1_000_000_000);
    }

    #[tokio::test]
    async fn test_file_info_retrieval() {
        let config = RiptideConfig::default();
        let torrent_engine = Arc::new(RwLock::new(TorrentEngine::new(config.clone())));
        let peer_manager = Arc::new(RwLock::new(crate::torrent::EnhancedPeerManager::new(
            config,
        )));

        let mut coordinator = StreamCoordinator::new(torrent_engine, peer_manager);
        let info_hash = create_test_info_hash();

        coordinator
            .register_torrent(info_hash, "magnet:?xt=test".to_string())
            .await
            .unwrap();

        let file_info = coordinator
            .get_file_info(*info_hash.as_bytes(), 0)
            .await
            .unwrap();

        assert_eq!(file_info.name, "sample_movie.mp4");
        assert!(file_info.is_streamable());
    }

    #[tokio::test]
    async fn test_range_reading() {
        let config = RiptideConfig::default();
        let torrent_engine = Arc::new(RwLock::new(TorrentEngine::new(config.clone())));
        let peer_manager = Arc::new(RwLock::new(crate::torrent::EnhancedPeerManager::new(
            config,
        )));

        let mut coordinator = StreamCoordinator::new(torrent_engine, peer_manager);
        let info_hash = create_test_info_hash();

        coordinator
            .register_torrent(info_hash, "magnet:?xt=test".to_string())
            .await
            .unwrap();

        let data = coordinator
            .read_range(*info_hash.as_bytes(), 0, 1024)
            .await
            .unwrap();

        assert_eq!(data.len(), 1024);
    }

    #[tokio::test]
    async fn test_streaming_session_creation() {
        let info_hash = create_test_info_hash();
        let session = StreamingSession::new(info_hash, 1_000_000_000, 0);

        assert_eq!(session.info_hash, info_hash);
        assert_eq!(session.total_size, 1_000_000_000);
        assert_eq!(session.current_position, 0);
        assert_eq!(session.buffer_state.buffer_health, 0.0);
    }
}
