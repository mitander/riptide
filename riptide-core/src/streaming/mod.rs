//! Direct streaming service with HTTP range requests
//!
//! Provides media streaming capabilities that integrate with the
//! peer management system for streaming performance.

pub mod ffmpeg;

pub mod mp4_validation;
pub mod performance_tests;

pub mod piece_reader;
pub mod range_handler;
pub mod remux;

use std::collections::HashMap;
use std::sync::Arc;

use axum::http::{HeaderMap, HeaderValue, StatusCode};
pub use ffmpeg::{
    FfmpegProcessor, ProductionFfmpegProcessor, RemuxingOptions, RemuxingResult,
    SimulationFfmpegProcessor,
};
pub use piece_reader::{
    PieceBasedStreamReader, PieceReaderError, create_piece_reader_from_trait_object,
};
pub use range_handler::{ContentInfo, RangeHandler, RangeRequest, RangeResponse};
// Legacy aliases for backward compatibility
pub use remux::RemuxStreamStrategy as RemuxStreamingStrategy;
pub use remux::types::RemuxConfig as RemuxStreamingConfig;
pub use remux::{
    ContainerFormat, DirectStreamStrategy, RemuxError, RemuxProgress, RemuxSessionManager,
    RemuxState, RemuxStreamStrategy, StrategyError, StreamData, StreamHandle, StreamReadiness,
    StreamingError, StreamingResult, StreamingStatus, StreamingStrategy,
};

use crate::config::RiptideConfig;
use crate::storage::DataSource;
use crate::torrent::TorrentEngineHandle;

/// HTTP streaming response with headers and status
#[derive(Debug, Clone)]
pub struct HttpStreamingResponse {
    pub body: Vec<u8>,
    pub status: StatusCode,
    pub headers: HeaderMap,
    pub content_type: String,
}

/// HTTP range request
#[derive(Debug, Clone)]
pub struct HttpRangeRequest {
    pub start: u64,
    pub end: Option<u64>,
}

/// HTTP streaming service coordinator integrating multiple streaming strategies.
///
/// Acts as a lightweight coordinator that delegates to appropriate streaming
/// strategies based on container format detection and handles HTTP protocol concerns.
pub struct HttpStreamingService {
    strategies: HashMap<ContainerFormat, Arc<dyn StreamingStrategy>>,
    session_manager: Arc<RemuxSessionManager>,
    torrent_engine: TorrentEngineHandle,
    data_source: Arc<dyn DataSource>,
}

impl Clone for HttpStreamingService {
    fn clone(&self) -> Self {
        Self {
            strategies: self.strategies.clone(),
            session_manager: self.session_manager.clone(),
            torrent_engine: self.torrent_engine.clone(),
            data_source: self.data_source.clone(),
        }
    }
}

impl HttpStreamingService {
    /// Create new streaming service with default strategies
    pub fn new(
        torrent_engine: TorrentEngineHandle,
        data_source: Arc<dyn DataSource>,
        _config: RiptideConfig,
        ffmpeg_processor: Arc<dyn FfmpegProcessor>,
    ) -> Self {
        let session_manager = RemuxSessionManager::new(
            Default::default(),
            Arc::clone(&data_source),
            ffmpeg_processor,
        );

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
        let remux_strategy = RemuxStreamStrategy::new(session_manager.clone());
        strategies.insert(ContainerFormat::Avi, Arc::new(remux_strategy.clone()));
        strategies.insert(ContainerFormat::Mkv, Arc::new(remux_strategy.clone()));
        strategies.insert(ContainerFormat::Mov, Arc::new(remux_strategy));

        Self {
            strategies,
            session_manager: Arc::new(session_manager),
            torrent_engine,
            data_source,
        }
    }

    /// Handle streaming request by delegating to appropriate strategy
    pub async fn handle_stream_request(
        &self,
        info_hash: crate::torrent::InfoHash,
        range: std::ops::Range<u64>,
    ) -> StreamingResult<StreamData> {
        // Check if there's a completed remux session first
        let is_remuxed = self
            .session_manager
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
    pub async fn check_stream_readiness(
        &self,
        info_hash: crate::torrent::InfoHash,
    ) -> StreamingResult<StreamingStatus> {
        // Check if there's a completed remux session first
        let is_remuxed = self
            .session_manager
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
        if let Ok(readiness) = self.session_manager.check_readiness(info_hash).await
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

        let start = if parts[0].is_empty() {
            // Suffix range: bytes=-500 (last 500 bytes)
            if let Ok(suffix_length) = parts[1].parse::<u64>() {
                file_size.saturating_sub(suffix_length)
            } else {
                return None;
            }
        } else if let Ok(start_pos) = parts[0].parse::<u64>() {
            start_pos
        } else {
            return None;
        };

        let end = if parts[1].is_empty() {
            None // bytes=200- (from 200 to end)
        } else if let Ok(end_pos) = parts[1].parse::<u64>() {
            Some(end_pos)
        } else {
            return None;
        };

        Some(HttpRangeRequest { start, end })
    }

    /// Handle HTTP streaming request with proper headers and status codes
    pub async fn handle_http_request(
        &self,
        info_hash: crate::torrent::InfoHash,
        range_header: Option<&str>,
    ) -> StreamingResult<HttpStreamingResponse> {
        // Check if there's a completed remux session first
        let is_remuxed = self
            .session_manager
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

        // Get file size first
        let total_size = match strategy.serve_range(&handle, 0..1).await {
            Ok(sample) => sample.total_size.unwrap_or(0),
            Err(_) => {
                // If we can't get a sample, try to get size from data source
                self.data_source.file_size(info_hash).await.unwrap_or(0)
            }
        };

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

        // Ensure end doesn't exceed file size and start is valid
        let actual_end = end.min(total_size.saturating_sub(1));
        let actual_start = start.min(total_size.saturating_sub(1));

        // Validate range before serving
        if actual_start >= actual_end {
            return Err(StrategyError::InvalidRange { range: start..end });
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
    pub async fn statistics(&self) -> StreamingServiceStats {
        StreamingServiceStats {
            active_sessions: 0,
            total_bytes_streamed: 0,
            concurrent_remux_sessions: 0,
        }
    }
}

/// Streaming service statistics
#[derive(Debug, Clone, serde::Serialize)]
pub struct StreamingServiceStats {
    pub active_sessions: usize,
    pub total_bytes_streamed: u64,
    pub concurrent_remux_sessions: usize,
}
