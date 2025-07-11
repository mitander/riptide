//! HTTP streaming coordinator using modular streaming strategies
//!
//! This module provides a clean coordinator pattern that delegates to appropriate
//! streaming strategies based on container format. Replaces the monolithic
//! HttpStreamingService with a focused, strategy-based approach.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::http::{HeaderMap, HeaderValue, StatusCode};
use bytes::Bytes;
use riptide_core::streaming::remux::{
    DirectStreamStrategy, RemuxConfig, RemuxStreamStrategy, StreamingStrategy,
};
use riptide_core::streaming::{
    ContainerFormat, FileAssembler, RemuxSessionManager, StreamData, StreamHandle, StreamReadiness,
    StreamingStatus,
};
use riptide_core::torrent::InfoHash;
use riptide_core::video::VideoQuality;
use tokio::sync::RwLock;
use tracing::{debug, info};

use super::http_streaming::{
    HttpStreamingConfig, HttpStreamingError, SimpleRangeRequest, StreamingRequest,
    StreamingResponse, StreamingSession,
};

/// HTTP streaming coordinator that delegates to appropriate strategies
pub struct StreamingCoordinator {
    file_assembler: Arc<dyn FileAssembler>,
    strategies: HashMap<ContainerFormat, Box<dyn StreamingStrategy>>,
    active_sessions: Arc<RwLock<HashMap<InfoHash, StreamingSession>>>,
    #[allow(dead_code)]
    config: HttpStreamingConfig,
}

impl StreamingCoordinator {
    /// Create new streaming coordinator with strategies
    pub fn new(file_assembler: Arc<dyn FileAssembler>, config: HttpStreamingConfig) -> Self {
        // Setup remux configuration
        let remux_config = RemuxConfig {
            max_concurrent_sessions: config.max_concurrent_streams,
            min_head_size: 3 * 1024 * 1024,              // 3MB
            min_tail_size: 2 * 1024 * 1024,              // 2MB
            remux_timeout: Duration::from_secs(30 * 60), // 30 minutes
            cleanup_after: Duration::from_secs(60 * 60), // 1 hour
            ..Default::default()
        };

        // Create session manager for remuxing
        let session_manager = RemuxSessionManager::new(remux_config, Arc::clone(&file_assembler));

        // Initialize strategies
        let mut strategies: HashMap<ContainerFormat, Box<dyn StreamingStrategy>> = HashMap::new();

        // Direct streaming strategies
        strategies.insert(
            ContainerFormat::Mp4,
            Box::new(DirectStreamStrategy::new(Arc::clone(&file_assembler))),
        );
        strategies.insert(
            ContainerFormat::WebM,
            Box::new(DirectStreamStrategy::new(Arc::clone(&file_assembler))),
        );

        // Remux strategies
        let remux_strategy = RemuxStreamStrategy::new(session_manager);
        strategies.insert(ContainerFormat::Avi, Box::new(remux_strategy.clone()));
        strategies.insert(ContainerFormat::Mkv, Box::new(remux_strategy.clone()));
        strategies.insert(ContainerFormat::Mov, Box::new(remux_strategy));

        Self {
            file_assembler,
            strategies,
            active_sessions: Arc::new(RwLock::new(HashMap::new())),
            config,
        }
    }

    /// Handle a streaming request by delegating to appropriate strategy
    pub async fn handle_streaming_request(
        &self,
        request: StreamingRequest,
    ) -> Result<StreamingResponse, HttpStreamingError> {
        debug!("Handling streaming request for {}", request.info_hash);

        // Detect container format
        let format = self.detect_container_format(request.info_hash).await?;
        debug!("Detected format: {:?} for {}", format, request.info_hash);

        // Get strategy for format
        let strategy =
            self.strategies
                .get(&format)
                .ok_or_else(|| HttpStreamingError::UnsupportedFormat {
                    format: format!("{format:?}"),
                })?;

        // Prepare stream handle
        let handle = strategy
            .prepare_stream(request.info_hash, format)
            .await
            .map_err(|e| HttpStreamingError::RemuxingFailed {
                reason: e.to_string(),
            })?;

        // Update session tracking
        self.update_session(&request, &handle).await;

        // Serve the requested range
        let range = request.range.unwrap_or(SimpleRangeRequest {
            start: 0,
            end: None,
        });

        self.serve_range_with_strategy(strategy.as_ref(), &handle, range)
            .await
    }

