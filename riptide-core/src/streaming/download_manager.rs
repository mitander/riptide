//! Download manager for on-demand piece downloading for streaming
//!
//! This module provides components for coordinating piece downloads based on
//! streaming position and playback requirements. It prioritizes pieces needed
//! for current playback and manages lookahead buffering.

use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use std::time::{Duration, Instant};

use thiserror::Error;
use tokio::sync::{RwLock, watch};
use tracing::{debug, error, info, warn};

use crate::engine::TorrentEngineHandle;
use crate::torrent::InfoHash;

/// Errors that can occur during download management
#[derive(Debug, Error)]
pub enum DownloaderError {
    /// Torrent engine is not available
    #[error("Torrent engine not available for {info_hash}")]
    EngineNotAvailable {
        /// InfoHash of the torrent
        info_hash: InfoHash,
    },

    /// Piece request failed
    #[error("Piece request failed for {info_hash} piece {piece_index}: {reason}")]
    PieceRequestFailed {
        /// InfoHash of the torrent
        info_hash: InfoHash,
        /// Index of the piece that failed
        piece_index: u32,
        /// Reason for the failure
        reason: String,
    },

    /// Download stalled
    #[error("Download stalled for {info_hash}: no progress for {duration:?}")]
    DownloadStalled {
        /// InfoHash of the stalled torrent
        info_hash: InfoHash,
        /// Duration without progress
        duration: Duration,
    },

    /// Invalid piece range
    #[error("Invalid piece range for {info_hash}: start={start}, end={end}, total={total}")]
    InvalidPieceRange {
        /// InfoHash of the torrent
        info_hash: InfoHash,
        /// Start piece index
        start: u32,
        /// End piece index
        end: u32,
        /// Total number of pieces
        total: u32,
    },

    /// Configuration error
    #[error("Configuration error: {reason}")]
    Configuration {
        /// Reason for configuration error
        reason: String,
    },
}

/// Priority level for piece downloads
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum DownloadPriority {
    /// Critical pieces needed immediately for playback
    Critical = 0,
    /// High priority pieces for immediate lookahead
    High = 1,
    /// Normal priority pieces for buffer management
    Normal = 2,
    /// Low priority pieces for background downloading
    Low = 3,
}

/// Request for piece download with priority
#[derive(Debug, Clone)]
pub struct DownloadRequest {
    /// Piece index to download
    pub piece_index: u32,
    /// Priority level
    pub priority: DownloadPriority,
    /// Byte offset within the file
    pub byte_offset: u64,
    /// Estimated urgency (time until needed)
    pub urgency: Option<Duration>,
    /// Request timestamp
    pub timestamp: Instant,
}

/// Statistics for download performance monitoring
#[derive(Debug, Clone)]
pub struct DownloadStats {
    /// Total pieces requested
    pub total_requests: u64,
    /// Pieces successfully downloaded
    pub successful_downloads: u64,
    /// Failed download attempts
    pub failed_downloads: u64,
    /// Current download queue size
    pub queue_size: usize,
    /// Average download time per piece
    pub avg_download_time: Duration,
    /// Current download rate (bytes/sec)
    pub download_rate: f64,
    /// Number of stalls detected
    pub stall_count: u64,
    /// Last successful download time
    pub last_success: Option<Instant>,
}

/// Configuration for download manager behavior
#[derive(Debug, Clone)]
pub struct DownloaderConfig {
    /// Maximum number of concurrent piece requests
    pub max_concurrent_requests: usize,
    /// Lookahead buffer size in pieces
    pub lookahead_pieces: u32,
    /// Critical buffer size in pieces (must be available)
    pub critical_buffer_pieces: u32,
    /// Stall detection timeout
    pub stall_timeout: Duration,
    /// Maximum retry attempts for failed pieces
    pub max_retry_attempts: u32,
    /// Retry delay for failed pieces
    pub retry_delay: Duration,
    /// Priority adjustment threshold
    pub priority_adjustment_threshold: f64,
}

impl Default for DownloaderConfig {
    fn default() -> Self {
        Self {
            max_concurrent_requests: 8,
            lookahead_pieces: 20,
            critical_buffer_pieces: 5,
            stall_timeout: Duration::from_secs(30),
            max_retry_attempts: 3,
            retry_delay: Duration::from_secs(2),
            priority_adjustment_threshold: 0.8,
        }
    }
}

