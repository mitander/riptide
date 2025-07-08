//! HTTP streaming service integrating FileAssembler and TranscodingService
//!
//! Provides a unified interface for serving video content with range requests,
//! adaptive bitrate selection, and intelligent prefetching.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::body::Body;
use axum::http::{HeaderMap, HeaderValue, Response, StatusCode};
use axum::response::IntoResponse;
use riptide_core::streaming::{FileAssembler, FileAssemblerError};
use riptide_core::torrent::InfoHash;
use riptide_core::transcoding::{
    JobPriority, SegmentKey, TranscodingService, VideoFormat, VideoQuality,
};
use tokio::sync::RwLock;
use tracing::{error, info};

/// Simple range request for streaming
#[derive(Debug, Clone, Default)]
pub struct SimpleRangeRequest {
    pub start: u64,
    pub end: Option<u64>,
}

/// HTTP streaming service that coordinates FileAssembler and TranscodingService
pub struct HttpStreamingService {
    file_assembler: Arc<dyn FileAssembler>,
    transcoding_service: Arc<TranscodingService>,
    sessions: Arc<RwLock<HashMap<InfoHash, StreamingSession>>>,
    config: HttpStreamingConfig,
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
}

/// Client capabilities for format negotiation
#[derive(Debug, Clone)]
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
    pub body: Body,
    pub content_type: String,
}

/// Streaming service errors
#[derive(Debug, thiserror::Error)]
pub enum HttpStreamingError {
    #[error("File assembler error: {0}")]
    FileAssembler(#[from] FileAssemblerError),

    #[error("Transcoding failed: {reason}")]
    TranscodingFailed { reason: String },

    #[error("Session not found for info hash: {info_hash}")]
    SessionNotFound { info_hash: InfoHash },

    #[error("Invalid range request: {reason}")]
    InvalidRange { reason: String },

    #[error("Unsupported format: {format}")]
    UnsupportedFormat { format: String },

    #[error("Service overloaded: {current_streams} active streams")]
    ServiceOverloaded { current_streams: usize },
}

impl IntoResponse for HttpStreamingError {
    fn into_response(self) -> Response<Body> {
        let (status, message) = match self {
            HttpStreamingError::FileAssembler(_) => {
                (StatusCode::INTERNAL_SERVER_ERROR, "File assembly failed")
            }
            HttpStreamingError::TranscodingFailed { .. } => {
                (StatusCode::INTERNAL_SERVER_ERROR, "Transcoding failed")
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
        };

        (status, message).into_response()
    }
}

impl HttpStreamingService {
    /// Create new HTTP streaming service
    pub fn new(
        file_assembler: Arc<dyn FileAssembler>,
        transcoding_service: Arc<TranscodingService>,
        config: HttpStreamingConfig,
    ) -> Self {
        Self {
            file_assembler,
            transcoding_service,
            sessions: Arc::new(RwLock::new(HashMap::new())),
            config,
        }
    }

    /// Create service with default configuration
    pub fn new_default(
        file_assembler: Arc<dyn FileAssembler>,
        transcoding_service: Arc<TranscodingService>,
    ) -> Self {
        Self::new(
            file_assembler,
            transcoding_service,
            HttpStreamingConfig::default(),
        )
    }

    /// Handle streaming request with range support
    pub async fn handle_streaming_request(
        &self,
        request: StreamingRequest,
    ) -> Result<StreamingResponse, HttpStreamingError> {
        // Check service capacity
        let session_count = self.sessions.read().await.len();
        if session_count >= self.config.max_concurrent_streams {
            return Err(HttpStreamingError::ServiceOverloaded {
                current_streams: session_count,
            });
        }

        // Get or create streaming session
        let session = self.get_or_create_session(request.clone()).await?;

        // Update session with current request
        self.update_session_state(&request, &session).await?;

        // Determine optimal streaming strategy
        let streaming_strategy = self
            .determine_streaming_strategy(&request, &session)
            .await?;

        match streaming_strategy {
            StreamingStrategy::Direct => self.serve_direct_stream(&request, &session).await,
            StreamingStrategy::Transcoded { quality, format } => {
                self.serve_transcoded_stream(&request, &session, quality, format)
                    .await
            }
            StreamingStrategy::Adaptive => self.serve_adaptive_stream(&request, &session).await,
        }
    }

