//! Direct streaming service with HTTP range requests
//!
//! Provides media streaming capabilities that integrate with the
//! peer management system for streaming performance.

pub mod ffmpeg;

pub mod mp4_validation;
pub mod performance_tests;

pub mod piece_reader;
pub mod progressive;
pub mod range;
pub mod remux;

use std::collections::HashMap;
use std::sync::Arc;

use axum::http::{HeaderMap, HeaderValue, StatusCode};
pub use ffmpeg::{Ffmpeg, ProductionFfmpeg, RemuxingOptions, RemuxingResult, SimulationFfmpeg};
pub use piece_reader::{
    PieceBasedStreamReader, PieceReaderError, create_piece_reader_from_trait_object,
};
pub use range::{ContentInfo, Range, RangeRequest, RangeResponse};
// Legacy aliases for backward compatibility
pub use remux::RemuxStreamStrategy as RemuxStreamingStrategy;
pub use remux::types::RemuxConfig as RemuxStreamingConfig;
pub use remux::{
    ContainerFormat, DirectStreamStrategy, RemuxError, RemuxProgress, RemuxState,
    RemuxStreamStrategy, Remuxer, StrategyError, StreamData, StreamHandle, StreamReadiness,
    StreamingError, StreamingResult, StreamingStatus, StreamingStrategy,
};

use crate::config::RiptideConfig;
use crate::engine::TorrentEngineHandle;
use crate::storage::DataSource;

/// HTTP streaming response with headers and status
#[derive(Debug, Clone)]
pub struct HttpStreamingResponse {
    /// Response body data
    pub body: Vec<u8>,
    /// HTTP status code for the response
    pub status: StatusCode,
    /// HTTP headers to include in the response
    pub headers: HeaderMap,
    /// MIME type of the content being streamed
    pub content_type: String,
}

/// HTTP range request
#[derive(Debug, Clone)]
pub struct HttpRangeRequest {
    /// Starting byte position of the range
    pub start: u64,
    /// Ending byte position of the range (exclusive, None for end of file)
    pub end: Option<u64>,
}

/// HTTP streaming service coordinator integrating multiple streaming strategies.
///
/// Acts as a lightweight coordinator that delegates to appropriate streaming
/// strategies based on container format detection and handles HTTP protocol concerns.
pub struct HttpStreaming {
    /// Map of container formats to their streaming strategies
    strategies: HashMap<ContainerFormat, Arc<dyn StreamingStrategy>>,
    /// Remuxer for converting between media formats
    remuxer: Arc<Remuxer>,
    /// Handle to the torrent engine for piece management
    torrent_engine: TorrentEngineHandle,
    /// Data source for reading torrent piece data
    data_source: Arc<dyn DataSource>,
}

impl Clone for HttpStreaming {
    fn clone(&self) -> Self {
        Self {
            strategies: self.strategies.clone(),
            remuxer: self.remuxer.clone(),
            torrent_engine: self.torrent_engine.clone(),
            data_source: self.data_source.clone(),
        }
    }
}

impl HttpStreaming {
    /// Create new streaming service with default strategies
    pub fn new(
        torrent_engine: TorrentEngineHandle,
        data_source: Arc<dyn DataSource>,
        _config: RiptideConfig,
        ffmpeg: Arc<dyn Ffmpeg>,
    ) -> Self {
        let remuxer = Remuxer::new(Default::default(), Arc::clone(&data_source), ffmpeg);

        let mut strategies: HashMap<ContainerFormat, Arc<dyn StreamingStrategy>> = HashMap::new();

        // Direct streaming strategies
        strategies.insert(
            ContainerFormat::Mp4,
            Arc::new(DirectStreamStrategy::new(Arc::clone(&data_source))),
        );
        strategies.insert(
            ContainerFormat::WebM,
            Arc::new(DirectStreamStrategy::new(Arc::clone(&data_source))),
        );

        // Remux streaming strategies
        let remux_strategy = RemuxStreamStrategy::new(remuxer.clone());
        strategies.insert(ContainerFormat::Avi, Arc::new(remux_strategy.clone()));
        strategies.insert(ContainerFormat::Mkv, Arc::new(remux_strategy.clone()));
        strategies.insert(ContainerFormat::Mov, Arc::new(remux_strategy));

        Self {
            strategies,
            remuxer: Arc::new(remuxer),
            torrent_engine,
            data_source,
        }
    }

