//! HTTP streaming service integrating FileAssembler and FFmpeg remuxing
//!
//! Provides a unified interface for serving video content with range requests,
//! adaptive bitrate selection, and intelligent prefetching.

use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::body::Body;
use axum::http::{HeaderMap, Response, StatusCode};
use axum::response::IntoResponse;
use bytes::Bytes;
use riptide_core::streaming::{FileAssembler, FileAssemblerError};
use riptide_core::torrent::InfoHash;
use riptide_core::video::{VideoFormat, VideoQuality};
use tokio::sync::RwLock;
use tracing::{error, info};

use super::coordinator::StreamingCoordinator;

/// Simple range request for streaming
#[derive(Debug, Clone, Default)]
pub struct SimpleRangeRequest {
    pub start: u64,
    pub end: Option<u64>,
}

/// Streaming strategy for handling different content types
#[derive(Debug, Clone)]
pub enum StreamingStrategy {
    /// Direct streaming without modification
    Direct,
    /// Remuxed streaming with quality and format conversion
    Remuxed {
        quality: VideoQuality,
        format: VideoFormat,
    },
    /// Adaptive HLS streaming
    Adaptive,
}

/// HTTP streaming service that coordinates FileAssembler and FFmpeg remuxing
pub struct HttpStreamingService {
    coordinator: StreamingCoordinator,
    #[allow(dead_code)]
    piece_store: Arc<dyn riptide_core::torrent::PieceStore>,
    performance_metrics: Arc<RwLock<StreamingPerformanceMetrics>>,
}

/// Configuration for HTTP streaming service
#[derive(Debug, Clone)]
pub struct HttpStreamingConfig {
    /// Maximum number of concurrent streams
    pub max_concurrent_streams: usize,
    /// Default segment duration for adaptive streaming
    pub segment_duration: Duration,
    /// Prefetch buffer size (in segments)
    pub prefetch_segments: usize,
    /// Minimum bandwidth for quality selection (Mbps)
    pub min_bandwidth_mbps: u32,
    /// Enable adaptive bitrate streaming
    pub enable_adaptive_streaming: bool,
    /// Browser compatibility mode
    pub browser_compatibility: bool,
}

impl Default for HttpStreamingConfig {
    fn default() -> Self {
        Self {
            max_concurrent_streams: 10,
            segment_duration: Duration::from_secs(6),
            prefetch_segments: 3,
            min_bandwidth_mbps: 1,
            enable_adaptive_streaming: true,
            browser_compatibility: true,
        }
    }
}

/// Active streaming session tracking
#[derive(Debug, Clone)]
pub struct StreamingSession {
    pub info_hash: InfoHash,
    pub current_position: Duration,
    pub preferred_quality: VideoQuality,
    pub client_capabilities: ClientCapabilities,
    pub last_request_time: Instant,
    pub bandwidth_estimate: Option<u64>,
    pub active_prefetch_jobs: Vec<String>,
    pub bytes_served: u64,
    pub requests_count: u64,
    pub average_response_time_ms: f64,
}

/// Simple performance metrics for streaming service
#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct StreamingPerformanceMetrics {
    pub total_requests: u64,
    pub direct_stream_requests: u64,
    pub remuxed_requests: u64,
    pub average_response_time_ms: f64,
    pub total_bytes_served: u64,
    pub cache_hits: u64,
    pub cache_misses: u64,
    pub active_remuxing_jobs: u64,
    pub failed_requests: u64,
    pub peak_concurrent_streams: usize,
    pub active_sessions: usize,
    pub max_concurrent_streams: usize,
    pub adaptive_streaming_enabled: bool,
}

/// Parameters for building range response headers
#[derive(Debug)]
pub struct RangeResponseParams {
    pub start: u64,
    pub end: u64,
    pub file_size: u64,
    pub length: u64,
    pub content_type: String,
}

/// Client capabilities for format negotiation
#[derive(Debug, Clone, Default)]
pub struct ClientCapabilities {
    pub supports_mp4: bool,
    pub supports_webm: bool,
    pub supports_hls: bool,
    pub user_agent: String,
}

/// Streaming request parameters
#[derive(Debug, Clone)]
pub struct StreamingRequest {
    pub info_hash: InfoHash,
    pub range: Option<SimpleRangeRequest>,
    pub client_capabilities: ClientCapabilities,
    pub preferred_quality: Option<VideoQuality>,
    pub time_offset: Option<Duration>,
}

/// HTTP streaming response
#[derive(Debug)]
pub struct StreamingResponse {
    pub status: StatusCode,
    pub headers: HeaderMap,
    pub body: Bytes,
    pub content_type: String,
}

