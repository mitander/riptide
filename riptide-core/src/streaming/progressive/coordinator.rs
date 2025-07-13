//! Progressive streaming coordinator for robust data flow management
//!
//! This module provides a testable coordinator that manages the complex interaction
//! between torrent downloading, progressive feeding, and FFmpeg processing.

use std::sync::Arc;
use std::time::{Duration, Instant};

use tokio::sync::{RwLock, mpsc};
use tracing::{debug, error, info, warn};

use crate::storage::DataSource;
use crate::streaming::{StreamingError, StreamingResult};
use crate::torrent::InfoHash;

/// Events that can occur during progressive streaming
#[derive(Debug, Clone)]
pub enum ProgressiveEvent {
    /// More data has become available from the torrent
    DataAvailable {
        /// Range of bytes that became available
        range: std::ops::Range<u64>,
    },
    /// Data was successfully written to FFmpeg
    DataWritten {
        /// Total bytes written so far
        bytes: u64,
    },
    /// FFmpeg process has exited
    ProcessExited {
        /// Exit status from FFmpeg
        status: std::process::ExitStatus,
    },
    /// Streaming position should be updated for piece prioritization
    UpdateStreamingPosition {
        /// New streaming position
        position: u64,
    },
    /// Request to shutdown the coordinator
    Shutdown,
}

/// Current state of the progressive streaming coordinator
#[derive(Debug, Clone)]
pub struct CoordinatorState {
    /// Total bytes that have been written to FFmpeg
    pub bytes_written: u64,
    /// Total file size being streamed
    pub file_size: u64,
    /// Whether FFmpeg process is still running
    pub ffmpeg_running: bool,
    /// Last time progress was made
    pub last_progress: Instant,
    /// Current streaming position (for piece prioritization)
    pub streaming_position: u64,
}

/// Coordinates progressive streaming with testable event-driven architecture
pub struct ProgressiveCoordinator {
    info_hash: InfoHash,
    data_source: Arc<dyn DataSource>,
    state: Arc<RwLock<CoordinatorState>>,
    event_tx: mpsc::UnboundedSender<ProgressiveEvent>,
    event_rx: Option<mpsc::UnboundedReceiver<ProgressiveEvent>>,
}

impl ProgressiveCoordinator {
    /// Create a new progressive streaming coordinator
    pub fn new(info_hash: InfoHash, data_source: Arc<dyn DataSource>, file_size: u64) -> Self {
        let (event_tx, event_rx) = mpsc::unbounded_channel();

        let state = Arc::new(RwLock::new(CoordinatorState {
            bytes_written: 0,
            file_size,
            ffmpeg_running: false,
            last_progress: Instant::now(),
            streaming_position: 0,
        }));

        Self {
            info_hash,
            data_source,
            state,
            event_tx,
            event_rx: Some(event_rx),
        }
    }

    /// Get the current state
    pub async fn state(&self) -> CoordinatorState {
        self.state.read().await.clone()
    }

    /// Send an event to the coordinator
    ///
    /// # Errors
    /// Returns `StreamingError` if the event channel is closed
    pub fn send_event(&self, event: ProgressiveEvent) -> Result<(), StreamingError> {
        self.event_tx
            .send(event)
            .map_err(|_| StreamingError::RemuxingFailed {
                reason: "Failed to send event to coordinator".to_string(),
            })
    }

    /// Start the coordinator event loop
    ///
    /// # Errors
    /// Returns `StreamingError` if the coordinator is already running or times out
    pub async fn run(&mut self) -> StreamingResult<()> {
        let mut event_rx = self
            .event_rx
            .take()
            .ok_or_else(|| StreamingError::RemuxingFailed {
                reason: "Coordinator already running".to_string(),
            })?;

        let mut data_check_interval = tokio::time::interval(Duration::from_millis(100));
        let progress_timeout = Duration::from_secs(30);

        info!(
            "Progressive streaming coordinator started for {}",
            self.info_hash
        );

        loop {
            tokio::select! {
                // Handle events
                event = event_rx.recv() => {
                    match event {
                        Some(ProgressiveEvent::Shutdown) => {
                            info!("Coordinator shutdown requested for {}", self.info_hash);
                            break;
                        }
                        Some(event) => {
                            if let Err(e) = self.handle_event(event).await {
                                error!("Error handling event for {}: {}", self.info_hash, e);
                                return Err(e);
                            }
                        }
                        None => {
                            debug!("Event channel closed for {}", self.info_hash);
                            break;
                        }
                    }
                }

                // Periodically check for new data availability
                _ = data_check_interval.tick() => {
                    if let Err(e) = self.check_data_availability().await {
                        error!("Error checking data availability for {}: {}", self.info_hash, e);
                        return Err(e);
                    }
                }
            }

            // Check for timeout
            let state = self.state.read().await;
            if state.ffmpeg_running && state.last_progress.elapsed() > progress_timeout {
                warn!(
                    "Progressive streaming timeout for {} after {} seconds",
                    self.info_hash,
                    progress_timeout.as_secs()
                );
                return Err(StreamingError::RemuxingFailed {
                    reason: "Progressive streaming timeout".to_string(),
                });
            }
        }

        Ok(())
    }

