//! Session management for remuxing operations

use std::collections::HashMap;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

use tokio::sync::{RwLock, Semaphore};
use tracing::{debug, error, info, warn};

use super::state::{RemuxError, RemuxState};
use super::types::{RemuxConfig, RemuxSession, StreamHandle, StreamReadiness, StreamingStatus};
use crate::storage::DataSource;
use crate::streaming::migration::ProgressiveStreaming;
use crate::streaming::{Ffmpeg, RemuxingOptions, StrategyError, StreamingResult};
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
                // Check if we have enough partial data to start streaming
                if self.is_partial_file_ready(info_hash).await {
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
        debug!("Starting remuxing process for {}", info_hash);

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

        // Start FFmpeg process
        let mut cmd = Command::new(&self.config.ffmpeg_path);
        cmd.arg("-i")
            .arg("pipe:0") // Read from stdin
            .arg("-c:v")
            .arg("copy") // Copy video codec
            .arg("-c:a")
            .arg("aac") // Convert audio to AAC
            .arg("-movflags")
            .arg("+faststart") // Enable HTTP range requests
            .arg("-f")
            .arg("mp4") // Output format
            .arg(&output_path)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::piped());

        let child = cmd.spawn().map_err(|e| StrategyError::RemuxingFailed {
            reason: format!("Failed to start FFmpeg: {e}"),
        })?;

        // Update session state
        session.state = RemuxState::Remuxing {
            started_at: Instant::now(),
        };
        session.ffmpeg_handle = Some(child);
        session.output_path = Some(output_path.clone());
        session.touch();

        info!(
            "Started remuxing session for {} -> {}",
            info_hash,
            output_path.display()
        );

        // Spawn task to feed data to FFmpeg and monitor progress
        let remuxer = self.clone();
        let info_hash_copy = info_hash;
        tokio::spawn(async move {
            if let Err(e) = remuxer.feed_ffmpeg_data(info_hash_copy).await {
                error!("Failed to feed FFmpeg data for {}: {}", info_hash_copy, e);
                remuxer
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

    /// Feed data to FFmpeg process progressively as it becomes available
    async fn feed_ffmpeg_data(&self, info_hash: InfoHash) -> StreamingResult<()> {
        debug!(
            "feed_ffmpeg_data: Starting progressive streaming for {}",
            info_hash
        );

        // Get file size and output path
        let (file_size, output_path) = {
            let sessions = self.sessions.read().await;
            let session =
                sessions
                    .get(&info_hash)
                    .ok_or_else(|| StrategyError::RemuxingFailed {
                        reason: "Session not found".to_string(),
                    })?;
            let file_size = self.data_source.file_size(info_hash).await.map_err(|e| {
                StrategyError::RemuxingFailed {
                    reason: format!("Failed to get file size: {e}"),
                }
            })?;
            let output_path =
                session
                    .output_path
                    .clone()
                    .ok_or_else(|| StrategyError::RemuxingFailed {
                        reason: "No output path".to_string(),
                    })?;
            (file_size, output_path)
        };

        // Create temporary input file path
        let temp_input = output_path.with_extension("progressive_input");

        // Check if this is an AVI file by reading minimal header
        let header_size = 16u64.min(file_size);
        let is_avi_file =
            if let Ok(header_data) = self.data_source.read_range(info_hash, 0..header_size).await {
                header_data.len() >= 12
                    && header_data.starts_with(b"RIFF")
                    && &header_data[8..12] == b"AVI "
            } else {
                false
            };

        // Configure FFmpeg options for streaming
        let video_codec = if is_avi_file {
            // For AVI files, transcode video to H.264 for better browser compatibility
            // AVI files often have DivX/Xvid codecs that don't work well in MP4
            "libx264".to_string()
        } else {
            "copy".to_string()
        };

        let remux_options = RemuxingOptions {
            video_codec,
            audio_codec: "aac".to_string(), // Convert audio for better compatibility
            faststart: true,
            timeout_seconds: Some(300),
            ignore_index: false,
            allow_partial: true, // Allow partial data for progressive streaming
        };

        debug!(
            "feed_ffmpeg_data: Creating progressive streaming for {} with file_size={}",
            info_hash, file_size
        );

        // Create progressive streaming instance with migration layer
        let progressive_streaming =
            ProgressiveStreaming::new(self.data_source.clone(), self.torrent_engine.clone());

        // Start progressive streaming
        debug!(
            "feed_ffmpeg_data: Starting progressive streaming for {}",
            info_hash
        );
        let streaming_handle = progressive_streaming
            .start_streaming(
                info_hash,
                temp_input,
                output_path.clone(),
                file_size,
                &remux_options,
            )
            .await?;
        debug!(
            "feed_ffmpeg_data: Progressive streaming started successfully for {}",
            info_hash
        );

        // Store streaming handle in session for monitoring
        let handle_arc = std::sync::Arc::new(streaming_handle);
        {
            let mut sessions = self.sessions.write().await;
            if let Some(session) = sessions.get_mut(&info_hash) {
                session.streaming_handle = Some(handle_arc.clone());
            }
        }

        // Start monitoring task
        let sessions = self.sessions.clone();
        let info_hash_copy = info_hash;
        let output_path_copy = output_path.clone();
        let handle_monitor = handle_arc.clone();

        tokio::spawn(async move {
            let mut check_interval = tokio::time::interval(std::time::Duration::from_secs(1));
            let mut consecutive_ready_checks = 0;

            loop {
                check_interval.tick().await;

                // Check if streaming output is ready
                let is_ready = handle_monitor.is_ready().await;

                if is_ready {
                    consecutive_ready_checks += 1;

                    // If ready for several checks, mark as completed
                    if consecutive_ready_checks >= 3 {
                        info!(
                            "Progressive streaming ready for {} (output file available)",
                            info_hash_copy
                        );

                        // Mark session as completed
                        let mut sessions = sessions.write().await;
                        if let Some(session) = sessions.get_mut(&info_hash_copy) {
                            session.state = RemuxState::Completed {
                                output_path: output_path_copy.clone(),
                            };
                            session.touch();
                        }

                        // Wait for streaming to complete in background
                        let handle_clone = Arc::clone(&handle_monitor);
                        tokio::spawn(async move {
                            if let Err(e) = handle_clone.wait_completion().await {
                                error!("Streaming completion failed: {}", e);
                            }
                        });

                        break;
                    }
                } else {
                    consecutive_ready_checks = 0;
                }
            }
        });

        info!(
            "Started progressive streaming for {} -> {}",
            info_hash,
            output_path.display()
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

        // For progressive streaming, we can start with much less data
        let required_head_size = if is_avi_file {
            // For AVI transcoding, only need 512KB to start - progressive feeder handles the rest
            (512 * 1024).min(file_size)
        } else {
            // For other formats, still use less data for progressive streaming
            (2 * 1024 * 1024)
                .min(file_size)
                .min(self.config.min_head_size)
        };

        debug!(
            "Checking required data for {}: file_size={}, required_head_size={}, min_head_config={}, is_avi={}",
            info_hash, file_size, required_head_size, self.config.min_head_size, is_avi_file
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

        // For very small files (< 5MB), require more data, but still allow progressive streaming
        if file_size < 5 * 1024 * 1024 {
            debug!(
                "Small file detected ({}MB), requiring more data but allowing progressive",
                file_size / 1024 / 1024
            );
            let target_size = (file_size * 3 / 4).max(required_head_size); // Require 75% of small files
            let sufficient_data_available = self
                .data_source
                .check_range_availability(info_hash, 0..target_size)
                .await
                .map(|availability| {
                    debug!(
                        "Full file availability for {}: available={}, missing_pieces={}",
                        info_hash,
                        availability.available,
                        availability.missing_pieces.len()
                    );
                    availability.available
                })
                .unwrap_or(false);
            return Ok(sufficient_data_available);
        }

        // For streaming optimization, start remuxing with just head data
        // This allows us to extract metadata and prepare the stream quickly
        if head_available {
            debug!("Head data available, starting remux for {}", info_hash);
            return Ok(true);
        }

        // Fallback: Check if we have both head and tail data (original behavior)
        let tail_start = file_size.saturating_sub(self.config.min_tail_size);
        let tail_available = self
            .data_source
            .check_range_availability(info_hash, tail_start..file_size)
            .await
            .map(|availability| {
                debug!(
                    "Tail data availability for {}: range={}..{}, available={}, missing_pieces={}",
                    info_hash,
                    tail_start,
                    file_size,
                    availability.available,
                    availability.missing_pieces.len()
                );
                availability.available
            })
            .unwrap_or(false);

        let result = head_available && tail_available;
        debug!(
            "Final readiness check for {}: head_available={}, tail_available={}, result={}",
            info_hash, head_available, tail_available, result
        );

        Ok(result)
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

    /// Check if a partially transcoded file is ready for streaming
    async fn is_partial_file_ready(&self, info_hash: InfoHash) -> bool {
        debug!(
            "is_partial_file_ready: Checking readiness for {}",
            info_hash
        );
        let sessions = self.sessions.read().await;
        let session = match sessions.get(&info_hash) {
            Some(session) => session,
            None => {
                debug!("is_partial_file_ready: No session found for {}", info_hash);
                return false;
            }
        };

        let output_path = match &session.output_path {
            Some(path) => path,
            None => {
                debug!("is_partial_file_ready: No output path for {}", info_hash);
                return false;
            }
        };

        // Check if the output file exists and has valid MP4 headers
        if !output_path.exists() {
            debug!(
                "is_partial_file_ready: Output file does not exist: {}",
                output_path.display()
            );
            return false;
        }

        // Check file size - we need at least 64KB for basic streaming
        if let Ok(metadata) = std::fs::metadata(output_path) {
            let file_size = metadata.len();
            debug!(
                "is_partial_file_ready: Output file size for {}: {} bytes",
                info_hash, file_size
            );

            if file_size < 64 * 1024 {
                debug!(
                    "is_partial_file_ready: File too small (< 64KB) for {}",
                    info_hash
                );
                return false;
            }

            // For fragmented MP4 progressive streaming, we need at least 128KB to start
            // This is much less than traditional faststart MP4
            if file_size < 128 * 1024 {
                debug!(
                    "is_partial_file_ready: File too small (< 128KB) for progressive streaming for {}",
                    info_hash
                );
                return false;
            }
        } else {
            debug!(
                "is_partial_file_ready: Failed to get metadata for {}",
                info_hash
            );
            return false;
        }

        // Validate MP4 structure - check for ftyp and moov atoms
        // Only read the first 64KB to check headers (don't read entire file)
        const HEADER_CHECK_SIZE: usize = 64 * 1024;
        if let Ok(mut file) = std::fs::File::open(output_path) {
            use std::io::Read;
            let mut buffer = vec![0u8; HEADER_CHECK_SIZE];
            if let Ok(bytes_read) = file.read(&mut buffer) {
                debug!(
                    "is_partial_file_ready: Read {} bytes from output file for {}",
                    bytes_read, info_hash
                );

                if bytes_read < 32 {
                    debug!(
                        "is_partial_file_ready: Too few bytes read (< 32) for {}",
                        info_hash
                    );
                    return false;
                }

                buffer.truncate(bytes_read);

                let has_ftyp =
                    buffer.len() >= 8 && (buffer[4..8] == *b"ftyp" || buffer.starts_with(b"ftyp"));

                // For fragmented MP4, look for moov atom OR moof atom (fragment)
                let has_moov = buffer.windows(4).any(|window| window == b"moov");
                let has_moof = buffer.windows(4).any(|window| window == b"moof");

                debug!(
                    "is_partial_file_ready: MP4 validation for {}: has_ftyp={}, has_moov={}, has_moof={}",
                    info_hash, has_ftyp, has_moov, has_moof
                );

                if has_ftyp && (has_moov || has_moof) {
                    let file_size = std::fs::metadata(output_path).map(|m| m.len()).unwrap_or(0);
                    tracing::info!(
                        "Partial fragmented MP4 file ready for streaming: {} ({} bytes total)",
                        info_hash,
                        file_size
                    );
                    return true;
                } else {
                    debug!(
                        "is_partial_file_ready: MP4 validation failed for {}",
                        info_hash
                    );
                }
            } else {
                debug!(
                    "is_partial_file_ready: Failed to read from output file for {}",
                    info_hash
                );
            }
        } else {
            debug!(
                "is_partial_file_ready: Failed to open output file for {}",
                info_hash
            );
        }

        false
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