    /// Get or create streaming session
    async fn get_or_create_session(
        &self,
        request: StreamingRequest,
    ) -> Result<StreamingSession, HttpStreamingError> {
        let mut sessions = self.sessions.write().await;

        if let Some(session) = sessions.get(&request.info_hash) {
            Ok(session.clone())
        } else {
            let session = StreamingSession {
                info_hash: request.info_hash,
                current_position: Duration::ZERO,
                preferred_quality: request.preferred_quality.unwrap_or(VideoQuality::Medium),
                client_capabilities: request.client_capabilities,
                last_request_time: Instant::now(),
                bandwidth_estimate: None,
                active_prefetch_jobs: Vec::new(),
            };

            sessions.insert(request.info_hash, session.clone());
            info!("Created new streaming session for {}", request.info_hash);
            Ok(session)
        }
    }

    /// Update session state with current request
    async fn update_session_state(
        &self,
        request: &StreamingRequest,
        _session: &StreamingSession,
    ) -> Result<(), HttpStreamingError> {
        let mut sessions = self.sessions.write().await;
        if let Some(session) = sessions.get_mut(&request.info_hash) {
            session.last_request_time = Instant::now();

            // Update position if time offset provided
            if let Some(time_offset) = request.time_offset {
                session.current_position = time_offset;
            }

            // Update preferred quality if specified
            if let Some(quality) = request.preferred_quality {
                session.preferred_quality = quality;
            }

            // Estimate bandwidth from range request pattern
            if let Some(range) = &request.range {
                session.bandwidth_estimate =
                    self.estimate_bandwidth(range, &session.last_request_time);
            }
        }

        Ok(())
    }

    /// Determine optimal streaming strategy
    async fn determine_streaming_strategy(
        &self,
        request: &StreamingRequest,
        session: &StreamingSession,
    ) -> Result<StreamingStrategy, HttpStreamingError> {
        // Check if direct streaming is possible
        if self
            .can_stream_directly(&request.info_hash, &session.client_capabilities)
            .await?
        {
            return Ok(StreamingStrategy::Direct);
        }

        // Check if adaptive streaming is enabled and supported
        if self.config.enable_adaptive_streaming && session.client_capabilities.supports_hls {
            return Ok(StreamingStrategy::Adaptive);
        }

        // Fall back to transcoded streaming
        let quality = self.select_quality_for_bandwidth(session.bandwidth_estimate);
        let format = self.select_format_for_client(&session.client_capabilities);

        Ok(StreamingStrategy::Transcoded { quality, format })
    }

    /// Serve direct stream without transcoding
    async fn serve_direct_stream(
        &self,
        request: &StreamingRequest,
        _session: &StreamingSession,
    ) -> Result<StreamingResponse, HttpStreamingError> {
        let default_range = SimpleRangeRequest::default();
        let range = request.range.as_ref().unwrap_or(&default_range);

        // Get file size
        let file_size = self.file_assembler.file_size(request.info_hash).await?;

        // Calculate actual range bounds
        let start = range.start;
        let end = range.end.unwrap_or(file_size - 1).min(file_size - 1);
        let length = end - start + 1;

        // Read data from file assembler with range
        let data = self
            .file_assembler
            .read_range(request.info_hash, start..end + 1)
            .await?;

        // Build HTTP response
        let mut headers = HeaderMap::new();
        headers.insert(
            "Content-Range",
            HeaderValue::from_str(&format!("bytes {start}-{end}/{file_size}")).unwrap(),
        );
        headers.insert(
            "Content-Length",
            HeaderValue::from_str(&length.to_string()).unwrap(),
        );
        headers.insert("Accept-Ranges", HeaderValue::from_static("bytes"));

        Ok(StreamingResponse {
            status: StatusCode::PARTIAL_CONTENT,
            headers,
            body: Body::from(data),
            content_type: "video/mp4".to_string(),
        })
    }

