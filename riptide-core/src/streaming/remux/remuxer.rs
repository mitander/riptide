//! Session management for remuxing operations

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

use tokio::sync::{RwLock, Semaphore};
use tracing::{debug, error, info, warn};

use super::state::{RemuxError, RemuxState};
use super::types::{RemuxConfig, RemuxSession, StreamHandle, StreamReadiness, StreamingStatus};
use crate::storage::DataSource;
use crate::streaming::chunk_server::{ChunkServer, RemuxedChunk};
use crate::streaming::realtime_remuxer::{RealtimeRemuxer, RemuxConfig as RealTimeRemuxConfig};
use crate::streaming::{Ffmpeg, StrategyError, StreamingResult};
use crate::torrent::InfoHash;

/// Manages remuxing sessions with state machine and concurrency control
pub struct Remuxer {
    sessions: Arc<RwLock<HashMap<InfoHash, RemuxSession>>>,
    semaphore: Arc<Semaphore>,
    config: RemuxConfig,
    session_counter: AtomicU64,
    data_source: Arc<dyn DataSource>,
    ffmpeg: Arc<dyn Ffmpeg>,
    torrent_engine: Option<crate::engine::TorrentEngineHandle>,
}

impl Remuxer {
    /// Create a new remux session manager
    pub fn new(
        config: RemuxConfig,
        data_source: Arc<dyn DataSource>,
        ffmpeg: Arc<dyn Ffmpeg>,
    ) -> Self {
        Self::new_with_engine(config, data_source, ffmpeg, None)
    }

    /// Create a new remux session manager with torrent engine handle
    pub fn new_with_engine(
        config: RemuxConfig,
        data_source: Arc<dyn DataSource>,
        ffmpeg: Arc<dyn Ffmpeg>,
        torrent_engine: Option<crate::engine::TorrentEngineHandle>,
    ) -> Self {
        let semaphore = Arc::new(Semaphore::new(config.max_concurrent_sessions));

        // Ensure cache directory exists
        if let Err(e) = std::fs::create_dir_all(&config.cache_dir) {
            warn!("Failed to create remux cache directory: {}", e);
        }

        Self {
            sessions: Arc::new(RwLock::new(HashMap::new())),
            semaphore,
            config,
            session_counter: AtomicU64::new(1),
            data_source,
            ffmpeg,
            torrent_engine,
        }
    }

    /// Find or create a session handle for the given info hash
    ///
    /// # Errors
    ///
    /// - `StreamingError::SessionCreation` - If session creation fails or transcoding cannot be initialized
    pub async fn find_or_create_session(
        &self,
        info_hash: InfoHash,
    ) -> StreamingResult<StreamHandle> {
        let mut sessions = self.sessions.write().await;

        if let Some(session) = sessions.get_mut(&info_hash) {
            session.touch();
            return Ok(StreamHandle {
                info_hash,
                session_id: session.session_id,
                format: crate::streaming::ContainerFormat::Mp4, // Output is always MP4
            });
        }

        // Create new session
        let session_id = self.session_counter.fetch_add(1, Ordering::SeqCst);
        let session = RemuxSession::new(info_hash, session_id);

        sessions.insert(info_hash, session);

        info!("Created new remux session {} for {}", session_id, info_hash);

        Ok(StreamHandle {
            info_hash,
            session_id,
            format: crate::streaming::ContainerFormat::Mp4,
        })
    }