    /// Serve streaming data by delegating to appropriate strategy
    ///
    /// # Errors
    ///
    /// - `StreamingError::DataSource` - If data source access fails
    /// - `StreamingError::Strategy` - If streaming strategy cannot handle the request
    pub async fn serve_stream_data(
        &self,
        info_hash: crate::torrent::InfoHash,
        range: std::ops::Range<u64>,
    ) -> StreamingResult<StreamData> {
        // Check if there's a completed remux session first
        let is_remuxed = self
            .remuxer
            .check_readiness(info_hash)
            .await
            .map(|readiness| readiness == StreamReadiness::Ready)
            .unwrap_or(false);

        // Detect container format
        let format = self.detect_container_format(info_hash).await?;

        // Get appropriate strategy based on format and remux status
        let strategy = if is_remuxed {
            // If remuxed, always use remux strategy (even if format is MP4)
            self.strategies
                .get(&ContainerFormat::Avi) // Use any remux strategy entry
                .ok_or_else(|| StrategyError::UnsupportedFormat {
                    format: "remux".to_string(),
                })?
        } else {
            // Use normal strategy selection
            self.strategies
                .get(&format)
                .ok_or_else(|| StrategyError::UnsupportedFormat {
                    format: format!("{format:?}"),
                })?
        };

        // Prepare stream handle
        let handle = strategy.prepare_stream(info_hash, format).await?;

        // Serve the requested range
        strategy.serve_range(&handle, range).await
    }

    /// Check if stream is ready for the given range
    /// Check if a stream is ready for serving
    ///
    /// # Errors
    ///
    /// - `StreamingError::SessionNotFound` - If session access fails
    /// - `StreamingError::ReadinessCheck` - If readiness status cannot be determined
    pub async fn check_stream_readiness(
        &self,
        info_hash: crate::torrent::InfoHash,
    ) -> StreamingResult<StreamingStatus> {
        // Check if there's a completed remux session first
        let is_remuxed = self
            .remuxer
            .check_readiness(info_hash)
            .await
            .map(|readiness| readiness == StreamReadiness::Ready)
            .unwrap_or(false);

        // Detect container format
        let format = self.detect_container_format(info_hash).await?;

        // Get appropriate strategy based on format and remux status
        let strategy = if is_remuxed {
            // If remuxed, always use remux strategy (even if format is MP4)
            self.strategies
                .get(&ContainerFormat::Avi) // Use any remux strategy entry
                .ok_or_else(|| StrategyError::UnsupportedFormat {
                    format: "remux".to_string(),
                })?
        } else {
            // Use normal strategy selection
            self.strategies
                .get(&format)
                .ok_or_else(|| StrategyError::UnsupportedFormat {
                    format: format!("{format:?}"),
                })?
        };

        // Prepare stream handle
        let handle = strategy.prepare_stream(info_hash, format).await?;

        // Trigger readiness check to allow state transitions (e.g., from WaitingForHeadAndTail to Remuxing)
        // Use a small range to check if the beginning of the file is ready
        let _readiness = strategy.is_ready(&handle, 0..1024).await?;

        // Get updated status after potential state transitions
        strategy.status(&handle).await
    }

    /// Detect container format from file headers
    async fn detect_container_format(
        &self,
        info_hash: crate::torrent::InfoHash,
    ) -> StreamingResult<ContainerFormat> {
        // Check if there's a completed remux session first
        // If remuxing is complete, the output is always MP4
        if let Ok(readiness) = self.remuxer.check_readiness(info_hash).await
            && readiness == StreamReadiness::Ready
        {
            tracing::debug!("Remux session completed for {}, serving as MP4", info_hash);
            return Ok(ContainerFormat::Mp4);
        }

        // Read first few bytes to detect original format
        const HEADER_SIZE: u64 = 32;
        let header_data = self
            .data_source
            .read_range(info_hash, 0..HEADER_SIZE)
            .await
            .map_err(|_| StrategyError::FormatDetectionFailed)?;

        tracing::debug!(
            "Format detection for {}: header_data={:?}",
            info_hash,
            &header_data[..header_data.len().min(16)]
        );

        // Simple format detection
        let format = if header_data.starts_with(b"ftyp") || header_data[4..8] == *b"ftyp" {
            ContainerFormat::Mp4
        } else if header_data.starts_with(b"\x1A\x45\xDF\xA3")
            || header_data.starts_with(b"\x1a\x45\xdf\xa3")
        {
            // Both MKV and WebM use the same EBML header
            ContainerFormat::Mkv
        } else if header_data.starts_with(b"RIFF") && header_data[8..12] == *b"AVI " {
            ContainerFormat::Avi
        } else {
            ContainerFormat::Unknown
        };

        tracing::debug!("Format detected for {}: {:?}", info_hash, format);

        Ok(format)
    }