    /// Serve transcoded stream
    async fn serve_transcoded_stream(
        &self,
        request: &StreamingRequest,
        session: &StreamingSession,
        quality: VideoQuality,
        format: VideoFormat,
    ) -> Result<StreamingResponse, HttpStreamingError> {
        // Start prefetching if needed
        self.start_prefetch_jobs(request.info_hash, session.current_position, quality, format)
            .await?;

        // Create segment key for current position
        let segment_key = SegmentKey::new(
            request.info_hash,
            session.current_position,
            self.config.segment_duration,
            quality,
            format,
        );

        // Submit transcoding job with high priority
        let job_result = self
            .transcoding_service
            .transcode_segment(
                segment_key.clone(),
                self.get_input_path(request.info_hash).await?,
                self.get_output_path(&segment_key).await?,
                JobPriority::High,
            )
            .await;

        match job_result {
            Ok(transcoded_segment) => {
                // Read the transcoded file
                let data = tokio::fs::read(&transcoded_segment.output_path)
                    .await
                    .map_err(|e| HttpStreamingError::TranscodingFailed {
                        reason: format!("Failed to read transcoded file: {e}"),
                    })?;

                let mut headers = HeaderMap::new();
                headers.insert(
                    "Content-Type",
                    HeaderValue::from_str(self.get_mime_type(&format)).unwrap(),
                );
                headers.insert(
                    "Content-Length",
                    HeaderValue::from_str(&data.len().to_string()).unwrap(),
                );

                Ok(StreamingResponse {
                    status: StatusCode::OK,
                    headers,
                    body: Body::from(data),
                    content_type: self.get_mime_type(&format).to_string(),
                })
            }
            Err(e) => Err(HttpStreamingError::TranscodingFailed {
                reason: e.to_string(),
            }),
        }
    }

    /// Serve adaptive stream (HLS/DASH)
    async fn serve_adaptive_stream(
        &self,
        request: &StreamingRequest,
        session: &StreamingSession,
    ) -> Result<StreamingResponse, HttpStreamingError> {
        // Generate HLS manifest
        let manifest = self
            .generate_hls_manifest(request.info_hash, session)
            .await?;

        let mut headers = HeaderMap::new();
        headers.insert(
            "Content-Type",
            HeaderValue::from_static("application/vnd.apple.mpegurl"),
        );
        headers.insert(
            "Content-Length",
            HeaderValue::from_str(&manifest.len().to_string()).unwrap(),
        );

        Ok(StreamingResponse {
            status: StatusCode::OK,
            headers,
            body: Body::from(manifest),
            content_type: "application/vnd.apple.mpegurl".to_string(),
        })
    }

    /// Start prefetch jobs for better streaming experience
    async fn start_prefetch_jobs(
        &self,
        info_hash: InfoHash,
        current_position: Duration,
        quality: VideoQuality,
        format: VideoFormat,
    ) -> Result<(), HttpStreamingError> {
        for i in 1..=self.config.prefetch_segments {
            let prefetch_position = current_position + (self.config.segment_duration * i as u32);

            let segment_key = SegmentKey::new(
                info_hash,
                prefetch_position,
                self.config.segment_duration,
                quality,
                format,
            );

            // Submit prefetch job with normal priority
            let _job_result = self
                .transcoding_service
                .transcode_segment(
                    segment_key.clone(),
                    self.get_input_path(info_hash).await?,
                    self.get_output_path(&segment_key).await?,
                    JobPriority::Normal,
                )
                .await;

            // Don't fail if prefetch jobs fail - they're optional
        }

        Ok(())
    }

    /// Check if content can be streamed directly without transcoding
    async fn can_stream_directly(
        &self,
        _info_hash: &InfoHash,
        client_capabilities: &ClientCapabilities,
    ) -> Result<bool, HttpStreamingError> {
        // For now, assume MP4 can be streamed directly if client supports it
        // TODO: Implement proper format detection
        Ok(client_capabilities.supports_mp4)
    }

    /// Select quality based on estimated bandwidth
    fn select_quality_for_bandwidth(&self, bandwidth_estimate: Option<u64>) -> VideoQuality {
        match bandwidth_estimate {
            Some(bw) if bw > 10_000_000 => VideoQuality::High, // >10 Mbps
            Some(bw) if bw > 5_000_000 => VideoQuality::Medium, // >5 Mbps
            Some(bw) if bw > 2_000_000 => VideoQuality::Low,   // >2 Mbps
            _ => VideoQuality::Medium,                         // Default to medium
        }
    }