    /// Check the readiness of a stream for serving
    ///
    /// # Errors
    ///
    /// - `StreamingError::SessionNotFound` - If session is not found or readiness status cannot be determined
    pub async fn check_readiness(&self, info_hash: InfoHash) -> StreamingResult<StreamReadiness> {
        let sessions = self.sessions.read().await;
        let session = sessions
            .get(&info_hash)
            .ok_or_else(|| StrategyError::RemuxingFailed {
                reason: "Session not found".to_string(),
            })?;

        debug!(
            "Checking readiness for {}, current state: {:?}",
            info_hash, session.state
        );

        match &session.state {
            RemuxState::WaitingForHeadAndTail => {
                drop(sessions); // Release read lock before async operations

                debug!("Checking if we have required data for {}", info_hash);
                if self.has_required_data(info_hash).await? {
                    debug!(
                        "Required data available, starting remuxing for {}",
                        info_hash
                    );
                    // Transition to remuxing state
                    self.start_remuxing(info_hash).await?;
                    Ok(StreamReadiness::Processing)
                } else {
                    debug!(
                        "Required data not available for {}, staying in waiting state",
                        info_hash
                    );
                    Ok(StreamReadiness::WaitingForData)
                }
            }
            RemuxState::Remuxing { .. } => {
                // Check if chunk server is ready for streaming
                if self.is_chunk_server_ready(info_hash).await {
                    Ok(StreamReadiness::Ready)
                } else {
                    Ok(StreamReadiness::Processing)
                }
            }
            RemuxState::Completed { .. } => Ok(StreamReadiness::Ready),
            RemuxState::Failed { can_retry, .. } => {
                if *can_retry {
                    Ok(StreamReadiness::CanRetry)
                } else {
                    Ok(StreamReadiness::Failed)
                }
            }
        }
    }

    /// Get the current status of a streaming session
    ///
    /// # Errors
    ///
    /// - `StreamingError::SessionNotFound` - If session is not found or status cannot be retrieved
    pub async fn status(&self, info_hash: InfoHash) -> StreamingResult<StreamingStatus> {
        debug!("Getting status for remux session: {}", info_hash);

        let sessions = self.sessions.read().await;
        let session = sessions
            .get(&info_hash)
            .ok_or_else(|| StrategyError::RemuxingFailed {
                reason: "Session not found".to_string(),
            })?;

        debug!(
            "Session found for {}, current state: {:?}",
            info_hash, session.state
        );

        let readiness = match &session.state {
            RemuxState::WaitingForHeadAndTail => StreamReadiness::WaitingForData,
            RemuxState::Remuxing { .. } => StreamReadiness::Processing,
            RemuxState::Completed { .. } => StreamReadiness::Ready,
            RemuxState::Failed { can_retry, .. } => {
                if *can_retry {
                    StreamReadiness::CanRetry
                } else {
                    StreamReadiness::Failed
                }
            }
        };

        debug!(
            "Returning status for {}: readiness={:?}, progress={}",
            info_hash, readiness, session.progress.progress
        );

        Ok(StreamingStatus {
            readiness,
            progress: Some(session.progress.progress),
            estimated_time_remaining: None, // TODO: Calculate based on progress
            error_message: match &session.state {
                RemuxState::Failed { error, .. } => Some(error.to_string()),
                _ => None,
            },
            last_activity: session.last_activity,
        })
    }

    /// Get the output file path for a completed session
    ///
    /// # Errors
    ///
    /// - `StreamingError::SessionNotFound` - If session is not found or output path is not available
    pub async fn output_path(&self, info_hash: InfoHash) -> StreamingResult<PathBuf> {
        let sessions = self.sessions.read().await;
        let session = sessions
            .get(&info_hash)
            .ok_or_else(|| StrategyError::RemuxingFailed {
                reason: "Session not found".to_string(),
            })?;

        match &session.state {
            RemuxState::Completed { output_path } => Ok(output_path.clone()),
            RemuxState::Remuxing { .. } => {
                // For progressive streaming, allow access to partial files
                if let Some(output_path) = &session.output_path {
                    Ok(output_path.clone())
                } else {
                    Err(StrategyError::StreamingNotReady {
                        reason: "Remuxing output path not available".to_string(),
                    })
                }
            }
            _ => Err(StrategyError::StreamingNotReady {
                reason: "Remuxing not completed".to_string(),
            }),
        }
    }

