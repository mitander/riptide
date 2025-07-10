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
    ContainerDetector, ContainerFormat, FileAssembler, FileAssemblerError, RemuxStreamingConfig,
    RemuxStreamingStrategy, StreamingStrategy as CoreStreamingStrategy,
    create_remux_streaming_strategy_with_config,
};
use riptide_core::torrent::InfoHash;
use riptide_core::video::{VideoFormat, VideoQuality};
use tokio::sync::RwLock;
use tracing::{debug, error, info, warn};

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
    remux_streaming: Option<RemuxStreamingStrategy>,
    sessions: Arc<RwLock<HashMap<InfoHash, StreamingSession>>>,
    config: HttpStreamingConfig,
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
        // Create unified remux streaming strategy
        let remux_config = RemuxStreamingConfig {
            min_head_size: 3 * 1024 * 1024, // 3MB head required for reliable MP4 metadata
            ..Default::default()
        };

        let remux_streaming = Some(create_remux_streaming_strategy_with_config(
            Arc::clone(&file_assembler),
            remux_config,
        ));

        let service = Self {
            file_assembler,
            piece_store,
            remux_streaming,
            sessions: Arc::new(RwLock::new(HashMap::new())),
            config,
            performance_metrics: Arc::new(RwLock::new(StreamingPerformanceMetrics::default())),
        };

        info!(
            "HttpStreamingService initialized with remux_streaming: {}",
            service.remux_streaming.is_some()
        );
        if let Some(ref rs) = service.remux_streaming {
            info!(
                "Remux streaming supports AVI: {}, MKV: {}, MP4: {}",
                rs.supports_format(&ContainerFormat::Avi),
                rs.supports_format(&ContainerFormat::Mkv),
                rs.supports_format(&ContainerFormat::Mp4)
            );
        }

        service
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
        // Create unified remux streaming strategy for testing
        let remux_config = RemuxStreamingConfig {
            min_head_size: 3 * 1024 * 1024, // 3MB head required for reliable MP4 metadata
            ..Default::default()
        };

        let remux_streaming = Some(create_remux_streaming_strategy_with_config(
            Arc::clone(&file_assembler),
            remux_config,
        ));

        Self {
            file_assembler,
            piece_store,
            remux_streaming,
            sessions: Arc::new(RwLock::new(HashMap::new())),
            config,
            performance_metrics: Arc::new(RwLock::new(StreamingPerformanceMetrics::default())),
        }
    }

    /// Create new streaming service with custom remux config (for testing)
    pub fn new_with_remux_config(
        file_assembler: Arc<dyn FileAssembler>,
        piece_store: Arc<dyn riptide_core::torrent::PieceStore>,
        config: HttpStreamingConfig,
        remux_config: RemuxStreamingConfig,
    ) -> Self {
        let remux_streaming = Some(create_remux_streaming_strategy_with_config(
            Arc::clone(&file_assembler),
            remux_config,
        ));

        Self {
            file_assembler,
            piece_store,
            remux_streaming,
            sessions: Arc::new(RwLock::new(HashMap::new())),
            config,
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

        // Always use direct streaming for MP4 files (no remuxing needed)
        if matches!(container_format, ContainerFormat::Mp4) {
            return Ok(StreamingStrategy::Direct);
        }

        // Check if adaptive streaming is enabled and supported
        if self.config.enable_adaptive_streaming && session.client_capabilities.supports_hls {
            return Ok(StreamingStrategy::Adaptive);
        }

        // For non-MP4 files, use unified remux streaming if available and client supports MP4
        debug!(
            "Client capabilities for {}: supports_mp4={}, supports_hls={}, supports_webm={}",
            request.info_hash,
            session.client_capabilities.supports_mp4,
            session.client_capabilities.supports_hls,
            session.client_capabilities.supports_webm
        );
        if session.client_capabilities.supports_mp4 {
            // Check if remux streaming is available and can handle this format
            debug!(
                "Checking remux streaming availability for {} (format: {:?})",
                request.info_hash, container_format
            );
            debug!(
                "Remux streaming available: {}, supports format: {}",
                self.remux_streaming.is_some(),
                self.remux_streaming
                    .as_ref()
                    .is_some_and(|rs| rs.supports_format(&container_format))
            );
            if let Some(ref remux_strategy) = self.remux_streaming
                && remux_strategy.supports_format(&container_format)
            {
                info!("Using unified remux streaming for {}", request.info_hash);
                let quality = VideoQuality::Medium;
                let format = VideoFormat::Mp4;
                return Ok(StreamingStrategy::Remuxed { quality, format });
            }

            // No remux streaming available
            error!(
                "Remux streaming not available for {} (format: {:?}). Remux streaming present: {}, format supported: {}",
                request.info_hash,
                container_format,
                self.remux_streaming.is_some(),
                self.remux_streaming
                    .as_ref()
                    .is_some_and(|rs| rs.supports_format(&container_format))
            );
            return Err(HttpStreamingError::RemuxingFailed {
                reason: format!("Remux streaming not available for format {container_format:?}"),
            });
        }

        // Never fall back to direct streaming for non-MP4 files - this causes MIME type issues
        Err(HttpStreamingError::RemuxingFailed {
            reason: format!(
                "Client does not support MP4 and remux streaming is required for format {container_format:?}"
            ),
        })
    }

    /// Serve direct stream without remuxing
    async fn serve_direct_stream(
        &self,
        request: &StreamingRequest,
        _session: &StreamingSession,
    ) -> Result<StreamingResponse, HttpStreamingError> {
        info!("serve_direct_stream called for {}", request.info_hash);

        let file_size = self.file_assembler.file_size(request.info_hash).await?;

        // Detect actual container format for safety check
        let header_size = 64.min(file_size);
        let header_data = self
            .file_assembler
            .read_range(request.info_hash, 0..header_size)
            .await?;
        let container_format = ContainerDetector::detect_format(&header_data);

        // Safety check: Never serve non-MP4 files directly (prevents MIME type flashing)
        if !matches!(container_format, ContainerFormat::Mp4) {
            error!(
                "CRITICAL: Attempted to serve non-MP4 file {} directly! Format: {:?}",
                request.info_hash, container_format
            );
            return Err(HttpStreamingError::RemuxingFailed {
                reason: format!(
                    "Cannot serve {container_format:?} file directly - remuxing required but not available"
                ),
            });
        }

        let content_type = container_format.mime_type().to_string();
        info!(
            "Direct streaming MP4 file {} (format: {:?}, content-type: {})",
            request.info_hash, container_format, content_type
        );

        // Handle range request vs full file request
        if let Some(range) = &request.range {
            // Range request - return 206 Partial Content
            let start = range.start;
            // Handle empty file case
            if file_size == 0 {
                return Err(HttpStreamingError::StreamingNotReady {
                    reason: "Cannot serve range from empty file".to_string(),
                });
            }

            let end = range.end.unwrap_or(file_size - 1).min(file_size - 1);

            // Validate range to prevent overflow
            if start >= file_size || end < start {
                return Err(HttpStreamingError::StreamingNotReady {
                    reason: format!(
                        "Invalid range: start={start}, end={end}, file_size={file_size}"
                    ),
                });
            }
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

    /// Serve remuxed stream using progressive remuxing for streaming while downloading
    async fn serve_remuxed_stream(
        &self,
        request: &StreamingRequest,
    ) -> Result<StreamingResponse, HttpStreamingError> {
        // First detect the container format to decide if remuxing is needed
        let format = self.detect_container_format(request.info_hash).await?;

        // Only remux non-MP4 files - serve MP4 files directly
        match format {
            ContainerFormat::Mp4 => {
                info!("File is already MP4, serving directly");
                // For direct streaming, create a temporary session
                let session = StreamingSession {
                    info_hash: request.info_hash,
                    current_position: Duration::from_secs(0),
                    preferred_quality: request.preferred_quality.unwrap_or(VideoQuality::High),
                    client_capabilities: request.client_capabilities.clone(),
                    last_request_time: std::time::Instant::now(),
                    bandwidth_estimate: None,
                    active_prefetch_jobs: Vec::new(),
                    bytes_served: 0,
                    requests_count: 0,
                    average_response_time_ms: 0.0,
                };
                self.serve_direct_stream(request, &session).await
            }
            ContainerFormat::Avi | ContainerFormat::Mkv => {
                info!("File is {format:?}, remuxing to MP4 for browser compatibility");

                // Try remux streaming for non-MP4 files
                if let Some(ref remux_streaming) = self.remux_streaming {
                    match self
                        .serve_progressive_remuxed_stream(remux_streaming, request)
                        .await
                    {
                        Ok(response) => return Ok(response),
                        Err(e) => {
                            warn!("Remux streaming failed: {}", e);
                            // Return 503 and let browser retry - never serve AVI/MKV directly
                            let mut headers = HeaderMap::new();
                            headers.insert("Retry-After", HeaderValue::from_static("5"));
                            headers.insert("Content-Type", HeaderValue::from_static("text/plain"));

                            return Ok(StreamingResponse {
                                status: StatusCode::SERVICE_UNAVAILABLE,
                                headers,
                                body: Body::from(
                                    "Video is being processed for browser compatibility. Please wait...",
                                ),
                                content_type: "text/plain".to_string(),
                            });
                        }
                    }
                }

                // No remux streaming available
                Err(HttpStreamingError::StreamingNotReady {
                    reason: "Remux streaming not available for this file format".to_string(),
                })
            }
            ContainerFormat::WebM | ContainerFormat::Mov => {
                info!("File is {format:?}, remuxing to MP4 for browser compatibility");
                // Handle WebM and MOV like other non-MP4 formats
                Err(HttpStreamingError::StreamingNotReady {
                    reason: "Format not yet supported for remux streaming".to_string(),
                })
            }
            ContainerFormat::Unknown => Err(HttpStreamingError::StreamingNotReady {
                reason: "Unknown file format, cannot stream".to_string(),
            }),
        }
    }

    /// Serve stream using remux streaming strategy
    async fn serve_progressive_remuxed_stream(
        &self,
        remux_streaming: &RemuxStreamingStrategy,
        request: &StreamingRequest,
    ) -> Result<StreamingResponse, HttpStreamingError> {
        info!("Using progressive remuxing for {}", request.info_hash);

        // Get file size from remux streaming strategy
        let file_size = remux_streaming
            .file_size(request.info_hash)
            .await
            .map_err(|e| HttpStreamingError::RemuxingFailed {
                reason: format!("Remux streaming failed: {e}"),
            })?;

        // Handle range request vs full file request
        if let Some(range) = &request.range {
            // Range request - return 206 Partial Content
            let start = range.start;
            // For remux streaming, file_size may be 0 if remuxing hasn't started
            // Handle browser requesting entire file (end = u64::MAX - 1)
            let end = if file_size == 0 {
                // Unknown size - always use small chunks to avoid insufficient data errors
                start + 64 * 1024 // Stream 64KB chunks initially for remux streaming
            } else {
                range.end.unwrap_or(file_size - 1).min(file_size - 1)
            };

            // Only validate range if we know the file size
            if file_size > 0 && (start >= file_size || end < start) {
                return Err(HttpStreamingError::StreamingNotReady {
                    reason: format!(
                        "Invalid range: start={start}, end={end}, file_size={file_size}"
                    ),
                });
            }

            // Always validate stream readiness by trying to get the first 1KB
            // This ensures HEAD requests get the same status as GET requests
            let readiness_check = remux_streaming
                .stream_range(request.info_hash, 0..1024)
                .await
                .is_ok();

            if !readiness_check {
                // Stream not ready - return 503 with retry
                let mut headers = HeaderMap::new();
                headers.insert("Retry-After", HeaderValue::from_static("2"));
                headers.insert("Content-Type", HeaderValue::from_static("text/plain"));

                return Ok(StreamingResponse {
                    status: StatusCode::SERVICE_UNAVAILABLE,
                    headers,
                    body: Body::from("Remux streaming in progress, please retry"),
                    content_type: "text/plain".to_string(),
                });
            }

            // Get data from progressive strategy - stream is confirmed ready
            let data = match remux_streaming
                .stream_range(request.info_hash, start..end + 1)
                .await
            {
                Ok(data) => data,
                Err(e)
                    if e.to_string().contains("Insufficient output data available")
                        || e.to_string().contains("Waiting for sufficient head data")
                        || e.to_string()
                            .contains("FFmpeg is processing, output not ready yet") =>
                {
                    // This should not happen after readiness check, but handle it
                    let mut headers = HeaderMap::new();
                    headers.insert("Retry-After", HeaderValue::from_static("2"));
                    headers.insert("Content-Type", HeaderValue::from_static("text/plain"));

                    return Ok(StreamingResponse {
                        status: StatusCode::SERVICE_UNAVAILABLE,
                        headers,
                        body: Body::from("Remux streaming in progress, please retry"),
                        content_type: "text/plain".to_string(),
                    });
                }
                Err(e) => {
                    return Err(HttpStreamingError::StreamingNotReady {
                        reason: format!("Remux streaming failed: {e}"),
                    });
                }
            };

            // Update end and length based on actual data received
            let actual_end = start + data.len() as u64 - 1;
            let actual_length = data.len() as u64;

            let mut headers = HeaderMap::new();
            // Handle unknown file size case for remux streaming
            let content_range = if file_size == 0 {
                format!("bytes {start}-{actual_end}/*")
            } else {
                format!("bytes {start}-{actual_end}/{file_size}")
            };
            headers.insert(
                "Content-Range",
                HeaderValue::from_str(&content_range).unwrap(),
            );
            headers.insert(
                "Content-Length",
                HeaderValue::from_str(&actual_length.to_string()).unwrap(),
            );
            headers.insert("Accept-Ranges", HeaderValue::from_static("bytes"));
            headers.insert("Content-Type", HeaderValue::from_static("video/mp4"));

            Ok(StreamingResponse {
                status: StatusCode::PARTIAL_CONTENT,
                headers,
                body: Body::from(data),
                content_type: "video/mp4".to_string(),
            })
        } else {
            // Full file request - for consistency with range requests, treat as range 0-64KB
            // This ensures HEAD and GET requests return the same status codes and headers

            // Always validate stream readiness by trying to get the first 1KB
            // This ensures HEAD requests get the same status as GET requests
            let readiness_check = remux_streaming
                .stream_range(request.info_hash, 0..1024)
                .await
                .is_ok();

            if !readiness_check {
                // Stream not ready - return 503 with retry
                let mut headers = HeaderMap::new();
                headers.insert("Retry-After", HeaderValue::from_static("2"));
                headers.insert("Content-Type", HeaderValue::from_static("text/plain"));

                return Ok(StreamingResponse {
                    status: StatusCode::SERVICE_UNAVAILABLE,
                    headers,
                    body: Body::from("Remux streaming in progress, please retry"),
                    content_type: "text/plain".to_string(),
                });
            }

            // For HEAD/GET consistency, always return first chunk as partial content
            // This matches what browsers expect for streaming media
            let chunk_size = 64 * 1024; // 64KB first chunk
            let data = match remux_streaming
                .stream_range(request.info_hash, 0..chunk_size)
                .await
            {
                Ok(data) => data,
                Err(e)
                    if e.to_string().contains("Insufficient output data available")
                        || e.to_string().contains("Waiting for sufficient head data")
                        || e.to_string()
                            .contains("FFmpeg is processing, output not ready yet") =>
                {
                    // This should not happen after readiness check, but handle it
                    let mut headers = HeaderMap::new();
                    headers.insert("Retry-After", HeaderValue::from_static("2"));
                    headers.insert("Content-Type", HeaderValue::from_static("text/plain"));

                    return Ok(StreamingResponse {
                        status: StatusCode::SERVICE_UNAVAILABLE,
                        headers,
                        body: Body::from("Remux streaming in progress, please retry"),
                        content_type: "text/plain".to_string(),
                    });
                }
                Err(e) => {
                    return Err(HttpStreamingError::StreamingNotReady {
                        reason: format!("Remux streaming failed: {e}"),
                    });
                }
            };

            // Return as partial content for consistency with range requests
            let actual_end = data.len() as u64 - 1;
            let mut headers = HeaderMap::new();

            // Use partial content status and headers for consistency
            let content_range = if file_size == 0 {
                format!("bytes 0-{actual_end}/*")
            } else {
                format!("bytes 0-{actual_end}/{file_size}")
            };

            headers.insert(
                "Content-Range",
                HeaderValue::from_str(&content_range).unwrap(),
            );
            headers.insert(
                "Content-Length",
                HeaderValue::from_str(&data.len().to_string()).unwrap(),
            );
            headers.insert("Accept-Ranges", HeaderValue::from_static("bytes"));
            headers.insert("Content-Type", HeaderValue::from_static("video/mp4"));

            Ok(StreamingResponse {
                status: StatusCode::PARTIAL_CONTENT,
                headers,
                body: Body::from(data),
                content_type: "video/mp4".to_string(),
            })
        }
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

    /// Check if streaming is ready for the given torrent
    pub async fn check_streaming_readiness(
        &self,
        info_hash: InfoHash,
        file_size: u64,
    ) -> Result<bool, HttpStreamingError> {
        if let Some(ref remux_streaming) = self.remux_streaming {
            remux_streaming
                .check_streaming_readiness(info_hash, file_size)
                .await
                .map_err(|e| HttpStreamingError::RemuxingFailed {
                    reason: format!("Streaming readiness check failed: {e}"),
                })
        } else {
            Ok(false)
        }
    }

    /// Detect container format of the file
    async fn detect_container_format(
        &self,
        info_hash: InfoHash,
    ) -> Result<ContainerFormat, HttpStreamingError> {
        // Try to get a small amount of data from the file header to detect format
        let header_data = self
            .file_assembler
            .read_range(info_hash, 0..1024)
            .await
            .map_err(HttpStreamingError::FileAssembler)?;

        Ok(ContainerDetector::detect_format(&header_data))
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
            range: std::ops::Range<u64>,
        ) -> Result<Vec<u8>, riptide_core::streaming::FileAssemblerError> {
            let length = (range.end - range.start) as usize;
            let mut data = vec![0u8; length];

            // If reading from the beginning, provide MP4 header for format detection
            if range.start == 0 && length >= 16 {
                // Create minimal MP4 ftyp box header
                data[4] = b'f'; // ftyp box type
                data[5] = b't';
                data[6] = b'y';
                data[7] = b'p';
                data[8] = b'm'; // mp42 brand
                data[9] = b'p';
                data[10] = b'4';
                data[11] = b'2';
            }

            Ok(data)
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
