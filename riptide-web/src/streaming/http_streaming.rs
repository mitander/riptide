//! HTTP streaming service integrating FileAssembler and FFmpeg remuxing
//!
//! Provides a unified interface for serving video content with range requests,
//! adaptive bitrate selection, and intelligent prefetching.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::body::Body;
use axum::http::{HeaderMap, HeaderValue, Response, StatusCode};
use axum::response::IntoResponse;
use riptide_core::streaming::{
    ContainerDetector, ContainerFormat, FfmpegProcessor, FileAssembler, FileAssemblerError,
    ProductionFfmpegProcessor,
};
use riptide_core::torrent::InfoHash;
use riptide_core::video::{VideoFormat, VideoQuality};
use tokio::sync::RwLock;
use tracing::{error, info};

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
    file_assembler: Arc<dyn FileAssembler>,
    #[allow(dead_code)]
    piece_store: Arc<dyn riptide_core::torrent::PieceStore>,
    ffmpeg_processor: Box<dyn FfmpegProcessor>,
    sessions: Arc<RwLock<HashMap<InfoHash, StreamingSession>>>,
    config: HttpStreamingConfig,

    #[allow(dead_code)]
    temp_file_cleanup: Arc<RwLock<Vec<std::path::PathBuf>>>,
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
    pub body: Body,
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
        // Create FFmpeg processor for remuxing
        let ffmpeg_processor = ProductionFfmpegProcessor::new(None);

        Self {
            file_assembler,
            piece_store,
            ffmpeg_processor: Box::new(ffmpeg_processor),
            sessions: Arc::new(RwLock::new(HashMap::new())),
            config,

            temp_file_cleanup: Arc::new(RwLock::new(Vec::new())),
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

    /// Create new streaming service with custom FFmpeg processor (for testing)
    pub fn new_with_ffmpeg<F: FfmpegProcessor + 'static>(
        file_assembler: Arc<dyn FileAssembler>,
        piece_store: Arc<dyn riptide_core::torrent::PieceStore>,
        ffmpeg_processor: F,
        config: HttpStreamingConfig,
    ) -> Self {
        Self {
            file_assembler,
            piece_store,
            ffmpeg_processor: Box::new(ffmpeg_processor),
            sessions: Arc::new(RwLock::new(HashMap::new())),
            config,

            temp_file_cleanup: Arc::new(RwLock::new(Vec::new())),
            performance_metrics: Arc::new(RwLock::new(StreamingPerformanceMetrics::default())),
        }
    }

    /// Handle streaming request with range support
    pub async fn handle_streaming_request(
        &self,
        request: StreamingRequest,
    ) -> Result<StreamingResponse, HttpStreamingError> {
        let start_time = Instant::now();
        let result = self.handle_streaming_request_internal(request).await;
        let _response_time = start_time.elapsed();

        // Skip performance metrics in request path to avoid handler trait issues
        // TODO: Implement background metrics collection

        result
    }

    /// Internal streaming request handler
    async fn handle_streaming_request_internal(
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

        // Ensure streaming session exists
        let session = self.ensure_session(request.clone()).await?;

        // Update session with current request
        self.update_session_state(&request, &session).await?;

        // Determine optimal streaming strategy
        let streaming_strategy = self
            .determine_streaming_strategy(&request, &session)
            .await?;

        match streaming_strategy {
            StreamingStrategy::Direct => {
                tracing::info!("Serving direct stream for {}", request.info_hash);
                // Skip metrics update in request path
                self.serve_direct_stream(&request, &session).await
            }
            StreamingStrategy::Remuxed {
                quality: _,
                format: _,
            } => {
                tracing::info!(
                    "Remuxing {} to MP4 for browser compatibility",
                    request.info_hash
                );
                self.serve_remuxed_stream(&request).await
            }
            StreamingStrategy::Adaptive => {
                tracing::info!("Serving adaptive HLS stream for {}", request.info_hash);
                // Adaptive streaming not yet implemented
                Err(HttpStreamingError::UnsupportedFormat {
                    format: "HLS".to_string(),
                })
            }
        }
    }

    /// Ensure streaming session exists
    async fn ensure_session(
        &self,
        request: StreamingRequest,
    ) -> Result<StreamingSession, HttpStreamingError> {
        let mut sessions = self.sessions.write().await;

        if let Some(session) = sessions.get(&request.info_hash) {
            Ok(session.clone())
        } else {
            let session = StreamingSession {
                info_hash: request.info_hash,
                current_position: request.time_offset.unwrap_or(Duration::ZERO),
                preferred_quality: request.preferred_quality.unwrap_or(VideoQuality::Medium),
                client_capabilities: request.client_capabilities.clone(),
                last_request_time: Instant::now(),
                bandwidth_estimate: None,
                active_prefetch_jobs: Vec::new(),
                bytes_served: 0,
                requests_count: 0,
                average_response_time_ms: 0.0,
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

            // Bandwidth estimation placeholder
            if let Some(_range) = &request.range {
                session.bandwidth_estimate = Some(5_000_000); // 5 Mbps default
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
        // First, detect the source file format
        let file_size = self.file_assembler.file_size(request.info_hash).await?;
        let header_size = 64.min(file_size);
        let header_data = self
            .file_assembler
            .read_range(request.info_hash, 0..header_size)
            .await?;

        let container_format = ContainerDetector::detect_format(&header_data);

        // If source is already MP4 and client supports MP4, use direct streaming
        if matches!(container_format, ContainerFormat::Mp4)
            && session.client_capabilities.supports_mp4
        {
            return Ok(StreamingStrategy::Direct);
        }

        // Check if adaptive streaming is enabled and supported
        if self.config.enable_adaptive_streaming && session.client_capabilities.supports_hls {
            return Ok(StreamingStrategy::Adaptive);
        }

        // For non-MP4 sources, use remuxed streaming if client supports MP4
        if session.client_capabilities.supports_mp4 {
            let quality = VideoQuality::Medium;
            let format = VideoFormat::Mp4;
            return Ok(StreamingStrategy::Remuxed { quality, format });
        }

        // Fall back to direct streaming (client will handle format compatibility)
        Ok(StreamingStrategy::Direct)
    }

    /// Serve direct stream without remuxing
    async fn serve_direct_stream(
        &self,
        request: &StreamingRequest,
        _session: &StreamingSession,
    ) -> Result<StreamingResponse, HttpStreamingError> {
        let file_size = self.file_assembler.file_size(request.info_hash).await?;
        let content_type = "video/mp4".to_string();

        // Handle range request vs full file request
        if let Some(range) = &request.range {
            // Range request - return 206 Partial Content
            let start = range.start;
            let end = range.end.unwrap_or(file_size - 1).min(file_size - 1);
            let length = end - start + 1;

            let data = self
                .file_assembler
                .read_range(request.info_hash, start..end + 1)
                .await?;

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
            headers.insert(
                "Content-Type",
                HeaderValue::from_str(&content_type).unwrap(),
            );

            Ok(StreamingResponse {
                status: StatusCode::PARTIAL_CONTENT,
                headers,
                body: Body::from(data),
                content_type,
            })
        } else {
            // Full file request - serve complete file
            let data = self
                .file_assembler
                .read_range(request.info_hash, 0..file_size)
                .await?;

            let mut headers = HeaderMap::new();
            headers.insert("Accept-Ranges", HeaderValue::from_static("bytes"));
            headers.insert(
                "Content-Length",
                HeaderValue::from_str(&file_size.to_string()).unwrap(),
            );
            headers.insert(
                "Content-Type",
                HeaderValue::from_str(&content_type).unwrap(),
            );

            Ok(StreamingResponse {
                status: StatusCode::OK,
                headers,
                body: Body::from(data),
                content_type,
            })
        }
    }

    /// Serve remuxed stream by creating MP4 from original file
    async fn serve_remuxed_stream(
        &self,
        request: &StreamingRequest,
    ) -> Result<StreamingResponse, HttpStreamingError> {
        // Get cache path for remuxed file
        let cache_dir = std::env::temp_dir().join("riptide-remux-cache");
        std::fs::create_dir_all(&cache_dir).map_err(|e| HttpStreamingError::RemuxFailed {
            reason: format!("Failed to create cache directory: {e}"),
        })?;

        let cache_path = cache_dir.join(format!("{}.mp4", request.info_hash));

        // If cached MP4 doesn't exist, create it
        if !cache_path.exists() {
            // Check if we have sufficient data for remuxing before attempting
            let file_size = self.file_assembler.file_size(request.info_hash).await?;
            let head_size = 1024 * 1024; // First 1MB for headers
            let tail_size = 2 * 1024 * 1024; // Last 2MB for index data

            let head_available = self
                .file_assembler
                .is_range_available(request.info_hash, 0..head_size.min(file_size));
            let tail_start = file_size.saturating_sub(tail_size);
            let tail_available = self
                .file_assembler
                .is_range_available(request.info_hash, tail_start..file_size);

            if !head_available || !tail_available {
                return Err(HttpStreamingError::StreamingNotReady {
                    reason: "File is being prepared for streaming. Head and tail data not yet available.".to_string(),
                });
            }

            self.create_remuxed_file(request.info_hash, &cache_path)
                .await?;
        }

        // Get file size from cached MP4
        let file_size = std::fs::metadata(&cache_path)
            .map_err(|e| HttpStreamingError::RemuxFailed {
                reason: format!("Failed to get cached file metadata: {e}"),
            })?
            .len();

        // Handle range request vs full file request
        if let Some(range) = &request.range {
            // Range request - return 206 Partial Content
            let start = range.start;
            let end = range.end.unwrap_or(file_size - 1).min(file_size - 1);
            let length = end - start + 1;

            let data = self.read_file_range(&cache_path, start..end + 1).await?;

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
            headers.insert("Content-Type", HeaderValue::from_str("video/mp4").unwrap());

            Ok(StreamingResponse {
                status: StatusCode::PARTIAL_CONTENT,
                headers,
                body: Body::from(data),
                content_type: "video/mp4".to_string(),
            })
        } else {
            // Full file request - serve complete remuxed file
            let data = tokio::fs::read(&cache_path).await.map_err(|e| {
                HttpStreamingError::RemuxFailed {
                    reason: format!("Failed to read cached file: {e}"),
                }
            })?;

            let mut headers = HeaderMap::new();
            headers.insert("Accept-Ranges", HeaderValue::from_static("bytes"));
            headers.insert(
                "Content-Length",
                HeaderValue::from_str(&file_size.to_string()).unwrap(),
            );
            headers.insert("Content-Type", HeaderValue::from_static("video/mp4"));

            Ok(StreamingResponse {
                status: StatusCode::OK,
                headers,
                body: Body::from(data),
                content_type: "video/mp4".to_string(),
            })
        }
    }

    /// Create remuxed MP4 file from original torrent data
    async fn create_remuxed_file(
        &self,
        info_hash: InfoHash,
        output_path: &std::path::Path,
    ) -> Result<(), HttpStreamingError> {
        // Check how much of the file we have available
        let file_size = self.file_assembler.file_size(info_hash).await?;

        // For streaming remuxing, we need head and tail data to create a valid MP4
        // This allows us to start remuxing while the file is still downloading
        let head_size = 1024 * 1024; // First 1MB for headers
        let tail_size = 2 * 1024 * 1024; // Last 2MB for index data

        tracing::info!(
            "Attempting to remux streaming file: {}MB (checking head/tail availability)",
            file_size / (1024 * 1024)
        );

        // Check if we have head and tail data available
        let head_available = self
            .file_assembler
            .is_range_available(info_hash, 0..head_size.min(file_size));
        let tail_start = file_size.saturating_sub(tail_size);
        let tail_available = self
            .file_assembler
            .is_range_available(info_hash, tail_start..file_size);

        if !head_available || !tail_available {
            return Err(HttpStreamingError::RemuxFailed {
                reason: "Head and tail data not yet available for remuxing".to_string(),
            });
        }

        // For streaming remuxing, we'll attempt to get available data
        // If complete file isn't available, we'll try with head + tail + available middle
        let original_data = match self
            .file_assembler
            .read_range(info_hash, 0..file_size)
            .await
        {
            Ok(data) => data,
            Err(_) => {
                // If complete file isn't available, try to create a sparse representation
                // Read head data
                let head_data = self
                    .file_assembler
                    .read_range(info_hash, 0..head_size.min(file_size))
                    .await
                    .map_err(|e| HttpStreamingError::RemuxFailed {
                        reason: format!("Failed to read head data: {e}"),
                    })?;

                // Read tail data
                let tail_data = self
                    .file_assembler
                    .read_range(info_hash, tail_start..file_size)
                    .await
                    .map_err(|e| HttpStreamingError::RemuxFailed {
                        reason: format!("Failed to read tail data: {e}"),
                    })?;

                // Try to read available middle data
                let middle_start = head_size.min(file_size);
                let middle_end = tail_start.max(middle_start);

                if middle_end > middle_start {
                    // Try to get available middle data, but don't fail if we can't
                    let middle_data = self
                        .file_assembler
                        .read_range(info_hash, middle_start..middle_end)
                        .await
                        .unwrap_or_else(|_| vec![0u8; (middle_end - middle_start) as usize]);

                    // Combine head + middle + tail
                    let mut combined_data = head_data;
                    combined_data.extend(middle_data);
                    combined_data.extend(tail_data);
                    combined_data
                } else {
                    // Just combine head + tail
                    let mut combined_data = head_data;
                    combined_data.extend(tail_data);
                    combined_data
                }
            }
        };

        // Detect container format
        let header_data = &original_data[..64.min(original_data.len())];
        let container_format = ContainerDetector::detect_format(header_data);

        let input_extension = match container_format {
            ContainerFormat::Avi => "avi",
            ContainerFormat::Mkv => "mkv",
            ContainerFormat::Mov => "mov",
            _ => "bin",
        };

        // Write available data to temp file for remuxing
        let temp_dir = std::env::temp_dir();
        let input_path =
            temp_dir.join(format!("riptide_remux_input_{info_hash}.{input_extension}"));

        tokio::fs::write(&input_path, &original_data)
            .await
            .map_err(|e| HttpStreamingError::RemuxFailed {
                reason: format!("Failed to write temp input file: {e}"),
            })?;

        // Remux to MP4 using FFmpeg with stream copy (no re-encoding)
        // Use options that work better with partial/incomplete files
        let remux_options = riptide_core::streaming::RemuxingOptions {
            ignore_index: true,
            allow_partial: true,
            ..Default::default()
        };
        let _result = self
            .ffmpeg_processor
            .remux_to_mp4(&input_path, output_path, &remux_options)
            .await
            .map_err(|e| HttpStreamingError::RemuxFailed {
                reason: format!("FFmpeg remux failed: {e}"),
            })?;

        // Clean up temp input file
        let _ = tokio::fs::remove_file(&input_path).await;

        tracing::info!(
            "Successfully remuxed streaming file to MP4: {}",
            output_path.display()
        );
        Ok(())
    }

    /// Read range from file
    async fn read_file_range(
        &self,
        file_path: &std::path::Path,
        range: std::ops::Range<u64>,
    ) -> Result<Vec<u8>, HttpStreamingError> {
        use tokio::io::{AsyncReadExt, AsyncSeekExt};

        let mut file = tokio::fs::File::open(file_path).await.map_err(|e| {
            HttpStreamingError::RemuxFailed {
                reason: format!("Failed to open cached file: {e}"),
            }
        })?;

        file.seek(std::io::SeekFrom::Start(range.start))
            .await
            .map_err(|e| HttpStreamingError::RemuxFailed {
                reason: format!("Failed to seek in cached file: {e}"),
            })?;

        let length = (range.end - range.start) as usize;
        let mut buffer = vec![0u8; length];
        file.read_exact(&mut buffer)
            .await
            .map_err(|e| HttpStreamingError::RemuxFailed {
                reason: format!("Failed to read from cached file: {e}"),
            })?;

        Ok(buffer)
    }

    /// Get streaming statistics
    pub async fn statistics(&self) -> StreamingPerformanceMetrics {
        self.performance_metrics.read().await.clone()
    }

    /// Clean up inactive sessions older than the specified duration
    pub async fn cleanup_inactive_sessions(&self, max_age: Duration) {
        let cutoff = Instant::now() - max_age;
        self.sessions
            .write()
            .await
            .retain(|_, session| session.last_request_time > cutoff);
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use riptide_core::torrent::InfoHash;
    use riptide_core::video::VideoQuality;

    use super::*;

    // Mock file assembler for testing
    struct MockFileAssembler;

    #[async_trait::async_trait]
    impl riptide_core::streaming::FileAssembler for MockFileAssembler {
        async fn file_size(
            &self,
            _info_hash: InfoHash,
        ) -> Result<u64, riptide_core::streaming::FileAssemblerError> {
            Ok(1024)
        }

        async fn read_range(
            &self,
            _info_hash: InfoHash,
            _range: std::ops::Range<u64>,
        ) -> Result<Vec<u8>, riptide_core::streaming::FileAssemblerError> {
            Ok(vec![0u8; 1024])
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
    async fn test_streaming_session_creation() {
        let service = create_test_service();
        let info_hash = InfoHash::new([1u8; 20]);

        let request = StreamingRequest {
            info_hash,
            range: None,
            client_capabilities: ClientCapabilities::default(),
            preferred_quality: Some(VideoQuality::Medium),
            time_offset: None,
        };

        let session = service.ensure_session(request).await.unwrap();
        assert_eq!(session.info_hash, info_hash);
        assert_eq!(session.preferred_quality, VideoQuality::Medium);
    }

    #[tokio::test]
    async fn test_strategy_selection() {
        let service = create_test_service();

        let mp4_client = ClientCapabilities {
            supports_mp4: true,
            supports_webm: false,
            supports_hls: false,
            user_agent: "Chrome".to_string(),
        };

        let session = StreamingSession {
            info_hash: InfoHash::new([1u8; 20]),
            current_position: Duration::from_secs(0),
            preferred_quality: VideoQuality::Medium,
            client_capabilities: mp4_client,
            last_request_time: Instant::now(),
            bandwidth_estimate: None,
            active_prefetch_jobs: vec![],
            bytes_served: 0,
            requests_count: 0,
            average_response_time_ms: 0.0,
        };

        let request = StreamingRequest {
            info_hash: InfoHash::new([1u8; 20]),
            range: None,
            client_capabilities: session.client_capabilities.clone(),
            preferred_quality: Some(VideoQuality::Medium),
            time_offset: None,
        };

        let strategy = service
            .determine_streaming_strategy(&request, &session)
            .await
            .unwrap();
        match strategy {
            StreamingStrategy::Direct => {
                // Expected for MP4-compatible clients
            }
            _ => panic!("Expected Direct strategy for MP4-compatible client"),
        }
    }
}