    /// Parse HTTP Range header
    pub fn parse_range_header(range_header: &str, file_size: u64) -> Option<HttpRangeRequest> {
        if !range_header.starts_with("bytes=") {
            return None;
        }

        let range_spec = &range_header[6..]; // Remove "bytes="
        let parts: Vec<&str> = range_spec.split('-').collect();

        if parts.len() != 2 {
            return None;
        }

        let (start, end) = if parts[0].is_empty() {
            // Suffix range: bytes=-500 (last 500 bytes)
            if let Ok(suffix_length) = parts[1].parse::<u64>() {
                let start = file_size.saturating_sub(suffix_length);
                (start, None) // No explicit end for suffix ranges
            } else {
                return None;
            }
        } else if let Ok(start_pos) = parts[0].parse::<u64>() {
            let end = if parts[1].is_empty() {
                None // bytes=200- (from 200 to end)
            } else if let Ok(end_pos) = parts[1].parse::<u64>() {
                Some(end_pos)
            } else {
                return None;
            };
            (start_pos, end)
        } else {
            return None;
        };

        Some(HttpRangeRequest { start, end })
    }

    /// Handle HTTP streaming request with proper headers and status codes
    ///
    /// # Errors
    ///
    /// - `StreamingError::DataRetrieval` - If stream data cannot be retrieved
    /// - `StreamingError::ResponseConstruction` - If HTTP response cannot be constructed
    ///
    /// # Panics
    ///
    /// Panics if content length conversion to string fails (should never happen for valid u64)
    pub async fn serve_http_stream(
        &self,
        info_hash: crate::torrent::InfoHash,
        range_header: Option<&str>,
    ) -> StreamingResult<HttpStreamingResponse> {
        // Check if there's a completed remux session first
        let is_remuxed = self
            .remuxer
            .check_readiness(info_hash)
            .await
            .map(|readiness| readiness == StreamReadiness::Ready)
            .unwrap_or(false);

        // Detect container format
        let format = self.detect_container_format(info_hash).await?;

        // Get appropriate strategy based on format and remux status
        let strategy = if is_remuxed {
            // If remuxed, always use remux strategy (even if format is MP4)
            self.strategies
                .get(&ContainerFormat::Avi) // Use any remux strategy entry
                .ok_or_else(|| StrategyError::UnsupportedFormat {
                    format: "remux".to_string(),
                })?
        } else {
            // Use normal strategy selection
            self.strategies
                .get(&format)
                .ok_or_else(|| StrategyError::UnsupportedFormat {
                    format: format!("{format:?}"),
                })?
        };

        // Prepare stream handle
        let handle = strategy.prepare_stream(info_hash, format).await?;

        // Get file size first - debug which source provides the size
        let total_size = match strategy.serve_range(&handle, 0..1).await {
            Ok(sample) => {
                let strategy_size = sample.total_size.unwrap_or(0);
                tracing::debug!(
                    "File size from strategy sample: {} bytes for {}",
                    strategy_size,
                    info_hash
                );
                strategy_size
            }
            Err(strategy_err) => {
                tracing::debug!(
                    "Strategy sample failed ({}), falling back to data source for {}",
                    strategy_err,
                    info_hash
                );
                let data_source_size = self.data_source.file_size(info_hash).await.unwrap_or(0);
                tracing::debug!(
                    "File size from data source: {} bytes for {}",
                    data_source_size,
                    info_hash
                );
                data_source_size
            }
        };

        tracing::info!(
            "Final file size determined: {} bytes for {}",
            total_size,
            info_hash
        );

        // Parse range request
        let (start, end, status) = if let Some(range_str) = range_header {
            if let Some(range_req) = Self::parse_range_header(range_str, total_size) {
                let end = range_req.end.unwrap_or(total_size.saturating_sub(1));
                (range_req.start, end, StatusCode::PARTIAL_CONTENT)
            } else {
                (0, total_size.saturating_sub(1), StatusCode::OK)
            }
        } else {
            (0, total_size.saturating_sub(1), StatusCode::OK)
        };

        tracing::debug!(
            "Processing range request: start={}, end={}, total_size={}, range_header={:?}",
            start,
            end,
            total_size,
            range_header
        );

        // Validate range first - return 416 if start is beyond file size
        if start >= total_size && total_size > 0 {
            tracing::warn!(
                "Range not satisfiable: start={} >= total_size={}",
                start,
                total_size
            );
            return Err(StrategyError::RangeNotSatisfiable {
                requested_start: start,
                file_size: total_size,
            });
        }

        // Clamp both start and end position to file boundaries
        let actual_end = end.min(total_size.saturating_sub(1));
        let actual_start = start.min(total_size.saturating_sub(1));

        tracing::debug!(
            "Range calculation: actual_start={}, actual_end={}, range_size={}",
            actual_start,
            actual_end,
            actual_end.saturating_sub(actual_start) + 1
        );

        // Update piece picker position for sequential downloading
        // This ensures pieces are downloaded in streaming order relative to playback position
        if actual_start > 0
            && let Err(e) = self
                .torrent_engine
                .update_streaming_position(info_hash, actual_start)
                .await
        {
            tracing::warn!(
                "Failed to update piece picker position for {}: {}",
                info_hash,
                e
            );
        }

        // Serve the requested range
        let stream_data = strategy
            .serve_range(&handle, actual_start..actual_end + 1)
            .await?;

        // Build HTTP headers
        let mut headers = HeaderMap::new();

        // Content-Length
        headers.insert(
            "content-length",
            HeaderValue::from_str(&stream_data.data.len().to_string()).unwrap(),
        );

        // Accept-Ranges
        headers.insert("accept-ranges", HeaderValue::from_static("bytes"));

        // Content-Range for partial content
        if status == StatusCode::PARTIAL_CONTENT {
            let content_range = format!("bytes {start}-{actual_end}/{total_size}");
            headers.insert(
                "content-range",
                HeaderValue::from_str(&content_range).unwrap(),
            );
        }

        // Cache control for streaming
        headers.insert("cache-control", HeaderValue::from_static("no-cache"));

        Ok(HttpStreamingResponse {
            body: stream_data.data,
            status,
            headers,
            content_type: stream_data.content_type,
        })
    }

