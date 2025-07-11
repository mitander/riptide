//! Remux streaming strategy implementation

use std::sync::Arc;

use async_trait::async_trait;

use super::session_manager::RemuxSessionManager;
use super::types::{StreamData, StreamHandle, StreamReadiness, StreamingStatus};
use crate::storage::DataSource;
use crate::streaming::{ContainerFormat, StrategyError, StreamingResult};
use crate::torrent::InfoHash;

/// Streaming strategy for formats that require remuxing to MP4
pub struct RemuxStreamStrategy {
    session_manager: RemuxSessionManager,
}

impl RemuxStreamStrategy {
    /// Create a new remux streaming strategy
    pub fn new(session_manager: RemuxSessionManager) -> Self {
        Self { session_manager }
    }

    /// Create a new remux streaming strategy with data source
    pub fn with_data_source(data_source: Arc<dyn DataSource>) -> Self {
        let config = super::types::RemuxConfig::default();
        let session_manager = RemuxSessionManager::new(config, data_source);
        Self::new(session_manager)
    }

    /// Access the session manager for cloning
    pub fn session_manager(&self) -> &RemuxSessionManager {
        &self.session_manager
    }
}

impl Clone for RemuxStreamStrategy {
    fn clone(&self) -> Self {
        Self::new(self.session_manager.clone())
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
        self.session_manager.get_or_create_session(info_hash).await
    }

    async fn serve_range(
        &self,
        handle: &StreamHandle,
        range: std::ops::Range<u64>,
    ) -> StreamingResult<StreamData> {
        // Check if remuxing is complete
        let readiness = self
            .session_manager
            .check_readiness(handle.info_hash)
            .await?;

        match readiness {
            StreamReadiness::Ready => {
                // Get the output file path and serve from it
                let output_path = self
                    .session_manager
                    .get_output_path(handle.info_hash)
                    .await?;

                // Read the requested range from the remuxed file
                let file_data = tokio::fs::read(&output_path).await.map_err(|e| {
                    StrategyError::RemuxingFailed {
                        reason: format!("Failed to read remuxed file: {e}"),
                    }
                })?;

                let start = range.start as usize;
                let end = range.end.min(file_data.len() as u64) as usize;

                if start >= file_data.len() {
                    return Err(StrategyError::InvalidRange {
                        range: range.clone(),
                    });
                }

                let data = file_data[start..end].to_vec();

                Ok(StreamData {
                    data,
                    content_type: "video/mp4".to_string(),
                    total_size: Some(file_data.len() as u64),
                    range_start: range.start,
                    range_end: end as u64,
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
        self.session_manager.check_readiness(handle.info_hash).await
    }

    async fn status(&self, handle: &StreamHandle) -> StreamingResult<StreamingStatus> {
        self.session_manager.get_status(handle.info_hash).await
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

    struct MockDataSource {
        files: HashMap<InfoHash, Vec<u8>>,
        available_ranges: HashMap<InfoHash, Vec<std::ops::Range<u64>>>,
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