    /// Start the remuxing process for a session
    async fn start_remuxing(&self, info_hash: InfoHash) -> StreamingResult<()> {
        info!("REMUX: Starting remuxing process for {}", info_hash);

        // Acquire semaphore permit for concurrency control
        let _permit =
            self.semaphore
                .acquire()
                .await
                .map_err(|_| StrategyError::RemuxingFailed {
                    reason: "Failed to acquire remuxing permit".to_string(),
                })?;

        debug!("Acquired remuxing permit for {}", info_hash);

        let mut sessions = self.sessions.write().await;
        let session =
            sessions
                .get_mut(&info_hash)
                .ok_or_else(|| StrategyError::RemuxingFailed {
                    reason: "Session not found".to_string(),
                })?;

        // Ensure we're in the correct state
        if !session.state.can_start_remuxing() {
            return Err(StrategyError::RemuxingFailed {
                reason: format!("Cannot start remuxing in state: {:?}", session.state),
            });
        }

        // Generate output path
        let output_path = self.config.cache_dir.join(format!("{info_hash}.mp4"));

        // Always use real-time remuxing for progressive streaming
        // This enables streaming to start immediately when head data is available
        let file_size = self.data_source.file_size(info_hash).await.map_err(|e| {
            StrategyError::RemuxingFailed {
                reason: format!("Failed to get file size: {e}"),
            }
        })?;

        let is_complete = self
            .data_source
            .check_range_availability(info_hash, 0..file_size)
            .await
            .map(|availability| availability.available)
            .unwrap_or(false);

        // Always use real-time remuxing for progressive streaming
        info!(
            "REMUX: Starting real-time remuxing for {} (completion: {}%)",
            info_hash,
            if is_complete { 100 } else { 0 }
        );

        // Use real-time remuxing for all downloads
        let is_avi_file = self.is_avi_file(info_hash).await;
        let mut remux_config = if is_avi_file {
            RealTimeRemuxConfig::for_avi()
        } else {
            RealTimeRemuxConfig::default()
        };
        remux_config.use_fragmented_mp4 = true;

        let chunk_server = Arc::new(ChunkServer::new(
            info_hash,
            100,              // max chunks
            10 * 1024 * 1024, // 10MB buffer
        ));

        let remuxer =
            RealtimeRemuxer::new(remux_config).map_err(|e| StrategyError::RemuxingFailed {
                reason: format!("Failed to create real-time remuxer: {e}"),
            })?;

        let remux_handle = remuxer
            .start()
            .await
            .map_err(|e| StrategyError::RemuxingFailed {
                reason: format!("Failed to start real-time remuxer: {e}"),
            })?;

        session.state = RemuxState::Remuxing {
            started_at: Instant::now(),
        };
        session.chunk_server = Some(chunk_server.clone());
        session.output_path = Some(output_path.clone());
        session.touch();

        // Spawn real-time pipeline task
        let remuxer_clone = self.clone();
        let info_hash_copy = info_hash;
        tokio::spawn(async move {
            if let Err(e) = remuxer_clone
                .coordinate_realtime_pipeline(info_hash_copy, remux_handle, chunk_server)
                .await
            {
                error!("Real-time pipeline failed for {}: {}", info_hash_copy, e);
                remuxer_clone
                    .mark_failed(
                        info_hash_copy,
                        RemuxError::FfmpegFailed {
                            reason: e.to_string(),
                        },
                    )
                    .await;
            }
        });

        Ok(())
    }

