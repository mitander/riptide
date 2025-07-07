//! StreamCoordinator implementation for managing torrent streaming sessions

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use tokio::sync::RwLock;

use super::types::{
    StreamCoordinator, StreamingError, StreamingSession, StreamingStats, TorrentMetadata,
};
use crate::streaming::range_handler::{FileInfo, PiecePriority, PieceRange};
use crate::streaming::{ContentInfo, RangeHandler};
use crate::torrent::{
    EnhancedPeerManager, InfoHash, PieceRequestParams, Priority, TorrentEngineHandle,
};

impl StreamCoordinator {
    /// Creates new stream coordinator with engine and peer manager.
    pub fn new(
        torrent_engine: TorrentEngineHandle,
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
        // Get torrent metadata from engine
        let metadata = self.create_torrent_metadata(info_hash, source).await?;

        let mut torrents = self.registered_torrents.write().await;
        torrents.insert(info_hash, metadata);

        Ok(())
    }

    /// Returns content information for a torrent.
    ///
    /// # Errors
    /// - `StreamingError::TorrentNotFound` - Torrent not registered
    pub async fn content_info(&self, info_hash: [u8; 20]) -> Result<ContentInfo, StreamingError> {
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

    /// Returns file information for a specific file in a torrent.
    ///
    /// # Errors
    /// - `StreamingError::TorrentNotFound` - Torrent not registered
    /// - `StreamingError::FileNotFound` - File index out of bounds
    pub async fn file_info(
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

        // Check if torrent exists in engine (optional validation)
        if let Err(e) = self.torrent_engine.session_details(info_hash).await {
            tracing::warn!(
                "Torrent {} not found in engine during streaming: {}",
                info_hash,
                e
            );
            // Continue anyway - torrent might be registered but not yet active
        }

        // Get or create streaming session for state management
        let _session = self.get_or_create_session(info_hash, start).await?;

        // Update session with current streaming position
        {
            let mut sessions = self.active_sessions.write().await;
            if let Some(session_data) = sessions.get_mut(&info_hash) {
                session_data.last_activity = std::time::Instant::now();
                session_data.current_position = start;
                session_data.bytes_served += length;
            }
        }

        // Calculate required pieces for this range
        let range_handler = self.create_range_handler(info_hash).await?;
        let required_pieces = range_handler.calculate_required_pieces(start, length);

        // Prioritize pieces for streaming using session context
        self.prioritize_pieces_for_streaming(info_hash, &required_pieces, start)
            .await?;

        // Update session with piece requests
        {
            let mut sessions = self.active_sessions.write().await;
            if let Some(session_data) = sessions.get_mut(&info_hash) {
                session_data.performance_metrics.total_requests += required_pieces.len() as u32;
            }
        }

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
        let file_info = self.file_info(*info_hash.as_bytes(), file_index).await?;

        // Calculate absolute offset within torrent
        let absolute_start = file_info.offset + start;
        let bounded_length = length.min(file_info.size - start);

        self.read_range(*info_hash.as_bytes(), absolute_start, bounded_length)
            .await
    }

    /// List all registered torrents.
    ///
    /// # Errors
    /// Returns error if torrent registry access fails.
    pub async fn list_torrents(&self) -> Result<Vec<TorrentMetadata>, StreamingError> {
        let torrents = self.registered_torrents.read().await;
        Ok(torrents.values().cloned().collect())
    }

    /// Returns streaming statistics.
    pub async fn statistics(&self) -> StreamingStats {
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

    /// Prioritize pieces for streaming performance.
    async fn prioritize_pieces_for_streaming(
        &self,
        info_hash: InfoHash,
        required_pieces: &[PieceRange],
        _current_position: u64,
    ) -> Result<(), StreamingError> {
        let peer_manager = self.peer_manager.read().await;

        // Request pieces with streaming priorities
        for piece_range in required_pieces {
            let priority = match piece_range.priority {
                PiecePriority::Critical => Priority::Critical,
                PiecePriority::High => Priority::High,
                PiecePriority::Normal => Priority::Normal,
                PiecePriority::Low => Priority::Background,
            };

            let params = PieceRequestParams {
                info_hash,
                piece_index: piece_range.piece_index.into(),
                piece_size: 32768, // Default piece size
                priority,
                deadline: Some(Instant::now() + Duration::from_secs(10)),
            };
            let result = peer_manager.request_piece_prioritized(params).await;

            // Handle piece request result
            if let Err(e) = result {
                tracing::warn!("Failed to request piece {}: {}", piece_range.piece_index, e);
            }
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
        // TODO: Replace mock with real implementation:
        // let session = self._torrent_engine.session_details(info_hash).await??;
        // For now, create minimal metadata

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