    /// Get streaming statistics
    pub async fn statistics(&self) -> StreamingStats {
        StreamingStats {
            active_sessions: 0,
            total_bytes_streamed: 0,
            concurrent_remux_sessions: 0,
        }
    }
}

/// Streaming service statistics
#[derive(Debug, Clone, serde::Serialize)]
pub struct StreamingStats {
    /// Number of currently active streaming sessions
    pub active_sessions: usize,
    /// Total bytes streamed across all sessions
    pub total_bytes_streamed: u64,
    /// Number of concurrent remux operations
    pub concurrent_remux_sessions: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_range_header_suffix_range() {
        let file_size = 1048576; // 1MB

        // Test suffix range: last 100 bytes
        let range = HttpStreaming::parse_range_header("bytes=-100", file_size).unwrap();
        assert_eq!(range.start, 1048476); // file_size - 100
        assert_eq!(range.end, None); // Should be None for suffix ranges

        // Test suffix range: last 1000 bytes
        let range = HttpStreaming::parse_range_header("bytes=-1000", file_size).unwrap();
        assert_eq!(range.start, 1047576); // file_size - 1000
        assert_eq!(range.end, None);

        // Test normal range for comparison
        let range = HttpStreaming::parse_range_header("bytes=100-199", file_size).unwrap();
        assert_eq!(range.start, 100);
        assert_eq!(range.end, Some(199));

        // Test open-ended range
        let range = HttpStreaming::parse_range_header("bytes=100-", file_size).unwrap();
        assert_eq!(range.start, 100);
        assert_eq!(range.end, None);
    }
}