    /// Coordinate real-time streaming pipeline with chunk serving
    async fn coordinate_realtime_pipeline(
        &self,
        info_hash: InfoHash,
        mut remux_handle: crate::streaming::realtime_remuxer::RemuxHandle,
        chunk_server: Arc<ChunkServer>,
    ) -> StreamingResult<()> {
        info!(
            "REMUX PIPELINE: Starting real-time pipeline for {}",
            info_hash
        );

        // Get file size for progress tracking
        let file_size = self.data_source.file_size(info_hash).await.map_err(|e| {
            StrategyError::RemuxingFailed {
                reason: format!("Failed to get file size: {e}"),
            }
        })?;

        // Spawn task to feed input data to remuxer
        let data_source = self.data_source.clone();
        let input_sender = remux_handle.input_sender.clone();
        let info_hash_copy = info_hash;
        tokio::spawn(async move {
            let mut offset = 0u64;
            const CHUNK_SIZE: u64 = 256 * 1024; // 256KB chunks

            while offset < file_size {
                let end = (offset + CHUNK_SIZE).min(file_size);

                match data_source.read_range(info_hash_copy, offset..end).await {
                    Ok(data) => {
                        if data.is_empty() {
                            // Wait for more data to become available
                            tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
                            continue;
                        }

                        if input_sender.send(data).await.is_err() {
                            info!("REMUX INPUT: Input receiver closed for {}", info_hash_copy);
                            break;
                        }

                        offset += end - offset;
                        if offset % (1024 * 1024) == 0 {
                            info!(
                                "REMUX INPUT: Fed {}MB to remuxer for {}",
                                offset / 1024 / 1024,
                                info_hash_copy
                            );
                        }
                    }
                    Err(_) => {
                        // Data not available yet, wait and retry
                        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
                    }
                }
            }

            info!(
                "REMUX INPUT: Finished feeding input data for {}",
                info_hash_copy
            );

            // Explicitly close the input sender to signal EOF to FFmpeg
            drop(input_sender);
            info!("REMUX INPUT: Closed input stream for {}", info_hash_copy);
        });

        // Spawn task to receive output chunks and feed to chunk server
        let chunk_server_clone = chunk_server.clone();
        let info_hash_copy = info_hash;
        tokio::spawn(async move {
            let mut total_chunks = 0u64;

            while let Some(chunk) = remux_handle.recv_chunk().await {
                let remuxed_chunk = RemuxedChunk {
                    offset: chunk.offset,
                    data: chunk.data.into(),
                    is_header: chunk.is_header,
                    timestamp: chunk.timestamp,
                };

                if let Err(e) = chunk_server_clone.add_chunk(remuxed_chunk).await {
                    error!(
                        "REMUX OUTPUT: Failed to add chunk to server for {}: {}",
                        info_hash_copy, e
                    );
                    break;
                }

                total_chunks += 1;
                if total_chunks == 1 {
                    info!("REMUX OUTPUT: First chunk received for {}", info_hash_copy);
                }
                if total_chunks % 10 == 0 {
                    info!(
                        "REMUX OUTPUT: Processed {} chunks for {}",
                        total_chunks, info_hash_copy
                    );
                }
            }

            // Mark chunk server as completed
            chunk_server_clone.mark_completed(file_size).await;
            info!(
                "REMUX OUTPUT: Real-time remuxing completed for {} ({} chunks)",
                info_hash_copy, total_chunks
            );
        });

        info!(
            "REMUX PIPELINE: Started real-time streaming pipeline for {}",
            info_hash
        );

        Ok(())
    }

    /// Mark a session as failed
    async fn mark_failed(&self, info_hash: InfoHash, error: RemuxError) {
        let mut sessions = self.sessions.write().await;
        if let Some(session) = sessions.get_mut(&info_hash) {
            let can_retry = error.is_retryable();
            session.state = RemuxState::Failed {
                error: error.clone(),
                can_retry,
            };
            session.ffmpeg_handle = None;
            session.touch();

            warn!(
                "Remuxing failed for {}: {} (retryable: {})",
                info_hash, error, can_retry
            );
        }
    }

    /// Check if we have sufficient data to start remuxing
    async fn has_required_data(&self, info_hash: InfoHash) -> StreamingResult<bool> {
        let file_size = self.data_source.file_size(info_hash).await.map_err(|e| {
            StrategyError::RemuxingFailed {
                reason: format!("Failed to get file size: {e}"),
            }
        })?;

        // Check if this is an AVI file to determine data requirements
        let is_avi_file = self.is_avi_file(info_hash).await;

        // CRITICAL FIX: For progressive streaming, we need very little data to start
        // The key insight is that real-time remuxing can start with just head data
        let required_head_size = if is_avi_file {
            // For AVI files, we need enough head data to read the metadata
            // But we can start remuxing with much less than before
            (1024 * 1024).min(file_size) // Just 1MB for AVI metadata
        } else {
            // For other formats, even less data is needed
            (512 * 1024).min(file_size) // 512KB should be enough for most formats
        };

        debug!(
            "Checking required data for {}: file_size={}, required_head_size={}, is_avi={}",
            info_hash, file_size, required_head_size, is_avi_file
        );

        // Check if we have head data
        let head_available = self
            .data_source
            .check_range_availability(info_hash, 0..required_head_size)
            .await
            .map(|availability| {
                debug!(
                    "Head data availability for {}: range=0..{}, available={}, missing_pieces={}",
                    info_hash,
                    required_head_size,
                    availability.available,
                    availability.missing_pieces.len()
                );
                availability.available
            })
            .unwrap_or(false);

        // For very small files (< 5MB), require more data for stability
        if file_size < 5 * 1024 * 1024 {
            debug!(
                "Small file detected ({}MB), requiring more data for stability",
                file_size / 1024 / 1024
            );
            let target_size = (file_size * 3 / 4).max(required_head_size); // Require 75% of small files
            let sufficient_data_available = self
                .data_source
                .check_range_availability(info_hash, 0..target_size)
                .await
                .map(|availability| {
                    debug!(
                        "Small file availability for {}: available={}, missing_pieces={}",
                        info_hash,
                        availability.available,
                        availability.missing_pieces.len()
                    );
                    availability.available
                })
                .unwrap_or(false);
            return Ok(sufficient_data_available);
        }

        // CRITICAL: For progressive streaming, start remuxing immediately with head data
        // This is the key fix - we don't need tail data for real-time remuxing
        if head_available {
            debug!(
                "Head data available ({}MB), starting progressive remux for {}",
                required_head_size / 1024 / 1024,
                info_hash
            );
            return Ok(true);
        }

        debug!(
            "Insufficient data for progressive remux: head_available={}, required_head_size={}",
            head_available, required_head_size
        );

        Ok(false)
    }