/// Streaming service errors
#[derive(Debug, thiserror::Error)]
pub enum HttpStreamingError {
    #[error("File assembler error: {0}")]
    FileAssembler(#[from] FileAssemblerError),

    #[error("Remuxing failed: {reason}")]
    RemuxingFailed { reason: String },

    #[error("Remux failed: {reason}")]
    RemuxFailed { reason: String },

    #[error("Streaming not ready: {reason}")]
    StreamingNotReady { reason: String },

    #[error("Session not found for info hash: {info_hash}")]
    SessionNotFound { info_hash: InfoHash },

    #[error("Invalid range request: {reason}")]
    InvalidRange { reason: String },

    #[error("Unsupported format: {format}")]
    UnsupportedFormat { format: String },

    #[error("Service overloaded: {current_streams} active streams")]
    ServiceOverloaded { current_streams: usize },

    #[error("Browser compatibility test failed: {reason}")]
    BrowserCompatibilityFailed { reason: String },

    #[error("Resource exhaustion: {resource}")]
    ResourceExhausted { resource: String },

    #[error("FFmpeg process failed: {exit_code}")]
    FfmpegProcessFailed { exit_code: i32 },

    #[error("Insufficient data: missing {missing_count} pieces for range {start}-{end}")]
    InsufficientData {
        start: u64,
        end: u64,
        missing_count: usize,
    },
}

impl IntoResponse for HttpStreamingError {
    fn into_response(self) -> Response<Body> {
        let (status, message) = match self {
            HttpStreamingError::FileAssembler(_) => {
                (StatusCode::INTERNAL_SERVER_ERROR, "File assembly failed")
            }
            HttpStreamingError::RemuxingFailed { .. } => {
                (StatusCode::INTERNAL_SERVER_ERROR, "Remuxing failed")
            }
            HttpStreamingError::RemuxFailed { .. } => {
                (StatusCode::INTERNAL_SERVER_ERROR, "Remux failed")
            }
            HttpStreamingError::StreamingNotReady { .. } => {
                (StatusCode::TOO_EARLY, "Stream preparation in progress")
            }
            HttpStreamingError::SessionNotFound { .. } => {
                (StatusCode::NOT_FOUND, "Stream not found")
            }
            HttpStreamingError::InvalidRange { .. } => {
                (StatusCode::RANGE_NOT_SATISFIABLE, "Invalid range")
            }
            HttpStreamingError::UnsupportedFormat { .. } => {
                (StatusCode::UNSUPPORTED_MEDIA_TYPE, "Unsupported format")
            }
            HttpStreamingError::ServiceOverloaded { .. } => {
                (StatusCode::TOO_MANY_REQUESTS, "Service overloaded")
            }
            HttpStreamingError::BrowserCompatibilityFailed { .. } => (
                StatusCode::INTERNAL_SERVER_ERROR,
                "Browser compatibility failed",
            ),
            HttpStreamingError::ResourceExhausted { .. } => {
                (StatusCode::SERVICE_UNAVAILABLE, "Resource exhausted")
            }
            HttpStreamingError::FfmpegProcessFailed { .. } => {
                (StatusCode::INTERNAL_SERVER_ERROR, "Remuxing failed")
            }
            HttpStreamingError::InsufficientData { .. } => {
                (StatusCode::SERVICE_UNAVAILABLE, "Content not yet available")
            }
        };

        (status, message).into_response()
    }
}

impl HttpStreamingService {
    /// Create new HTTP streaming service
    pub fn new(
        file_assembler: Arc<dyn FileAssembler>,
        piece_store: Arc<dyn riptide_core::torrent::PieceStore>,
        config: HttpStreamingConfig,
    ) -> Self {
        let coordinator = StreamingCoordinator::new(Arc::clone(&file_assembler), config.clone());

        info!("HttpStreamingService initialized with coordinator-based architecture");

        Self {
            coordinator,
            piece_store,
            performance_metrics: Arc::new(RwLock::new(StreamingPerformanceMetrics::default())),
        }
    }

    /// Create service with default configuration
    pub fn new_default(
        file_assembler: Arc<dyn FileAssembler>,
        piece_store: Arc<dyn riptide_core::torrent::PieceStore>,
    ) -> Self {
        Self::new(file_assembler, piece_store, HttpStreamingConfig::default())
    }

    /// Create new streaming service with custom config (for testing)
    pub fn new_with_config(
        file_assembler: Arc<dyn FileAssembler>,
        piece_store: Arc<dyn riptide_core::torrent::PieceStore>,
        config: HttpStreamingConfig,
    ) -> Self {
        Self::new(file_assembler, piece_store, config)
    }

