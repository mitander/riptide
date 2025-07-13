//! Remux streaming strategy implementation

use std::sync::Arc;

use async_trait::async_trait;

use super::remuxer::Remuxer;
use super::types::{StreamHandle, StreamReadiness, StreamingStatus};
use crate::storage::DataSource;
use crate::streaming::{
    ContainerFormat, SimulationFfmpeg, StrategyError, StreamData, StreamingResult,
};
use crate::torrent::InfoHash;

/// Streaming strategy for formats that require remuxing to MP4
pub struct RemuxStreamStrategy {
    remuxer: Remuxer,
}

impl RemuxStreamStrategy {
    /// Create a new remux streaming strategy
    pub fn new(remuxer: Remuxer) -> Self {
        Self { remuxer }
    }

    /// Create a new remux streaming strategy with data source
    pub fn with_data_source(data_source: Arc<dyn DataSource>) -> Self {
        let config = super::types::RemuxConfig::default();
        let ffmpeg = Arc::new(SimulationFfmpeg::new());
        let remuxer = Remuxer::new(config, data_source, ffmpeg);
        Self::new(remuxer)
    }

    /// Access the session manager for cloning
    pub fn remuxer(&self) -> &Remuxer {
        &self.remuxer
    }
}

impl Clone for RemuxStreamStrategy {
    fn clone(&self) -> Self {
        Self::new(self.remuxer.clone())
    }
}

/// Trait for streaming strategies
#[async_trait]
pub trait StreamingStrategy: Send + Sync {
    /// Check if this strategy can handle the given container format
    fn can_handle(&self, format: ContainerFormat) -> bool;

    /// Prepare streaming for the given request
    async fn prepare_stream(
        &self,
        info_hash: InfoHash,
        format: ContainerFormat,
    ) -> StreamingResult<StreamHandle>;

    /// Serve a byte range from the prepared stream
    async fn serve_range(
        &self,
        handle: &StreamHandle,
        range: std::ops::Range<u64>,
    ) -> StreamingResult<StreamData>;

    /// Check if the stream is ready for the given range
    async fn is_ready(
        &self,
        handle: &StreamHandle,
        range: std::ops::Range<u64>,
    ) -> StreamingResult<StreamReadiness>;

    /// Get current streaming status
    async fn status(&self, handle: &StreamHandle) -> StreamingResult<StreamingStatus>;
}

#[async_trait]
impl StreamingStrategy for RemuxStreamStrategy {
    fn can_handle(&self, format: ContainerFormat) -> bool {
        // Remux strategy handles formats that need conversion to MP4
        matches!(
            format,
            ContainerFormat::Avi | ContainerFormat::Mkv | ContainerFormat::Mov
        )
    }

    async fn prepare_stream(
        &self,
        info_hash: InfoHash,
        _format: ContainerFormat,
    ) -> StreamingResult<StreamHandle> {
        self.remuxer.find_or_create_session(info_hash).await
    }