    /// Check if a file is an AVI file by reading its header
    async fn is_avi_file(&self, info_hash: InfoHash) -> bool {
        const HEADER_CHECK_SIZE: u64 = 32;
        if let Ok(header_data) = self
            .data_source
            .read_range(info_hash, 0..HEADER_CHECK_SIZE)
            .await
        {
            header_data.len() >= 12
                && header_data.starts_with(b"RIFF")
                && &header_data[8..12] == b"AVI "
        } else {
            false
        }
    }

    /// Check if chunk server is ready for streaming
    async fn is_chunk_server_ready(&self, info_hash: InfoHash) -> bool {
        let sessions = self.sessions.read().await;
        let session = match sessions.get(&info_hash) {
            Some(session) => session,
            None => {
                info!("CHUNK SERVER: No session found for {}", info_hash);
                return false;
            }
        };

        // Check if chunk server is available and ready
        if let Some(chunk_server) = &session.chunk_server {
            let is_ready = chunk_server.is_ready_for_playback().await;
            let stats = chunk_server.stats().await;

            info!(
                "CHUNK SERVER: Readiness check for {}: ready={}, chunks={}, bytes={}",
                info_hash, is_ready, stats.total_chunks, stats.total_bytes
            );

            if is_ready {
                info!(
                    "CHUNK SERVER: Ready for streaming: {} ({} chunks, {} bytes buffered)",
                    info_hash, stats.total_chunks, stats.total_bytes
                );
            }

            is_ready
        } else {
            info!("CHUNK SERVER: No chunk server found for {}", info_hash);
            false
        }
    }

    /// Get access to the underlying data source
    pub fn data_source(&self) -> &Arc<dyn DataSource> {
        &self.data_source
    }

    /// Clean up stale sessions
    pub async fn cleanup_stale_sessions(&self) {
        let mut sessions = self.sessions.write().await;
        let mut to_remove = Vec::new();

        for (info_hash, session) in sessions.iter() {
            if session.is_stale(self.config.cleanup_after) {
                to_remove.push(*info_hash);
            }
        }

        for info_hash in to_remove {
            if let Some(session) = sessions.remove(&info_hash) {
                // Clean up output file if it exists
                if let Some(output_path) = session.output_path
                    && let Err(e) = std::fs::remove_file(&output_path)
                {
                    warn!(
                        "Failed to remove remux output file {}: {}",
                        output_path.display(),
                        e
                    );
                }

                info!("Cleaned up stale remux session for {}", info_hash);
            }
        }
    }
}