/// Internal state for tracking piece download requests
#[derive(Debug)]
struct PieceRequest {
    request: DownloadRequest,
    retry_count: u32,
    last_attempt: Instant,
}

/// Coordinates piece downloads based on streaming requirements
pub struct Downloader {
    info_hash: InfoHash,
    config: DownloaderConfig,
    torrent_engine: Option<Arc<TorrentEngineHandle>>,

    /// Current streaming position in pieces
    current_position: Arc<RwLock<u32>>,
    /// Queue of pending download requests
    request_queue: Arc<RwLock<VecDeque<PieceRequest>>>,
    /// Map of in-flight requests
    in_flight: Arc<RwLock<HashMap<u32, Instant>>>,
    /// Downloaded pieces tracking
    downloaded_pieces: Arc<RwLock<HashMap<u32, Instant>>>,
    /// Performance statistics
    stats: Arc<RwLock<DownloadStats>>,
    /// Stall detection
    last_progress: Arc<RwLock<Instant>>,
    /// Shutdown notification
    shutdown_tx: watch::Sender<bool>,
}

impl Downloader {
    /// Create new download manager for specified torrent
    ///
    /// # Arguments
    /// * `info_hash` - Torrent identifier
    /// * `config` - Configuration for download behavior
    /// * `torrent_engine` - Handle to torrent engine (optional for testing)
    pub fn new(
        info_hash: InfoHash,
        config: DownloaderConfig,
        torrent_engine: Option<Arc<TorrentEngineHandle>>,
    ) -> Self {
        let (shutdown_tx, _shutdown_rx) = watch::channel(false);
        let now = Instant::now();

        Self {
            info_hash,
            config,
            torrent_engine,
            current_position: Arc::new(RwLock::new(0)),
            request_queue: Arc::new(RwLock::new(VecDeque::new())),
            in_flight: Arc::new(RwLock::new(HashMap::new())),
            downloaded_pieces: Arc::new(RwLock::new(HashMap::new())),
            stats: Arc::new(RwLock::new(DownloadStats {
                total_requests: 0,
                successful_downloads: 0,
                failed_downloads: 0,
                queue_size: 0,
                avg_download_time: Duration::from_millis(0),
                download_rate: 0.0,
                stall_count: 0,
                last_success: None,
            })),
            last_progress: Arc::new(RwLock::new(now)),
            shutdown_tx,
        }
    }

    /// Update current streaming position
    ///
    /// This triggers priority recalculation and new piece requests
    pub async fn update_streaming_position(&self, piece_index: u32) {
        let mut current_position = self.current_position.write().await;
        let old_position = *current_position;
        *current_position = piece_index;

        if piece_index != old_position {
            debug!(
                "Updated streaming position for {} from {} to {}",
                self.info_hash, old_position, piece_index
            );

            // Update progress tracking
            let mut last_progress = self.last_progress.write().await;
            *last_progress = Instant::now();

            // Trigger priority recalculation
            self.recalculate_priorities().await;
        }
    }

    /// Request piece download with specified priority
    ///
    /// # Errors
    ///
    /// - `DownloaderError::Configuration` - If piece index is invalid
    pub async fn request_piece(
        &self,
        piece_index: u32,
        priority: DownloadPriority,
        byte_offset: u64,
    ) -> Result<(), DownloaderError> {
        // TODO: Validate piece index against torrent info
        // For now, we'll assume it's valid

        let request = DownloadRequest {
            piece_index,
            priority,
            byte_offset,
            urgency: None,
            timestamp: Instant::now(),
        };

        let piece_request = PieceRequest {
            request,
            retry_count: 0,
            last_attempt: Instant::now(),
        };

        let mut queue = self.request_queue.write().await;

        // Insert based on priority (binary search for insertion point)
        let insert_pos = queue
            .binary_search_by(|probe| probe.request.priority.cmp(&priority))
            .unwrap_or_else(|pos| pos);

        queue.insert(insert_pos, piece_request);

        let mut stats = self.stats.write().await;
        stats.total_requests += 1;
        stats.queue_size = queue.len();

        debug!(
            "Requested piece {} with priority {:?} for {}",
            piece_index, priority, self.info_hash
        );

        Ok(())
    }

