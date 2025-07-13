//! Robust progressive feeder with better error handling and testability

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use tokio::process::Child;
use tokio::sync::oneshot;
use tracing::{debug, error, info, warn};

use super::coordinator::{ProgressiveCoordinator, ProgressiveEvent};
use crate::storage::DataSource;
use crate::streaming::{RemuxingOptions, StreamingError, StreamingResult};
use crate::torrent::InfoHash;

/// Simplified progressive feeder that works with the coordinator
pub struct RobustProgressiveFeeder {
    info_hash: InfoHash,
    coordinator: Arc<ProgressiveCoordinator>,
    #[allow(dead_code)]
    output_path: PathBuf,
    shutdown_tx: Option<oneshot::Sender<()>>,
}

impl RobustProgressiveFeeder {
    /// Create a new robust progressive feeder
    pub fn new(
        info_hash: InfoHash,
        data_source: Arc<dyn DataSource>,
        output_path: PathBuf,
        file_size: u64,
    ) -> Self {
        let coordinator = Arc::new(ProgressiveCoordinator::new(
            info_hash,
            data_source,
            file_size,
        ));

        Self {
            info_hash,
            coordinator,
            output_path,
            shutdown_tx: None,
        }
    }

    /// Start progressive streaming with better coordination
    ///
    /// # Errors
    /// Returns `StreamingError` if FFmpeg process cannot be started
    pub async fn start(&mut self, options: &RemuxingOptions) -> StreamingResult<()> {
        info!(
            "Starting robust progressive streaming for {}",
            self.info_hash
        );

        // Start FFmpeg process
        let ffmpeg_process = self.start_ffmpeg_process(options).await?;

        // Start coordinator
        let coordinator_clone = Arc::clone(&self.coordinator);
        let info_hash = self.info_hash;

        let (shutdown_tx, shutdown_rx) = oneshot::channel();
        self.shutdown_tx = Some(shutdown_tx);

        // Spawn the main feeding task
        tokio::spawn(async move {
            let result = Self::run_progressive_feeding(
                coordinator_clone,
                ffmpeg_process,
                info_hash,
                shutdown_rx,
            )
            .await;

            match result {
                Ok(()) => info!(
                    "Progressive feeding completed successfully for {}",
                    info_hash
                ),
                Err(e) => error!("Progressive feeding failed for {}: {}", info_hash, e),
            }
        });

        Ok(())
    }

    /// Main progressive feeding loop with coordinator
    async fn run_progressive_feeding(
        coordinator: Arc<ProgressiveCoordinator>,
        mut ffmpeg_process: Child,
        info_hash: InfoHash,
        mut shutdown_rx: oneshot::Receiver<()>,
    ) -> StreamingResult<()> {
        debug!("Starting progressive feeding loop for {}", info_hash);

        // Mark FFmpeg as running
        coordinator.send_event(ProgressiveEvent::UpdateStreamingPosition { position: 0 })?;

        let mut feed_interval = tokio::time::interval(Duration::from_millis(100));
        let bytes_written = 0u64;

        loop {
            tokio::select! {
                _ = feed_interval.tick() => {
                    // Check coordinator state
                    let state = coordinator.state().await;

                    if bytes_written >= state.file_size {
                        debug!("All data processed for {}, waiting for FFmpeg completion", info_hash);

                        // Close stdin if still open
                        if ffmpeg_process.stdin.is_some() {
                            debug!("Closing FFmpeg stdin for {}", info_hash);
                            ffmpeg_process.stdin.take();
                        }
                        continue;
                    }

                    // This is where we'd implement the actual data feeding
                    // For now, this is a placeholder that demonstrates the structure
                    debug!("Checking for data to feed to FFmpeg for {}", info_hash);
                }

                _ = &mut shutdown_rx => {
                    info!("Shutdown requested for progressive feeding of {}", info_hash);
                    coordinator.send_event(ProgressiveEvent::Shutdown)?;
                    break;
                }

                status = ffmpeg_process.wait() => {
                    match status {
                        Ok(exit_status) => {
                            coordinator.send_event(ProgressiveEvent::ProcessExited { status: exit_status })?;
                            if exit_status.success() {
                                info!("FFmpeg completed successfully for {}", info_hash);
                            } else {
                                warn!("FFmpeg exited with status {} for {}", exit_status, info_hash);
                            }
                            break;
                        }
                        Err(e) => {
                            error!("Error waiting for FFmpeg process for {}: {}", info_hash, e);
                            return Err(StreamingError::RemuxingFailed {
                                reason: format!("FFmpeg process error: {e}"),
                            });
                        }
                    }
                }
            }
        }

        Ok(())
    }

    /// Start FFmpeg process (simplified version)
    async fn start_ffmpeg_process(&self, _options: &RemuxingOptions) -> StreamingResult<Child> {
        debug!("Starting FFmpeg process for {}", self.info_hash);

        // This would contain the FFmpeg startup logic
        // For now, return a mock process to demonstrate structure
        Err(StreamingError::RemuxingFailed {
            reason: "FFmpeg startup not implemented in refactored version".to_string(),
        })
    }

    /// Shutdown the feeder
    pub fn shutdown(&mut self) {
        if let Some(shutdown_tx) = self.shutdown_tx.take() {
            let _ = shutdown_tx.send(());
        }
    }
}

impl Drop for RobustProgressiveFeeder {
    fn drop(&mut self) {
        self.shutdown();
    }
}

#[cfg(test)]
mod tests {
    use async_trait::async_trait;

    use super::*;
    use crate::storage::{DataResult, RangeAvailability};

    struct MockDataSource;

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
            Ok(1000000)
        }

        async fn check_range_availability(
            &self,
            _info_hash: InfoHash,
            _range: std::ops::Range<u64>,
        ) -> DataResult<RangeAvailability> {
            Ok(RangeAvailability {
                available: true,
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
    async fn test_feeder_creation() {
        let info_hash = InfoHash::new([1u8; 20]);
        let data_source = Arc::new(MockDataSource);
        let output_path = PathBuf::from("/tmp/test.mp4");

        let feeder = RobustProgressiveFeeder::new(info_hash, data_source, output_path, 1000000);

        // Test that feeder was created successfully
        assert_eq!(feeder.info_hash, info_hash);
    }
}