    /// Select format based on client capabilities
    fn select_format_for_client(&self, client_capabilities: &ClientCapabilities) -> VideoFormat {
        if client_capabilities.supports_mp4 {
            VideoFormat::Mp4
        } else if client_capabilities.supports_webm {
            VideoFormat::WebM
        } else {
            VideoFormat::Mp4 // Default fallback
        }
    }

    /// Estimate bandwidth from request patterns
    fn estimate_bandwidth(
        &self,
        _range: &SimpleRangeRequest,
        _last_request: &Instant,
    ) -> Option<u64> {
        // TODO: Implement bandwidth estimation based on request patterns
        None
    }

    /// Get MIME type for video format
    fn get_mime_type(&self, format: &VideoFormat) -> &'static str {
        match format {
            VideoFormat::Mp4 => "video/mp4",
            VideoFormat::WebM => "video/webm",
            VideoFormat::Hls => "application/vnd.apple.mpegurl",
        }
    }

    /// Generate HLS manifest for adaptive streaming
    async fn generate_hls_manifest(
        &self,
        _info_hash: InfoHash,
        _session: &StreamingSession,
    ) -> Result<String, HttpStreamingError> {
        // TODO: Implement HLS manifest generation
        Ok("#EXTM3U\n#EXT-X-VERSION:3\n#EXT-X-TARGETDURATION:6\n#EXT-X-ENDLIST\n".to_string())
    }

    /// Get input path for transcoding
    async fn get_input_path(
        &self,
        _info_hash: InfoHash,
    ) -> Result<std::path::PathBuf, HttpStreamingError> {
        // TODO: Implement input path resolution
        Ok(std::path::PathBuf::from("/tmp/input.mp4"))
    }

    /// Get output path for transcoded segment
    async fn get_output_path(
        &self,
        segment_key: &SegmentKey,
    ) -> Result<std::path::PathBuf, HttpStreamingError> {
        // TODO: Implement output path generation
        Ok(std::path::PathBuf::from(format!(
            "/tmp/output_{segment_key:?}.mp4"
        )))
    }

    /// Get service statistics
    pub async fn statistics(&self) -> HttpStreamingStats {
        let sessions = self.sessions.read().await;
        let transcoding_stats = self
            .transcoding_service
            .statistics()
            .await
            .unwrap_or_else(|_| riptide_core::transcoding::TranscodingStats {
                pool_stats: riptide_core::transcoding::PoolStats {
                    active_workers: 0,
                    idle_workers: 0,
                    queue_length: 0,
                    completed_jobs: 0,
                    failed_jobs: 0,
                    average_processing_time: Some(std::time::Duration::ZERO),
                },
                enabled_formats: vec![],
                quality_levels: vec![],
                adaptive_streaming_enabled: false,
            });

        HttpStreamingStats {
            active_sessions: sessions.len(),
            max_concurrent_streams: self.config.max_concurrent_streams,
            total_transcoding_jobs: transcoding_stats.pool_stats.completed_jobs,
            active_transcoding_jobs: transcoding_stats.pool_stats.active_workers,
            adaptive_streaming_enabled: self.config.enable_adaptive_streaming,
        }
    }