impl Clone for Remuxer {
    fn clone(&self) -> Self {
        Self {
            sessions: Arc::clone(&self.sessions),
            semaphore: Arc::clone(&self.semaphore),
            config: self.config.clone(),
            session_counter: AtomicU64::new(self.session_counter.load(Ordering::SeqCst)),
            data_source: Arc::clone(&self.data_source),
            ffmpeg: Arc::clone(&self.ffmpeg),
            torrent_engine: self.torrent_engine.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap as StdHashMap;

    use async_trait::async_trait;

    use super::*;
    use crate::storage::{DataError, DataResult, RangeAvailability};

    // Type aliases for complex types
    type FileSizeMap = StdHashMap<InfoHash, u64>;
    type RangeMap = StdHashMap<InfoHash, Vec<std::ops::Range<u64>>>;

    struct MockDataSource {
        files: FileSizeMap,
        available_ranges: RangeMap,
    }

    impl MockDataSource {
        fn new() -> Self {
            Self {
                files: StdHashMap::new(),
                available_ranges: StdHashMap::new(),
            }
        }

        fn add_file(&mut self, info_hash: InfoHash, size: u64) {
            self.files.insert(info_hash, size);
        }

        fn make_range_available(&mut self, info_hash: InfoHash, range: std::ops::Range<u64>) {
            self.available_ranges
                .entry(info_hash)
                .or_default()
                .push(range);
        }
    }

    #[async_trait]
    impl DataSource for MockDataSource {
        async fn read_range(
            &self,
            _info_hash: InfoHash,
            _range: std::ops::Range<u64>,
        ) -> DataResult<Vec<u8>> {
            Ok(vec![0u8; 1024])
        }

        async fn file_size(&self, info_hash: InfoHash) -> DataResult<u64> {
            self.files
                .get(&info_hash)
                .copied()
                .ok_or_else(|| DataError::FileNotFound { info_hash })
        }

        async fn check_range_availability(
            &self,
            info_hash: InfoHash,
            range: std::ops::Range<u64>,
        ) -> DataResult<RangeAvailability> {
            let available = if let Some(available_ranges) = self.available_ranges.get(&info_hash) {
                available_ranges
                    .iter()
                    .any(|r| r.start <= range.start && r.end >= range.end)
            } else {
                false
            };

            Ok(RangeAvailability {
                available,
                missing_pieces: vec![],
                cache_hit: false,
            })
        }

        fn source_type(&self) -> &'static str {
            "mock"
        }

        async fn can_handle(&self, info_hash: InfoHash) -> bool {
            self.files.contains_key(&info_hash)
        }
    }

    #[tokio::test]
    async fn test_session_creation() {
        let config = RemuxConfig::default();
        let mut data_source = MockDataSource::new();
        let info_hash = InfoHash::new([1u8; 20]);
        data_source.add_file(info_hash, 10 * 1024 * 1024);

        let remuxer = Remuxer::new(
            config,
            Arc::new(data_source),
            Arc::new(crate::streaming::SimulationFfmpeg::new()),
        );

        let handle = remuxer.find_or_create_session(info_hash).await.unwrap();
        assert_eq!(handle.info_hash, info_hash);
        assert_eq!(handle.session_id, 1);
    }

    #[tokio::test]
    async fn test_readiness_without_data() {
        let config = RemuxConfig::default();
        let mut data_source = MockDataSource::new();
        let info_hash = InfoHash::new([1u8; 20]);
        data_source.add_file(info_hash, 10 * 1024 * 1024);

        let remuxer = Remuxer::new(
            config,
            Arc::new(data_source),
            Arc::new(crate::streaming::SimulationFfmpeg::new()),
        );
        let _handle = remuxer.find_or_create_session(info_hash).await.unwrap();

        let readiness = remuxer.check_readiness(info_hash).await.unwrap();
        assert_eq!(readiness, StreamReadiness::WaitingForData);
    }

    #[tokio::test]
    async fn test_readiness_with_data() {
        let config = RemuxConfig::default();
        let mut data_source = MockDataSource::new();
        let info_hash = InfoHash::new([1u8; 20]);
        let file_size = 10 * 1024 * 1024u64;
        data_source.add_file(info_hash, file_size);

        // Make head and tail available
        data_source.make_range_available(info_hash, 0..config.min_head_size);
        data_source.make_range_available(info_hash, (file_size - config.min_tail_size)..file_size);

        let remuxer = Remuxer::new(
            config,
            Arc::new(data_source),
            Arc::new(crate::streaming::SimulationFfmpeg::new()),
        );
        let _handle = remuxer.find_or_create_session(info_hash).await.unwrap();

        let readiness = remuxer.check_readiness(info_hash).await.unwrap();
        assert_eq!(readiness, StreamReadiness::Processing);
    }
}