    /// Check if a stream is ready for serving
    pub async fn check_stream_readiness(
        &self,
        info_hash: InfoHash,
    ) -> Result<StreamingStatus, HttpStreamingError> {
        // Detect format
        let format = self.detect_container_format(info_hash).await?;

        // Get strategy
        let strategy =
            self.strategies
                .get(&format)
                .ok_or_else(|| HttpStreamingError::UnsupportedFormat {
                    format: format!("{format:?}"),
                })?;

        // Create dummy handle for readiness check
        let handle = StreamHandle {
            info_hash,
            session_id: 0,
            format,
        };

        strategy
            .status(&handle)
            .await
            .map_err(|e| HttpStreamingError::StreamingNotReady {
                reason: e.to_string(),
            })
    }

    /// Detect container format from file headers
    async fn detect_container_format(
        &self,
        info_hash: InfoHash,
    ) -> Result<ContainerFormat, HttpStreamingError> {
        // Read first 64 bytes for format detection
        const HEADER_SIZE: u64 = 64;

        if !self
            .file_assembler
            .is_range_available(info_hash, 0..HEADER_SIZE)
        {
            return Err(HttpStreamingError::StreamingNotReady {
                reason: "Header data not available for format detection".to_string(),
            });
        }

        let header_data = self
            .file_assembler
            .read_range(info_hash, 0..HEADER_SIZE)
            .await?;

        // Simple format detection
        if header_data.starts_with(b"ftyp") || header_data[4..8] == *b"ftyp" {
            Ok(ContainerFormat::Mp4)
        } else if header_data.starts_with(b"\x1A\x45\xDF\xA3") {
            Ok(ContainerFormat::Mkv)
        } else if header_data.starts_with(b"RIFF") && header_data[8..12] == *b"AVI " {
            Ok(ContainerFormat::Avi)
        } else if header_data.starts_with(b"\x1a\x45\xdf\xa3") {
            Ok(ContainerFormat::WebM)
        } else {
            Ok(ContainerFormat::Unknown)
        }
    }

    /// Serve a range using the given strategy
    async fn serve_range_with_strategy(
        &self,
        strategy: &dyn StreamingStrategy,
        handle: &StreamHandle,
        range: SimpleRangeRequest,
    ) -> Result<StreamingResponse, HttpStreamingError> {
        // Convert range request
        let file_size = self.file_assembler.file_size(handle.info_hash).await?;
        let end = range.end.unwrap_or(file_size.saturating_sub(1));
        let range_spec = range.start..end + 1;

        // Check readiness
        let readiness = strategy
            .is_ready(handle, range_spec.clone())
            .await
            .map_err(|e| HttpStreamingError::StreamingNotReady {
                reason: e.to_string(),
            })?;

        match readiness {
            StreamReadiness::Ready => {
                // Serve the data
                let stream_data = strategy
                    .serve_range(handle, range_spec)
                    .await
                    .map_err(|e| HttpStreamingError::RemuxingFailed {
                        reason: e.to_string(),
                    })?;

                self.build_streaming_response(stream_data, range.start, end)
            }
            StreamReadiness::Processing => {
                // Return 425 Too Early for processing streams
                Ok(StreamingResponse {
                    status: StatusCode::TOO_EARLY,
                    headers: HeaderMap::new(),
                    body: Bytes::from("Stream is being processed"),
                    content_type: "text/plain".to_string(),
                })
            }
            StreamReadiness::WaitingForData => {
                // Return 503 Service Unavailable for missing data
                Ok(StreamingResponse {
                    status: StatusCode::SERVICE_UNAVAILABLE,
                    headers: HeaderMap::new(),
                    body: Bytes::from("Waiting for data"),
                    content_type: "text/plain".to_string(),
                })
            }
            StreamReadiness::Failed => Err(HttpStreamingError::RemuxingFailed {
                reason: "Stream processing failed permanently".to_string(),
            }),
            StreamReadiness::CanRetry => {
                // Return 503 Service Unavailable with retry indication
                let mut headers = HeaderMap::new();
                headers.insert("Retry-After", HeaderValue::from_static("30"));
                Ok(StreamingResponse {
                    status: StatusCode::SERVICE_UNAVAILABLE,
                    headers,
                    body: Bytes::from("Processing failed, retry available"),
                    content_type: "text/plain".to_string(),
                })
            }
        }
    }