    async fn serve_range(
        &self,
        handle: &StreamHandle,
        range: std::ops::Range<u64>,
    ) -> StreamingResult<StreamData> {
        // Validate range request first
        if range.start >= range.end {
            return Err(StrategyError::InvalidRange {
                range: range.clone(),
            });
        }

        // Check if remuxing is complete
        let readiness = self.remuxer.check_readiness(handle.info_hash).await?;

        match readiness {
            StreamReadiness::Ready => {
                // Get the output file path and serve from it
                let output_path = self.remuxer.output_path(handle.info_hash).await?;

                // Get original file size from data source (not partial remux file size)
                let original_file_size = self
                    .remuxer
                    .data_source()
                    .file_size(handle.info_hash)
                    .await
                    .map_err(|e| StrategyError::RemuxingFailed {
                        reason: format!("Failed to get original file size: {e}"),
                    })?;

                // Get current remux file size for range validation
                let mut current_remux_size = tokio::fs::metadata(&output_path)
                    .await
                    .map_err(|e| StrategyError::RemuxingFailed {
                        reason: format!("Failed to get remux file metadata: {e}"),
                    })?
                    .len();

                // Check if requested range is beyond current remux progress
                // Wait briefly for remux to catch up instead of immediate failure
                if range.start >= current_remux_size {
                    tracing::debug!(
                        "Range request {}..{} at current remux progress ({}), waiting for more data",
                        range.start,
                        range.end,
                        current_remux_size
                    );

                    // Wait up to 30 seconds for remux to generate more data for progressive streaming
                    let max_wait_ms = 30000;
                    let check_interval_ms = 100;
                    let mut waited_ms = 0;

                    while waited_ms < max_wait_ms {
                        tokio::time::sleep(tokio::time::Duration::from_millis(check_interval_ms))
                            .await;
                        waited_ms += check_interval_ms;

                        // Recheck file size
                        if let Ok(metadata) = tokio::fs::metadata(&output_path).await {
                            let updated_size = metadata.len();
                            if range.start < updated_size {
                                tracing::debug!(
                                    "Remux caught up: size grew from {} to {} bytes after {}ms wait",
                                    current_remux_size,
                                    updated_size,
                                    waited_ms
                                );
                                break;
                            }
                        }
                    }

                    // Final check after waiting
                    let final_size = tokio::fs::metadata(&output_path)
                        .await
                        .map_err(|e| StrategyError::RemuxingFailed {
                            reason: format!("Failed to get final remux file metadata: {e}"),
                        })?
                        .len();

                    if range.start >= final_size {
                        // Check if we have any partial data available
                        if final_size > 0 {
                            // We have some data, but not up to the requested start position
                            // For progressive playback, we should indicate this properly
                            return Err(StrategyError::StreamingNotReady {
                                reason: format!(
                                    "Remux in progress: requested byte {} not yet available after {}ms wait (current size: {})",
                                    range.start, waited_ms, final_size
                                ),
                            });
                        } else {
                            // No data at all yet
                            return Err(StrategyError::StreamingNotReady {
                                reason: "Remux in progress: no data available yet".to_string(),
                            });
                        }
                    }

                    // Update current_remux_size for the next step
                    current_remux_size = final_size;
                }

                // If request is beyond available remux data, use what's available
                let actual_end = range.end.min(current_remux_size);

                // Read only the requested range, not the entire file
                let mut file = tokio::fs::File::open(&output_path).await.map_err(|e| {
                    StrategyError::RemuxingFailed {
                        reason: format!("Failed to open remuxed file: {e}"),
                    }
                })?;

                use tokio::io::{AsyncReadExt, AsyncSeekExt};
                file.seek(std::io::SeekFrom::Start(range.start))
                    .await
                    .map_err(|e| StrategyError::RemuxingFailed {
                        reason: format!("Failed to seek in remuxed file: {e}"),
                    })?;

                let read_size = (actual_end - range.start) as usize;
                let mut file_data = vec![0u8; read_size];
                file.read_exact(&mut file_data).await.map_err(|e| {
                    StrategyError::RemuxingFailed {
                        reason: format!("Failed to read remuxed file range: {e}"),
                    }
                })?;

                // For progressive streaming, if we're still remuxing and serving partial data,
                // we might want to indicate the actual remuxed file size instead of original
                // Check if remuxing is still in progress by comparing sizes
                let is_remuxing = current_remux_size < original_file_size;

                let reported_total_size = if is_remuxing {
                    // During remuxing, report the current size to help players understand
                    // that more data is being produced
                    Some(current_remux_size.max(actual_end))
                } else {
                    // For completed remux, report the original file size for compatibility
                    Some(original_file_size)
                };

                Ok(StreamData {
                    data: file_data,
                    content_type: "video/mp4".to_string(),
                    total_size: reported_total_size,
                    range_start: range.start,
                    range_end: actual_end,
                })
            }
            StreamReadiness::Processing => Err(StrategyError::StreamingNotReady {
                reason: "Remuxing in progress".to_string(),
            }),
            StreamReadiness::WaitingForData => Err(StrategyError::StreamingNotReady {
                reason: "Waiting for sufficient data".to_string(),
            }),
            StreamReadiness::CanRetry => Err(StrategyError::StreamingNotReady {
                reason: "Previous remuxing failed, retry available".to_string(),
            }),
            StreamReadiness::Failed => Err(StrategyError::RemuxingFailed {
                reason: "Remuxing failed permanently".to_string(),
            }),
        }
    }

    async fn is_ready(
        &self,
        handle: &StreamHandle,
        _range: std::ops::Range<u64>,
    ) -> StreamingResult<StreamReadiness> {
        self.remuxer.check_readiness(handle.info_hash).await
    }

    async fn status(&self, handle: &StreamHandle) -> StreamingResult<StreamingStatus> {
        self.remuxer.status(handle.info_hash).await
    }
}

/// Direct streaming strategy for formats that don't need remuxing
pub struct DirectStreamStrategy {
    data_source: Arc<dyn DataSource>,
}

impl DirectStreamStrategy {
    /// Create a new direct streaming strategy
    pub fn new(data_source: Arc<dyn DataSource>) -> Self {
        Self { data_source }
    }
}

#[async_trait]
impl StreamingStrategy for DirectStreamStrategy {
    fn can_handle(&self, format: ContainerFormat) -> bool {
        // Direct strategy handles formats that can stream natively
        matches!(format, ContainerFormat::Mp4 | ContainerFormat::WebM)
    }

    async fn prepare_stream(
        &self,
        info_hash: InfoHash,
        format: ContainerFormat,
    ) -> StreamingResult<StreamHandle> {
        Ok(StreamHandle {
            info_hash,
            session_id: 0, // Direct streaming doesn't need session management
            format,
        })
    }