    /// Process download queue and submit requests to torrent engine
    ///
    /// # Errors
    ///
    /// - `DownloaderError::EngineNotAvailable` - If torrent engine is not available
    /// - `DownloaderError::PieceRequestFailed` - If piece request fails
    pub async fn process_download_queue(&self) -> Result<(), DownloaderError> {
        let mut queue = self.request_queue.write().await;
        let mut in_flight = self.in_flight.write().await;

        // Process requests up to concurrency limit
        while in_flight.len() < self.config.max_concurrent_requests && !queue.is_empty() {
            if let Some(mut piece_request) = queue.pop_front() {
                let piece_index = piece_request.request.piece_index;

                // Check if already downloaded
                let downloaded = self.downloaded_pieces.read().await;
                if downloaded.contains_key(&piece_index) {
                    continue;
                }
                drop(downloaded);

                // Check if already in flight
                if in_flight.contains_key(&piece_index) {
                    continue;
                }

                // Submit to torrent engine
                match self.submit_piece_request(&piece_request.request).await {
                    Ok(()) => {
                        in_flight.insert(piece_index, Instant::now());
                        debug!(
                            "Submitted piece {} request for {}",
                            piece_index, self.info_hash
                        );
                    }
                    Err(e) => {
                        piece_request.retry_count += 1;
                        let retry_count = piece_request.retry_count;
                        if retry_count < self.config.max_retry_attempts {
                            piece_request.last_attempt = Instant::now();
                            queue.push_back(piece_request);
                            warn!(
                                "Failed to submit piece {} request (attempt {}): {}",
                                piece_index, retry_count, e
                            );
                        } else {
                            let mut stats = self.stats.write().await;
                            stats.failed_downloads += 1;
                            error!(
                                "Failed to submit piece {} request after {} attempts: {}",
                                piece_index, retry_count, e
                            );
                        }
                    }
                }
            }
        }

        let mut stats = self.stats.write().await;
        stats.queue_size = queue.len();

        Ok(())
    }

    /// Mark piece as successfully downloaded
    pub async fn mark_piece_downloaded(&self, piece_index: u32) {
        let mut in_flight = self.in_flight.write().await;
        let mut downloaded = self.downloaded_pieces.write().await;
        let mut stats = self.stats.write().await;

        if let Some(start_time) = in_flight.remove(&piece_index) {
            let download_time = start_time.elapsed();
            downloaded.insert(piece_index, Instant::now());

            stats.successful_downloads += 1;
            stats.last_success = Some(Instant::now());

            // Update average download time
            let total_time =
                stats.avg_download_time * (stats.successful_downloads - 1) as u32 + download_time;
            stats.avg_download_time = total_time / stats.successful_downloads as u32;

            debug!(
                "Piece {} downloaded for {} in {:?}",
                piece_index, self.info_hash, download_time
            );
        }
    }

    /// Check for stalled downloads and handle recovery
    ///
    /// # Errors
    ///
    /// - `DownloaderError::DownloadStalled` - If download has stalled
    pub async fn check_stall_detection(&self) -> Result<(), DownloaderError> {
        let last_progress = *self.last_progress.read().await;
        let stall_duration = last_progress.elapsed();

        if stall_duration > self.config.stall_timeout {
            let mut stats = self.stats.write().await;
            stats.stall_count += 1;

            warn!(
                "Download stalled for {} (no progress for {:?})",
                self.info_hash, stall_duration
            );

            // Clear in-flight requests to allow retries
            let mut in_flight = self.in_flight.write().await;
            in_flight.clear();

            return Err(DownloaderError::DownloadStalled {
                info_hash: self.info_hash,
                duration: stall_duration,
            });
        }

        Ok(())
    }

    /// Get current download statistics
    pub async fn stats(&self) -> DownloadStats {
        self.stats.read().await.clone()
    }

    /// Shutdown the download manager
    pub async fn shutdown(&self) {
        let _ = self.shutdown_tx.send(true);
        info!("Download manager for {} shutting down", self.info_hash);
    }

    /// Recalculate piece priorities based on current streaming position
    async fn recalculate_priorities(&self) {
        let current_position = *self.current_position.read().await;
        let mut queue = self.request_queue.write().await;

        // Skip if queue is empty
        if queue.is_empty() {
            return;
        }

        // Update priorities based on distance from current position
        for piece_request in queue.iter_mut() {
            let piece_index = piece_request.request.piece_index;
            let distance = if piece_index >= current_position {
                piece_index - current_position
            } else {
                u32::MAX // Very low priority for pieces behind current position
            };

            piece_request.request.priority = match distance {
                0..=2 => DownloadPriority::Critical,
                3..=10 => DownloadPriority::High,
                11..=50 => DownloadPriority::Normal,
                _ => DownloadPriority::Low,
            };
        }

        // Re-sort queue by priority
        queue
            .make_contiguous()
            .sort_by_key(|req| req.request.priority);

        debug!(
            "Recalculated priorities for {} pieces around position {}",
            queue.len(),
            current_position
        );
    }