    /// Build HTTP streaming response from stream data
    fn build_streaming_response(
        &self,
        stream_data: StreamData,
        start: u64,
        end: u64,
    ) -> Result<StreamingResponse, HttpStreamingError> {
        let mut headers = HeaderMap::new();

        // Set content type
        headers.insert(
            "Content-Type",
            HeaderValue::from_str(&stream_data.content_type).map_err(|_| {
                HttpStreamingError::RemuxingFailed {
                    reason: "Invalid content type".to_string(),
                }
            })?,
        );

        // Set content length
        headers.insert(
            "Content-Length",
            HeaderValue::from_str(&stream_data.data.len().to_string()).map_err(|_| {
                HttpStreamingError::RemuxingFailed {
                    reason: "Invalid content length".to_string(),
                }
            })?,
        );

        // Set range headers for partial content
        let status = if start > 0 || stream_data.total_size.is_some_and(|size| end < size - 1) {
            headers.insert("Accept-Ranges", HeaderValue::from_static("bytes"));

            if let Some(total_size) = stream_data.total_size {
                let content_range = format!("bytes {start}-{end}/{total_size}");
                headers.insert(
                    "Content-Range",
                    HeaderValue::from_str(&content_range).map_err(|_| {
                        HttpStreamingError::RemuxingFailed {
                            reason: "Invalid content range".to_string(),
                        }
                    })?,
                );
            }
            StatusCode::PARTIAL_CONTENT
        } else {
            StatusCode::OK
        };

        // Cache headers for efficient streaming
        headers.insert(
            "Cache-Control",
            HeaderValue::from_static("public, max-age=3600"),
        );
        headers.insert("Access-Control-Allow-Origin", HeaderValue::from_static("*"));

        Ok(StreamingResponse {
            status,
            headers,
            body: Bytes::from(stream_data.data),
            content_type: stream_data.content_type,
        })
    }

    /// Update session tracking
    async fn update_session(&self, request: &StreamingRequest, handle: &StreamHandle) {
        let mut sessions = self.active_sessions.write().await;

        let session = sessions
            .entry(request.info_hash)
            .or_insert_with(|| StreamingSession {
                info_hash: request.info_hash,
                current_position: Duration::ZERO,
                preferred_quality: request.preferred_quality.unwrap_or(VideoQuality::Medium),
                client_capabilities: request.client_capabilities.clone(),
                last_request_time: Instant::now(),
                bandwidth_estimate: None,
                active_prefetch_jobs: Vec::new(),
                bytes_served: 0,
                requests_count: 0,
                average_response_time_ms: 0.0,
            });

        session.last_request_time = Instant::now();
        session.requests_count += 1;

        if let Some(time_offset) = request.time_offset {
            session.current_position = time_offset;
        }

        debug!(
            "Updated session for {} (session_id: {}, requests: {})",
            request.info_hash, handle.session_id, session.requests_count
        );
    }

    /// Get statistics about active streaming sessions
    pub async fn statistics(&self) -> CoordinatorStatistics {
        let sessions = self.active_sessions.read().await;

        CoordinatorStatistics {
            active_sessions: sessions.len(),
            total_requests: sessions.values().map(|s| s.requests_count).sum(),
            total_bytes_served: sessions.values().map(|s| s.bytes_served).sum(),
            average_session_duration: Duration::from_secs(300), // TODO: Calculate actual average
        }
    }

    /// Cleanup stale sessions
    pub async fn cleanup_stale_sessions(&self, max_idle: Duration) {
        let mut sessions = self.active_sessions.write().await;
        let now = Instant::now();

        sessions.retain(|info_hash, session| {
            let is_active = now.duration_since(session.last_request_time) < max_idle;
            if !is_active {
                info!("Cleaned up stale session for {}", info_hash);
            }
            is_active
        });
    }
}

/// Statistics about the streaming coordinator
#[derive(Debug, Clone, serde::Serialize)]
pub struct CoordinatorStatistics {
    pub active_sessions: usize,
    pub total_requests: u64,
    pub total_bytes_served: u64,
    pub average_session_duration: Duration,
}

// Clone implementation moved to riptide-core where RemuxStreamStrategy is defined

#[cfg(test)]
mod tests {
    use std::collections::HashMap as StdHashMap;

    use async_trait::async_trait;
    use riptide_core::streaming::FileAssemblerError;

    use super::*;
    use crate::streaming::ClientCapabilities;

    struct MockFileAssembler {
        files: StdHashMap<InfoHash, Vec<u8>>,
        available_ranges: StdHashMap<InfoHash, Vec<std::ops::Range<u64>>>,
    }

    impl MockFileAssembler {
        fn new() -> Self {
            Self {
                files: StdHashMap::new(),
                available_ranges: StdHashMap::new(),
            }
        }

        fn add_file(&mut self, info_hash: InfoHash, data: Vec<u8>) {
            let file_size = data.len() as u64;
            self.files.insert(info_hash, data);
            // Make entire file available by default
            #[allow(clippy::single_range_in_vec_init)]
            self.available_ranges.insert(info_hash, vec![0..file_size]);
        }
    }

    #[async_trait]
    impl FileAssembler for MockFileAssembler {
        async fn read_range(
            &self,
            info_hash: InfoHash,
            range: std::ops::Range<u64>,
        ) -> Result<Vec<u8>, FileAssemblerError> {
            let data =
                self.files
                    .get(&info_hash)
                    .ok_or_else(|| FileAssemblerError::CacheError {
                        reason: "File not found".to_string(),
                    })?;

            let start = range.start as usize;
            let end = range.end.min(data.len() as u64) as usize;

            Ok(data[start..end].to_vec())
        }