    /// Handle a specific event
    async fn handle_event(&self, event: ProgressiveEvent) -> StreamingResult<()> {
        match event {
            ProgressiveEvent::DataAvailable { range } => {
                debug!("Data available for {}: {:?}", self.info_hash, range);
                // This would trigger writing data to FFmpeg
                // Implementation would go here
            }
            ProgressiveEvent::DataWritten { bytes } => {
                let mut state = self.state.write().await;
                state.bytes_written = bytes;
                state.last_progress = Instant::now();
                debug!("Data written for {}: {} bytes total", self.info_hash, bytes);
            }
            ProgressiveEvent::ProcessExited { status } => {
                let mut state = self.state.write().await;
                state.ffmpeg_running = false;
                if status.success() {
                    info!("FFmpeg completed successfully for {}", self.info_hash);
                } else {
                    warn!(
                        "FFmpeg exited with status {} for {}",
                        status, self.info_hash
                    );
                }
            }
            ProgressiveEvent::UpdateStreamingPosition { position } => {
                let mut state = self.state.write().await;
                state.streaming_position = position;
                debug!(
                    "Updated streaming position to {} for {}",
                    position, self.info_hash
                );
            }
            ProgressiveEvent::Shutdown => {
                // Handled in main loop
            }
        }
        Ok(())
    }

    /// Check if new data has become available
    async fn check_data_availability(&self) -> StreamingResult<()> {
        let state = self.state.read().await;
        let current_pos = state.bytes_written;
        let chunk_size = 1024 * 1024; // 1MB chunks
        let chunk_end = (current_pos + chunk_size).min(state.file_size);

        if current_pos >= state.file_size {
            return Ok(());
        }

        match self
            .data_source
            .check_range_availability(self.info_hash, current_pos..chunk_end)
            .await
        {
            Ok(availability) if availability.available => {
                // Notify that data is available
                self.send_event(ProgressiveEvent::DataAvailable {
                    range: current_pos..chunk_end,
                })?;
            }
            Ok(_) => {
                // Data not available yet, that's fine
            }
            Err(e) => {
                return Err(StreamingError::RemuxingFailed {
                    reason: format!("Failed to check data availability: {e}"),
                });
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use async_trait::async_trait;

    use super::*;
    use crate::storage::{DataResult, RangeAvailability};

    struct MockDataSource {
        file_size: u64,
        available_ranges: Vec<std::ops::Range<u64>>,
    }

    impl MockDataSource {
        fn new(file_size: u64) -> Self {
            Self {
                file_size,
                available_ranges: Vec::new(),
            }
        }

        #[allow(dead_code)]
        fn make_range_available(&mut self, range: std::ops::Range<u64>) {
            self.available_ranges.push(range);
        }
    }

    #[async_trait]
    impl DataSource for MockDataSource {
        async fn read_range(
            &self,
            _info_hash: InfoHash,
            range: std::ops::Range<u64>,
        ) -> DataResult<Vec<u8>> {
            Ok(vec![0u8; (range.end - range.start) as usize])
        }

        async fn file_size(&self, _info_hash: InfoHash) -> DataResult<u64> {
            Ok(self.file_size)
        }

        async fn check_range_availability(
            &self,
            _info_hash: InfoHash,
            range: std::ops::Range<u64>,
        ) -> DataResult<RangeAvailability> {
            let available = self
                .available_ranges
                .iter()
                .any(|r| r.start <= range.start && r.end >= range.end);
            Ok(RangeAvailability {
                available,
                missing_pieces: vec![],
                cache_hit: false,
            })
        }

        fn source_type(&self) -> &'static str {
            "mock"
        }

        async fn can_handle(&self, _info_hash: InfoHash) -> bool {
            true
        }
    }

    #[tokio::test]
    async fn test_coordinator_creation() {
        let info_hash = InfoHash::new([1u8; 20]);
        let data_source = Arc::new(MockDataSource::new(1000000));

        let coordinator = ProgressiveCoordinator::new(info_hash, data_source, 1000000);
        let state = coordinator.state().await;

        assert_eq!(state.bytes_written, 0);
        assert_eq!(state.file_size, 1000000);
        assert!(!state.ffmpeg_running);
    }

    #[tokio::test]
    async fn test_event_handling() {
        let info_hash = InfoHash::new([1u8; 20]);
        let data_source = Arc::new(MockDataSource::new(1000000));

        let coordinator = ProgressiveCoordinator::new(info_hash, data_source, 1000000);

        // Test data written event
        coordinator
            .send_event(ProgressiveEvent::DataWritten { bytes: 1024 })
            .unwrap();

        let _state = coordinator.state().await;
        // Note: We'd need to run the coordinator loop to see the effect
        // This test demonstrates the testable structure
    }
}
