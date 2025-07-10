//! Session management for remuxing operations

use std::collections::HashMap;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use tokio::sync::{RwLock, Semaphore};
use tracing::{error, info, warn};

use super::state::{RemuxError, RemuxState};
use super::types::{RemuxConfig, RemuxSession, StreamHandle, StreamReadiness, StreamingStatus};
use crate::streaming::{FileAssembler, StrategyError, StreamingResult};
use crate::torrent::InfoHash;

/// Manages remuxing sessions with state machine and concurrency control
pub struct RemuxSessionManager {
    sessions: Arc<RwLock<HashMap<InfoHash, RemuxSession>>>,
    semaphore: Arc<Semaphore>,
    config: RemuxConfig,
    session_counter: AtomicU64,
    file_assembler: Arc<dyn FileAssembler>,
}

impl RemuxSessionManager {
    /// Create a new remux session manager
    pub fn new(config: RemuxConfig, file_assembler: Arc<dyn FileAssembler>) -> Self {
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
            file_assembler,
        }
    }

    /// Get or create a session handle for the given info hash
    pub async fn get_or_create_session(
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
    pub async fn check_readiness(&self, info_hash: InfoHash) -> StreamingResult<StreamReadiness> {
        let sessions = self.sessions.read().await;
        let session = sessions
            .get(&info_hash)
            .ok_or_else(|| StrategyError::RemuxingFailed {
                reason: "Session not found".to_string(),
            })?;

        match &session.state {
            RemuxState::WaitingForHeadAndTail => {
                drop(sessions); // Release read lock before async operations

                if self.has_required_data(info_hash).await? {
                    // Transition to remuxing state
                    self.start_remuxing(info_hash).await?;
                    Ok(StreamReadiness::Processing)
                } else {
                    Ok(StreamReadiness::WaitingForData)
                }
            }
            RemuxState::Remuxing { .. } => Ok(StreamReadiness::Processing),
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
    pub async fn get_status(&self, info_hash: InfoHash) -> StreamingResult<StreamingStatus> {
        let sessions = self.sessions.read().await;
        let session = sessions
            .get(&info_hash)
            .ok_or_else(|| StrategyError::RemuxingFailed {
                reason: "Session not found".to_string(),
            })?;

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
    pub async fn get_output_path(&self, info_hash: InfoHash) -> StreamingResult<PathBuf> {
        let sessions = self.sessions.read().await;
        let session = sessions
            .get(&info_hash)
            .ok_or_else(|| StrategyError::RemuxingFailed {
                reason: "Session not found".to_string(),
            })?;

        match &session.state {
            RemuxState::Completed { output_path } => Ok(output_path.clone()),
            _ => Err(StrategyError::StreamingNotReady {
                reason: "Remuxing not completed".to_string(),
            }),
        }
    }

    /// Start the remuxing process for a session
    async fn start_remuxing(&self, info_hash: InfoHash) -> StreamingResult<()> {
        // Acquire semaphore permit for concurrency control
        let _permit =
            self.semaphore
                .acquire()
                .await
                .map_err(|_| StrategyError::RemuxingFailed {
                    reason: "Failed to acquire remuxing permit".to_string(),
                })?;

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
        let session_manager = self.clone();
        let info_hash_copy = info_hash;
        tokio::spawn(async move {
            if let Err(e) = session_manager.feed_ffmpeg_data(info_hash_copy).await {
                error!("Failed to feed FFmpeg data for {}: {}", info_hash_copy, e);
                session_manager
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

    /// Feed data to FFmpeg process and monitor completion
    async fn feed_ffmpeg_data(&self, info_hash: InfoHash) -> StreamingResult<()> {
        // Implementation would:
        // 1. Get file size from file assembler
        // 2. Feed data sequentially to FFmpeg stdin
        // 3. Monitor FFmpeg stderr for progress updates
        // 4. Update session progress
        // 5. Mark as completed or failed when FFmpeg exits

        // For now, simulate the process
        tokio::time::sleep(Duration::from_secs(5)).await;

        // Mark as completed
        let output_path = {
            let sessions = self.sessions.read().await;
            let session =
                sessions
                    .get(&info_hash)
                    .ok_or_else(|| StrategyError::RemuxingFailed {
                        reason: "Session not found".to_string(),
                    })?;
            session
                .output_path
                .clone()
                .ok_or_else(|| StrategyError::RemuxingFailed {
                    reason: "No output path".to_string(),
                })?
        };

        self.mark_completed(info_hash, output_path).await;
        Ok(())
    }

    /// Mark a session as completed
    async fn mark_completed(&self, info_hash: InfoHash, output_path: PathBuf) {
        let mut sessions = self.sessions.write().await;
        if let Some(session) = sessions.get_mut(&info_hash) {
            session.state = RemuxState::Completed {
                output_path: output_path.clone(),
            };
            session.ffmpeg_handle = None; // Process has finished
            session.touch();

            info!(
                "Remuxing completed for {} -> {}",
                info_hash,
                output_path.display()
            );
        }
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
        let file_size = self
            .file_assembler
            .file_size(info_hash)
            .await
            .map_err(|e| StrategyError::RemuxingFailed {
                reason: format!("Failed to get file size: {e}"),
            })?;

        // Check if we have head data
        let head_available = self
            .file_assembler
            .is_range_available(info_hash, 0..self.config.min_head_size.min(file_size));

        // Check if we have tail data
        let tail_start = file_size.saturating_sub(self.config.min_tail_size);
        let tail_available = self
            .file_assembler
            .is_range_available(info_hash, tail_start..file_size);

        Ok(head_available && tail_available)
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

impl Clone for RemuxSessionManager {
    fn clone(&self) -> Self {
        Self {
            sessions: Arc::clone(&self.sessions),
            semaphore: Arc::clone(&self.semaphore),
            config: self.config.clone(),
            session_counter: AtomicU64::new(self.session_counter.load(Ordering::SeqCst)),
            file_assembler: Arc::clone(&self.file_assembler),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap as StdHashMap;

    use async_trait::async_trait;

    use super::*;
    use crate::streaming::FileAssemblerError;

    struct MockFileAssembler {
        files: StdHashMap<InfoHash, u64>,
        available_ranges: StdHashMap<InfoHash, Vec<std::ops::Range<u64>>>,
    }

    impl MockFileAssembler {
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
    impl FileAssembler for MockFileAssembler {
        async fn read_range(
            &self,
            _info_hash: InfoHash,
            _range: std::ops::Range<u64>,
        ) -> Result<Vec<u8>, FileAssemblerError> {
            Ok(vec![0u8; 1024])
        }

        async fn file_size(&self, info_hash: InfoHash) -> Result<u64, FileAssemblerError> {
            self.files
                .get(&info_hash)
                .copied()
                .ok_or_else(|| FileAssemblerError::CacheError {
                    reason: "File not found".to_string(),
                })
        }

        fn is_range_available(&self, info_hash: InfoHash, range: std::ops::Range<u64>) -> bool {
            if let Some(available) = self.available_ranges.get(&info_hash) {
                available
                    .iter()
                    .any(|r| r.start <= range.start && r.end >= range.end)
            } else {
                false
            }
        }
    }

    #[tokio::test]
    async fn test_session_creation() {
        let config = RemuxConfig::default();
        let mut file_assembler = MockFileAssembler::new();
        let info_hash = InfoHash::new([1u8; 20]);
        file_assembler.add_file(info_hash, 10 * 1024 * 1024);

        let manager = RemuxSessionManager::new(config, Arc::new(file_assembler));

        let handle = manager.get_or_create_session(info_hash).await.unwrap();
        assert_eq!(handle.info_hash, info_hash);
        assert_eq!(handle.session_id, 1);
    }

    #[tokio::test]
    async fn test_readiness_without_data() {
        let config = RemuxConfig::default();
        let mut file_assembler = MockFileAssembler::new();
        let info_hash = InfoHash::new([1u8; 20]);
        file_assembler.add_file(info_hash, 10 * 1024 * 1024);

        let manager = RemuxSessionManager::new(config, Arc::new(file_assembler));
        let _handle = manager.get_or_create_session(info_hash).await.unwrap();

        let readiness = manager.check_readiness(info_hash).await.unwrap();
        assert_eq!(readiness, StreamReadiness::WaitingForData);
    }

    #[tokio::test]
    async fn test_readiness_with_data() {
        let config = RemuxConfig::default();
        let mut file_assembler = MockFileAssembler::new();
        let info_hash = InfoHash::new([1u8; 20]);
        let file_size = 10 * 1024 * 1024u64;
        file_assembler.add_file(info_hash, file_size);

        // Make head and tail available
        file_assembler.make_range_available(info_hash, 0..config.min_head_size);
        file_assembler
            .make_range_available(info_hash, (file_size - config.min_tail_size)..file_size);

        let manager = RemuxSessionManager::new(config, Arc::new(file_assembler));
        let _handle = manager.get_or_create_session(info_hash).await.unwrap();

        let readiness = manager.check_readiness(info_hash).await.unwrap();
        assert_eq!(readiness, StreamReadiness::Processing);
    }
}