        async fn file_size(&self, info_hash: InfoHash) -> Result<u64, FileAssemblerError> {
            self.files
                .get(&info_hash)
                .map(|data| data.len() as u64)
                .ok_or_else(|| FileAssemblerError::CacheError {
                    reason: "File not found".to_string(),
                })
        }

        fn is_range_available(&self, info_hash: InfoHash, range: std::ops::Range<u64>) -> bool {
            if let Some(available) = self.available_ranges.get(&info_hash) {
                available
                    .iter()
                    .any(|r| r.start <= range.start && r.end >= range.end)
            } else {
                false
            }
        }
    }

    /// Create test MP4 file data
    fn create_test_mp4_data() -> Vec<u8> {
        let mut data = Vec::new();

        // ftyp box
        data.extend_from_slice(&(32u32).to_be_bytes()); // Box size
        data.extend_from_slice(b"ftyp"); // Box type
        data.extend_from_slice(b"mp41"); // Major brand
        data.extend_from_slice(&(0u32).to_be_bytes()); // Minor version
        data.extend_from_slice(b"mp41"); // Compatible brand 1
        data.extend_from_slice(b"isom"); // Compatible brand 2
        data.extend_from_slice(b"avc1"); // Compatible brand 3

        // moov box
        data.extend_from_slice(&(8u32).to_be_bytes()); // Box size
        data.extend_from_slice(b"moov"); // Box type

        // mdat box
        data.extend_from_slice(&(1024u32).to_be_bytes()); // Box size
        data.extend_from_slice(b"mdat"); // Box type
        data.extend_from_slice(&vec![0u8; 1016]); // Dummy media data

        data
    }

    #[tokio::test]
    async fn test_coordinator_mp4_direct_streaming() {
        let mut file_assembler = MockFileAssembler::new();
        let info_hash = InfoHash::new([1u8; 20]);
        let mp4_data = create_test_mp4_data();

        file_assembler.add_file(info_hash, mp4_data.clone());

        let config = HttpStreamingConfig::default();
        let coordinator = StreamingCoordinator::new(Arc::new(file_assembler), config);

        let request = StreamingRequest {
            info_hash,
            range: Some(SimpleRangeRequest {
                start: 0,
                end: Some(1023),
            }),
            client_capabilities: ClientCapabilities {
                supports_mp4: true,
                ..Default::default()
            },
            preferred_quality: Some(VideoQuality::High),
            time_offset: None,
        };

        let response = coordinator.handle_streaming_request(request).await.unwrap();
        assert_eq!(response.status, StatusCode::PARTIAL_CONTENT);
        assert_eq!(response.content_type, "video/mp4");
    }

    #[tokio::test]
    async fn test_coordinator_readiness_check() {
        let mut file_assembler = MockFileAssembler::new();
        let info_hash = InfoHash::new([2u8; 20]);
        let mp4_data = create_test_mp4_data();

        file_assembler.add_file(info_hash, mp4_data);

        let config = HttpStreamingConfig::default();
        let coordinator = StreamingCoordinator::new(Arc::new(file_assembler), config);

        let status = coordinator.check_stream_readiness(info_hash).await.unwrap();
        assert_eq!(status.readiness, StreamReadiness::Ready);
    }

    #[tokio::test]
    async fn test_coordinator_unsupported_format() {
        let file_assembler = MockFileAssembler::new();
        let config = HttpStreamingConfig::default();
        let coordinator = StreamingCoordinator::new(Arc::new(file_assembler), config);

        let info_hash = InfoHash::new([3u8; 20]);
        let result = coordinator.check_stream_readiness(info_hash).await;

        assert!(matches!(
            result,
            Err(HttpStreamingError::StreamingNotReady { .. })
        ));
    }

    #[tokio::test]
    async fn test_coordinator_session_tracking() {
        let mut file_assembler = MockFileAssembler::new();
        let info_hash = InfoHash::new([4u8; 20]);
        let mp4_data = create_test_mp4_data();

        file_assembler.add_file(info_hash, mp4_data);

        let config = HttpStreamingConfig::default();
        let coordinator = StreamingCoordinator::new(Arc::new(file_assembler), config);

        let request = StreamingRequest {
            info_hash,
            range: None,
            client_capabilities: ClientCapabilities::default(),
            preferred_quality: Some(VideoQuality::Medium),
            time_offset: Some(Duration::from_secs(30)),
        };

        let _response = coordinator.handle_streaming_request(request).await.unwrap();

        let stats = coordinator.statistics().await;
        assert_eq!(stats.active_sessions, 1);
        assert_eq!(stats.total_requests, 1);
    }
}