    async fn serve_range(
        &self,
        handle: &StreamHandle,
        range: std::ops::Range<u64>,
    ) -> StreamingResult<StreamData> {
        // Check if the range is available
        let availability = self
            .data_source
            .check_range_availability(handle.info_hash, range.clone())
            .await
            .map_err(|e| StrategyError::StreamingNotReady {
                reason: format!("Failed to check range availability: {e}"),
            })?;

        if !availability.available {
            return Err(StrategyError::StreamingNotReady {
                reason: "Requested range not available".to_string(),
            });
        }

        // Read the data directly from data source
        let data = self
            .data_source
            .read_range(handle.info_hash, range.clone())
            .await
            .map_err(|e| StrategyError::StreamingNotReady {
                reason: format!("Failed to read data: {e}"),
            })?;

        let file_size = self
            .data_source
            .file_size(handle.info_hash)
            .await
            .map_err(|e| StrategyError::StreamingNotReady {
                reason: format!("Failed to get file size: {e}"),
            })?;

        let content_type = match handle.format {
            ContainerFormat::Mp4 => "video/mp4",
            ContainerFormat::WebM => "video/webm",
            _ => "application/octet-stream",
        };

        Ok(StreamData {
            data,
            content_type: content_type.to_string(),
            total_size: Some(file_size),
            range_start: range.start,
            range_end: range.end,
        })
    }

    async fn is_ready(
        &self,
        handle: &StreamHandle,
        range: std::ops::Range<u64>,
    ) -> StreamingResult<StreamReadiness> {
        let availability = self
            .data_source
            .check_range_availability(handle.info_hash, range)
            .await
            .map_err(|e| StrategyError::StreamingNotReady {
                reason: format!("Failed to check range availability: {e}"),
            })?;

        if availability.available {
            Ok(StreamReadiness::Ready)
        } else {
            Ok(StreamReadiness::WaitingForData)
        }
    }

    async fn status(&self, _handle: &StreamHandle) -> StreamingResult<StreamingStatus> {
        // Direct streaming is always ready if data is available
        Ok(StreamingStatus {
            readiness: StreamReadiness::Ready,
            progress: Some(1.0), // Direct streaming doesn't have progress
            estimated_time_remaining: None,
            error_message: None,
            last_activity: std::time::Instant::now(),
        })
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;
    use crate::storage::{DataError, DataResult, RangeAvailability};

    // Type aliases for complex types
    type FileDataMap = HashMap<InfoHash, Vec<u8>>;
    type RangeMap = HashMap<InfoHash, Vec<std::ops::Range<u64>>>;

    struct MockDataSource {
        files: FileDataMap,
        available_ranges: RangeMap,
    }

    impl MockDataSource {
        fn new() -> Self {
            Self {
                files: HashMap::new(),
                available_ranges: HashMap::new(),
            }
        }

        fn add_file(&mut self, info_hash: InfoHash, data: Vec<u8>) {
            self.files.insert(info_hash, data);
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
            info_hash: InfoHash,
            range: std::ops::Range<u64>,
        ) -> DataResult<Vec<u8>> {
            let data = self
                .files
                .get(&info_hash)
                .ok_or_else(|| DataError::FileNotFound { info_hash })?;

            let start = range.start as usize;
            let end = range.end.min(data.len() as u64) as usize;

            if start >= data.len() {
                return Err(DataError::InsufficientData {
                    start: range.start,
                    end: range.end,
                    missing_count: 1,
                });
            }

            Ok(data[start..end].to_vec())
        }

        async fn file_size(&self, info_hash: InfoHash) -> DataResult<u64> {
            self.files
                .get(&info_hash)
                .map(|data| data.len() as u64)
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
    async fn test_direct_strategy_can_handle() {
        let data_source = Arc::new(MockDataSource::new());
        let strategy = DirectStreamStrategy::new(data_source);

        assert!(strategy.can_handle(ContainerFormat::Mp4));
        assert!(strategy.can_handle(ContainerFormat::WebM));
        assert!(!strategy.can_handle(ContainerFormat::Avi));
        assert!(!strategy.can_handle(ContainerFormat::Mkv));
    }

    #[tokio::test]
    async fn test_remux_strategy_can_handle() {
        let data_source = Arc::new(MockDataSource::new());
        let strategy = RemuxStreamStrategy::with_data_source(data_source);

        assert!(!strategy.can_handle(ContainerFormat::Mp4));
        assert!(!strategy.can_handle(ContainerFormat::WebM));
        assert!(strategy.can_handle(ContainerFormat::Avi));
        assert!(strategy.can_handle(ContainerFormat::Mkv));
    }

    #[tokio::test]
    async fn test_direct_streaming() {
        let mut data_source = MockDataSource::new();
        let info_hash = InfoHash::new([1u8; 20]);
        let test_data = b"test mp4 data".to_vec();

        data_source.add_file(info_hash, test_data.clone());
        data_source.make_range_available(info_hash, 0..test_data.len() as u64);

        let strategy = DirectStreamStrategy::new(Arc::new(data_source));

        let handle = strategy
            .prepare_stream(info_hash, ContainerFormat::Mp4)
            .await
            .unwrap();
        let readiness = strategy.is_ready(&handle, 0..5).await.unwrap();
        assert_eq!(readiness, StreamReadiness::Ready);

        let stream_data = strategy.serve_range(&handle, 0..5).await.unwrap();
        assert_eq!(stream_data.data, b"test ");
        assert_eq!(stream_data.content_type, "video/mp4");
    }
}