    /// Cleanup inactive sessions
    pub async fn cleanup_inactive_sessions(&self, timeout: Duration) {
        let mut sessions = self.sessions.write().await;
        let now = Instant::now();

        sessions.retain(|info_hash, session| {
            let is_active = now.duration_since(session.last_request_time) < timeout;
            if !is_active {
                info!("Removing inactive session for {}", info_hash);
            }
            is_active
        });
    }
}

/// Streaming strategy selection
#[derive(Debug, Clone)]
enum StreamingStrategy {
    /// Direct streaming without transcoding
    Direct,
    /// Transcoded streaming with specified quality/format
    Transcoded {
        quality: VideoQuality,
        format: VideoFormat,
    },
    /// Adaptive streaming (HLS/DASH)
    Adaptive,
}

/// HTTP streaming service statistics
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct HttpStreamingStats {
    pub active_sessions: usize,
    pub max_concurrent_streams: usize,
    pub total_transcoding_jobs: u64,
    pub active_transcoding_jobs: usize,
    pub adaptive_streaming_enabled: bool,
}

/// Default implementations for testing
impl Default for ClientCapabilities {
    fn default() -> Self {
        Self {
            supports_mp4: true,
            supports_webm: false,
            supports_hls: false,
            user_agent: "Unknown".to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use async_trait::async_trait;

    use super::*;

    // Mock implementations for testing
    struct MockFileAssembler;

    #[async_trait]
    impl FileAssembler for MockFileAssembler {
        async fn read_range(
            &self,
            _info_hash: InfoHash,
            _range: std::ops::Range<u64>,
        ) -> Result<Vec<u8>, FileAssemblerError> {
            Ok(vec![0u8; 1024])
        }

        async fn file_size(&self, _info_hash: InfoHash) -> Result<u64, FileAssemblerError> {
            Ok(1024 * 1024)
        }

        fn is_range_available(&self, _info_hash: InfoHash, _range: std::ops::Range<u64>) -> bool {
            true
        }
    }

    fn create_test_service() -> HttpStreamingService {
        let file_assembler = Arc::new(MockFileAssembler);
        let transcoding_service = Arc::new(TranscodingService::new_default());
        HttpStreamingService::new_default(file_assembler, transcoding_service)
    }

    #[tokio::test]
    async fn test_service_creation() {
        let service = create_test_service();
        let stats = service.statistics().await;
        assert_eq!(stats.active_sessions, 0);
        assert!(stats.adaptive_streaming_enabled);
    }

    #[tokio::test]
    async fn test_session_creation() {
        let service = create_test_service();
        let info_hash = InfoHash::new([1u8; 20]);

        let request = StreamingRequest {
            info_hash,
            range: None,
            client_capabilities: ClientCapabilities::default(),
            preferred_quality: Some(VideoQuality::High),
            time_offset: None,
        };

        let session = service.get_or_create_session(request).await.unwrap();
        assert_eq!(session.info_hash, info_hash);
        assert_eq!(session.preferred_quality, VideoQuality::High);
    }

    #[tokio::test]
    async fn test_quality_selection() {
        let service = create_test_service();

        // Test bandwidth-based quality selection
        assert_eq!(
            service.select_quality_for_bandwidth(Some(15_000_000)),
            VideoQuality::High
        );
        assert_eq!(
            service.select_quality_for_bandwidth(Some(7_000_000)),
            VideoQuality::Medium
        );
        assert_eq!(
            service.select_quality_for_bandwidth(Some(3_000_000)),
            VideoQuality::Low
        );
        assert_eq!(
            service.select_quality_for_bandwidth(None),
            VideoQuality::Medium
        );
    }

    #[tokio::test]
    async fn test_format_selection() {
        let service = create_test_service();

        let mp4_client = ClientCapabilities {
            supports_mp4: true,
            supports_webm: false,
            supports_hls: false,
            user_agent: "Chrome".to_string(),
        };

        let webm_client = ClientCapabilities {
            supports_mp4: false,
            supports_webm: true,
            supports_hls: false,
            user_agent: "Firefox".to_string(),
        };

        assert_eq!(
            service.select_format_for_client(&mp4_client),
            VideoFormat::Mp4
        );
        assert_eq!(
            service.select_format_for_client(&webm_client),
            VideoFormat::WebM
        );
    }

    #[tokio::test]
    async fn test_session_cleanup() {
        let service = create_test_service();
        let info_hash = InfoHash::new([1u8; 20]);

        let request = StreamingRequest {
            info_hash,
            range: None,
            client_capabilities: ClientCapabilities::default(),
            preferred_quality: None,
            time_offset: None,
        };

        // Create session
        let _session = service.get_or_create_session(request).await.unwrap();
        assert_eq!(service.statistics().await.active_sessions, 1);

        // Cleanup should remove inactive sessions
        service
            .cleanup_inactive_sessions(Duration::from_millis(1))
            .await;
        tokio::time::sleep(Duration::from_millis(10)).await;
        service
            .cleanup_inactive_sessions(Duration::from_millis(1))
            .await;

        assert_eq!(service.statistics().await.active_sessions, 0);
    }
}