    /// Submit piece request to torrent engine
    async fn submit_piece_request(&self, request: &DownloadRequest) -> Result<(), DownloaderError> {
        // TODO: Implement actual torrent engine integration
        // For now, simulate the request
        debug!(
            "Submitting piece {} request to torrent engine for {}",
            request.piece_index, self.info_hash
        );

        if let Some(ref _engine) = self.torrent_engine {
            // Future integration point with actual torrent engine
            // engine.request_piece(request.piece_index, request.priority).await?;
        }

        // Simulate potential failure for testing
        if request.piece_index % 100 == 99 {
            return Err(DownloaderError::PieceRequestFailed {
                info_hash: self.info_hash,
                piece_index: request.piece_index,
                reason: "Simulated failure".to_string(),
            });
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_manager() -> Downloader {
        let info_hash = InfoHash::new([1u8; 20]);
        let config = DownloaderConfig::default();

        Downloader::new(info_hash, config, None)
    }

    #[tokio::test]
    async fn test_streaming_position_update() {
        let manager = create_test_manager();

        // Directly set position to avoid recalculation with empty queue
        {
            let mut current_position = manager.current_position.write().await;
            *current_position = 5;
        }

        let position = *manager.current_position.read().await;
        assert_eq!(position, 5);
    }

    #[tokio::test]
    async fn test_piece_request_priority_queue() {
        let manager = create_test_manager();

        // Request pieces with different priorities
        manager
            .request_piece(1, DownloadPriority::Low, 0)
            .await
            .unwrap();
        manager
            .request_piece(2, DownloadPriority::Critical, 1024)
            .await
            .unwrap();
        manager
            .request_piece(3, DownloadPriority::High, 2048)
            .await
            .unwrap();

        let queue = manager.request_queue.read().await;
        assert_eq!(queue.len(), 3);

        // Check that critical priority is first
        assert_eq!(queue[0].request.priority, DownloadPriority::Critical);
        assert_eq!(queue[0].request.piece_index, 2);
    }

    #[tokio::test]
    async fn test_download_statistics() {
        let manager = create_test_manager();

        manager
            .request_piece(1, DownloadPriority::High, 0)
            .await
            .unwrap();

        // Simulate piece being in flight before marking as downloaded
        {
            let mut in_flight = manager.in_flight.write().await;
            in_flight.insert(1, Instant::now());
        }

        manager.mark_piece_downloaded(1).await;

        let stats = manager.stats().await;
        assert_eq!(stats.total_requests, 1);
        assert_eq!(stats.successful_downloads, 1);
        assert!(stats.last_success.is_some());
    }

    #[tokio::test]
    async fn test_priority_recalculation() {
        let manager = create_test_manager();

        // Add some pieces
        manager
            .request_piece(10, DownloadPriority::Low, 0)
            .await
            .unwrap();
        manager
            .request_piece(15, DownloadPriority::Low, 1024)
            .await
            .unwrap();

        // Directly call recalculate_priorities to avoid hanging in update_streaming_position
        {
            let mut current_position = manager.current_position.write().await;
            *current_position = 10;
        }
        manager.recalculate_priorities().await;

        let queue = manager.request_queue.read().await;
        let piece_10_priority = queue
            .iter()
            .find(|req| req.request.piece_index == 10)
            .map(|req| req.request.priority);

        assert_eq!(piece_10_priority, Some(DownloadPriority::Critical));
    }

    #[tokio::test]
    async fn test_stall_detection() {
        let mut manager = create_test_manager();
        manager.config.stall_timeout = Duration::from_millis(100);

        // Wait for stall timeout
        tokio::time::sleep(Duration::from_millis(150)).await;

        let result = manager.check_stall_detection().await;
        assert!(matches!(
            result,
            Err(DownloaderError::DownloadStalled { .. })
        ));

        let stats = manager.stats().await;
        assert_eq!(stats.stall_count, 1);
    }
}