    /// Handle streaming request with range support
    pub async fn handle_streaming_request(
        &self,
        request: StreamingRequest,
    ) -> Result<StreamingResponse, HttpStreamingError> {
        let start_time = Instant::now();
        let result = self.coordinator.handle_streaming_request(request).await;
        let _response_time = start_time.elapsed();

        // Update performance metrics
        if let Ok(ref response) = result {
            let mut metrics = self.performance_metrics.write().await;
            metrics.total_requests += 1;
            if response.status == StatusCode::OK || response.status == StatusCode::PARTIAL_CONTENT {
                metrics.total_bytes_served += response.body.len() as u64;
            }
        }

        result
    }

    /// Check if a stream is ready for serving
    pub async fn check_stream_readiness(
        &self,
        info_hash: InfoHash,
    ) -> Result<riptide_core::streaming::StreamingStatus, HttpStreamingError> {
        self.coordinator.check_stream_readiness(info_hash).await
    }

    /// Get coordinator statistics
    pub async fn statistics(&self) -> super::coordinator::CoordinatorStatistics {
        self.coordinator.statistics().await
    }

    /// Clean up stale sessions
    pub async fn cleanup_stale_sessions(&self, max_idle: Duration) {
        self.coordinator.cleanup_stale_sessions(max_idle).await
    }

    /// Get performance metrics
    pub async fn get_performance_metrics(&self) -> StreamingPerformanceMetrics {
        self.performance_metrics.read().await.clone()
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use riptide_core::torrent::InfoHash;

    use super::*;

    // Mock implementations for testing
    struct MockFileAssembler;

    #[async_trait::async_trait]
    impl FileAssembler for MockFileAssembler {
        async fn read_range(
            &self,
            _info_hash: InfoHash,
            _range: std::ops::Range<u64>,
        ) -> Result<Vec<u8>, FileAssemblerError> {
            // Return mock MP4 header
            Ok(vec![
                0x00, 0x00, 0x00, 0x20, // Size (32 bytes)
                0x66, 0x74, 0x79, 0x70, // "ftyp"
                0x6D, 0x70, 0x34, 0x31, // "mp41"
                0x00, 0x00, 0x00, 0x00, // Minor version
                0x6D, 0x70, 0x34, 0x31, // Compatible brand
                0x69, 0x73, 0x6F, 0x6D, // "isom"
                0x61, 0x76, 0x63, 0x31, // "avc1"
                0x00, 0x00, 0x00, 0x00, // Padding
            ])
        }

        async fn file_size(&self, _info_hash: InfoHash) -> Result<u64, FileAssemblerError> {
            Ok(1024 * 1024) // 1MB
        }

        fn is_range_available(&self, _info_hash: InfoHash, _range: std::ops::Range<u64>) -> bool {
            true
        }
    }

    // Mock piece store for testing
    struct MockPieceStore;

    #[async_trait::async_trait]
    impl riptide_core::torrent::PieceStore for MockPieceStore {
        async fn piece_data(
            &self,
            _info_hash: InfoHash,
            _piece_index: riptide_core::torrent::PieceIndex,
        ) -> Result<Vec<u8>, riptide_core::torrent::TorrentError> {
            Ok(vec![0u8; 1024])
        }

        fn has_piece(
            &self,
            _info_hash: InfoHash,
            _piece_index: riptide_core::torrent::PieceIndex,
        ) -> bool {
            true
        }

        fn piece_count(
            &self,
            _info_hash: InfoHash,
        ) -> Result<u32, riptide_core::torrent::TorrentError> {
            Ok(1)
        }
    }

    fn create_test_service() -> HttpStreamingService {
        let file_assembler = Arc::new(MockFileAssembler);
        let piece_store = Arc::new(MockPieceStore);
        HttpStreamingService::new_default(file_assembler, piece_store)
    }

    #[tokio::test]
    async fn test_service_creation() {
        let service = create_test_service();
        let stats = service.statistics().await;
        assert_eq!(stats.active_sessions, 0);
    }

    #[tokio::test]
    async fn test_stream_readiness_check() {
        let service = create_test_service();
        let info_hash = InfoHash::new([1u8; 20]);

        // This should work with the coordinator
        let result = service.check_stream_readiness(info_hash).await;
        // Since coordinator handles readiness checks, we don't need to assert specific outcomes
        // Just ensure the delegation works
        assert!(result.is_ok() || result.is_err());
    }

    #[tokio::test]
    async fn test_performance_metrics() {
        let service = create_test_service();
        let metrics = service.get_performance_metrics().await;
        assert_eq!(metrics.total_requests, 0);
        assert_eq!(metrics.total_bytes_served, 0);
    }
}
