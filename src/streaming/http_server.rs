//! HTTP server for media streaming with range request support

use std::net::SocketAddr;
use std::sync::Arc;

use axum::Router;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use hyper::header::{ACCEPT_RANGES, CONTENT_LENGTH, CONTENT_RANGE, CONTENT_TYPE};
use mime_guess::from_path;
use serde::Deserialize;
use tokio::sync::RwLock;
use tower_http::cors::CorsLayer;

use super::range_handler::FileInfo;
use super::{StreamCoordinator, StreamingError};
use crate::config::RiptideConfig;

/// HTTP server configuration for streaming service.
#[derive(Debug, Clone)]
pub struct StreamingServerConfig {
    pub bind_address: SocketAddr,
    pub enable_cors: bool,
    pub max_concurrent_streams: usize,
    pub chunk_size: usize,
    pub buffer_ahead_duration: std::time::Duration,
}

impl StreamingServerConfig {
    /// Create server config from Riptide configuration.
    pub fn from_riptide_config(_config: &RiptideConfig) -> Self {
        Self {
            bind_address: "127.0.0.1:8080".parse().unwrap(), // Default streaming port
            enable_cors: true,
            max_concurrent_streams: 10,
            chunk_size: 64 * 1024, // 64KB chunks
            buffer_ahead_duration: std::time::Duration::from_secs(30),
        }
    }
}

/// HTTP server for media streaming with range request support.
pub struct StreamingHttpServer {
    config: StreamingServerConfig,
    coordinator: Arc<RwLock<StreamCoordinator>>,
    shutdown_sender: Option<tokio::sync::oneshot::Sender<()>>,
}

/// Application state for HTTP handlers.
#[derive(Clone)]
struct AppState {
    coordinator: Arc<RwLock<StreamCoordinator>>,
    config: StreamingServerConfig,
}

/// Query parameters for streaming requests.
#[derive(Deserialize)]
struct StreamQuery {
    #[serde(default)]
    seek: Option<u64>,
    #[serde(default)]
    format: Option<String>,
}

impl StreamingHttpServer {
    /// Creates new HTTP server with configuration.
    pub fn new(config: StreamingServerConfig, coordinator: Arc<RwLock<StreamCoordinator>>) -> Self {
        Self {
            config,
            coordinator,
            shutdown_sender: None,
        }
    }

    /// Start the HTTP server.
    ///
    /// # Errors
    /// - `StreamingError::ServerStartFailed` - Failed to bind or start server
    pub async fn start(&self) -> Result<(), StreamingError> {
        let app_state = AppState {
            coordinator: Arc::clone(&self.coordinator),
            config: self.config.clone(),
        };

        let app = Router::new()
            .route("/stream/:info_hash", get(stream_handler))
            .route("/stream/:info_hash/:file_index", get(stream_file_handler))
            .route("/api/torrents", get(list_torrents_handler))
            .route("/api/stats", get(stats_handler))
            .route("/health", get(health_handler))
            .with_state(app_state);

        let app = if self.config.enable_cors {
            app.layer(CorsLayer::permissive())
        } else {
            app
        };

        let listener = tokio::net::TcpListener::bind(self.config.bind_address)
            .await
            .map_err(|e| StreamingError::ServerStartFailed {
                address: self.config.bind_address,
                reason: e.to_string(),
            })?;

        tracing::info!("Streaming server starting on {}", self.config.bind_address);

        tokio::spawn(async move {
            if let Err(e) = axum::serve(listener, app).await {
                tracing::error!("Server error: {}", e);
            }
        });

        Ok(())
    }

    /// Stop the HTTP server gracefully.
    pub async fn stop(&mut self) -> Result<(), StreamingError> {
        if let Some(sender) = self.shutdown_sender.take() {
            let _ = sender.send(());
        }
        Ok(())
    }
}

/// Handle streaming requests for a torrent.
async fn stream_handler(
    State(state): State<AppState>,
    Path(info_hash): Path<String>,
    headers: HeaderMap,
    Query(query): Query<StreamQuery>,
) -> impl IntoResponse {
    tracing::debug!("Stream request for torrent: {}", info_hash);

    let info_hash_bytes = match hex::decode(&info_hash) {
        Ok(bytes) if bytes.len() == 20 => {
            let mut hash = [0u8; 20];
            hash.copy_from_slice(&bytes);
            hash
        }
        _ => {
            tracing::warn!("Invalid info hash: {}", info_hash);
            return (StatusCode::BAD_REQUEST, "Invalid torrent hash").into_response();
        }
    };

    let coordinator = state.coordinator.read().await;

    // Get torrent metadata
    let content_info = match coordinator.get_content_info(info_hash_bytes).await {
        Ok(info) => info,
        Err(e) => {
            tracing::error!("Failed to get content info: {}", e);
            return (StatusCode::NOT_FOUND, "Torrent not found").into_response();
        }
    };

    // Parse range header if present
    let range_request = parse_range_header(&headers, content_info.total_size);

    match handle_range_request(
        &coordinator,
        info_hash_bytes,
        range_request,
        &content_info,
        query.seek,
    )
    .await
    {
        Ok(response) => response.into_response(),
        Err(e) => {
            tracing::error!("Stream error: {}", e);
            (StatusCode::INTERNAL_SERVER_ERROR, "Streaming error").into_response()
        }
    }
}

/// Handle streaming requests for a specific file within a torrent.
async fn stream_file_handler(
    State(state): State<AppState>,
    Path((info_hash, file_index)): Path<(String, usize)>,
    headers: HeaderMap,
    Query(query): Query<StreamQuery>,
) -> impl IntoResponse {
    tracing::debug!("Stream file request: {} file {}", info_hash, file_index);

    let info_hash_bytes = match hex::decode(&info_hash) {
        Ok(bytes) if bytes.len() == 20 => {
            let mut hash = [0u8; 20];
            hash.copy_from_slice(&bytes);
            hash
        }
        _ => {
            return (StatusCode::BAD_REQUEST, "Invalid torrent hash").into_response();
        }
    };

    let coordinator = state.coordinator.read().await;

    // Get file info
    let file_info = match coordinator.get_file_info(info_hash_bytes, file_index).await {
        Ok(info) => info,
        Err(e) => {
            tracing::error!("Failed to get file info: {}", e);
            return (StatusCode::NOT_FOUND, "File not found").into_response();
        }
    };

    // Parse range header
    let range_request = parse_range_header(&headers, file_info.size);

    match handle_file_range_request(
        &coordinator,
        info_hash_bytes,
        file_index,
        range_request,
        &file_info,
        query.seek,
    )
    .await
    {
        Ok(response) => response.into_response(),
        Err(e) => {
            tracing::error!("File stream error: {}", e);
            (StatusCode::INTERNAL_SERVER_ERROR, "File streaming error").into_response()
        }
    }
}

/// List available torrents.
async fn list_torrents_handler(State(state): State<AppState>) -> impl IntoResponse {
    let coordinator = state.coordinator.read().await;

    match coordinator.list_torrents().await {
        Ok(torrents) => axum::Json(torrents).into_response(),
        Err(e) => {
            tracing::error!("Failed to list torrents: {}", e);
            (StatusCode::INTERNAL_SERVER_ERROR, "Failed to list torrents").into_response()
        }
    }
}

/// Get streaming statistics.
async fn stats_handler(State(state): State<AppState>) -> impl IntoResponse {
    let coordinator = state.coordinator.read().await;
    let stats = coordinator.get_stats().await;
    axum::Json(stats).into_response()
}

/// Health check endpoint.
async fn health_handler() -> impl IntoResponse {
    axum::Json(serde_json::json!({
        "status": "healthy",
        "service": "riptide-streaming",
        "timestamp": chrono::Utc::now().to_rfc3339()
    }))
    .into_response()
}

/// Parse HTTP Range header into structured range request.
fn parse_range_header(headers: &HeaderMap, content_length: u64) -> Option<super::RangeRequest> {
    let range_header = headers.get("range")?;
    let range_str = range_header.to_str().ok()?;

    if !range_str.starts_with("bytes=") {
        return None;
    }

    let ranges_str = &range_str[6..]; // Remove "bytes=" prefix
    let mut ranges = Vec::new();

    for range_part in ranges_str.split(',') {
        let range_part = range_part.trim();

        if let Some((start_str, end_str)) = range_part.split_once('-') {
            let (start, end) = if start_str.is_empty() {
                // Suffix range: "-500" means last 500 bytes
                if let Ok(suffix_length) = end_str.parse::<u64>() {
                    let start = content_length.saturating_sub(suffix_length);
                    let end = content_length - 1;
                    (start, end)
                } else {
                    continue;
                }
            } else if end_str.is_empty() {
                // Open range: "500-" means from byte 500 to end
                if let Ok(start) = start_str.parse::<u64>() {
                    (start, content_length - 1)
                } else {
                    continue;
                }
            } else {
                // Normal range: "100-200"
                if let (Ok(start), Ok(end)) = (start_str.parse::<u64>(), end_str.parse::<u64>()) {
                    (start, end.min(content_length - 1))
                } else {
                    continue;
                }
            };

            if start <= end && start < content_length {
                ranges.push((start, end));
            }
        }
    }

    if ranges.is_empty() {
        None
    } else {
        Some(super::RangeRequest { ranges })
    }
}

/// Handle range request for entire torrent content.
async fn handle_range_request(
    coordinator: &StreamCoordinator,
    info_hash: [u8; 20],
    range_request: Option<super::RangeRequest>,
    content_info: &super::ContentInfo,
    _seek_position: Option<u64>,
) -> Result<Response, StreamingError> {
    let (start, end, status_code) = match range_request {
        Some(range) => {
            if range.ranges.len() != 1 {
                return Err(StreamingError::UnsupportedRange {
                    reason: "Multiple ranges not supported".to_string(),
                });
            }
            let (start, end) = range.ranges[0];
            (start, end, StatusCode::PARTIAL_CONTENT)
        }
        None => (0, content_info.total_size - 1, StatusCode::OK),
    };

    // Read data from torrent
    let data = coordinator
        .read_range(info_hash, start, end - start + 1)
        .await?;

    let content_type = content_info
        .mime_type
        .as_deref()
        .unwrap_or("application/octet-stream");

    let mut response = Response::builder()
        .status(status_code)
        .header(CONTENT_TYPE, content_type)
        .header(ACCEPT_RANGES, "bytes")
        .header(CONTENT_LENGTH, data.len().to_string());

    if status_code == StatusCode::PARTIAL_CONTENT {
        response = response.header(
            CONTENT_RANGE,
            format!("bytes {}-{}/{}", start, end, content_info.total_size),
        );
    }

    response
        .body(axum::body::Body::from(data))
        .map_err(|e| StreamingError::ResponseError {
            reason: e.to_string(),
        })
}

/// Handle range request for specific file within torrent.
async fn handle_file_range_request(
    coordinator: &StreamCoordinator,
    info_hash: [u8; 20],
    file_index: usize,
    range_request: Option<super::RangeRequest>,
    file_info: &FileInfo,
    _seek_position: Option<u64>,
) -> Result<Response, StreamingError> {
    let (start, end, status_code) = match range_request {
        Some(range) => {
            if range.ranges.len() != 1 {
                return Err(StreamingError::UnsupportedRange {
                    reason: "Multiple ranges not supported".to_string(),
                });
            }
            let (start, end) = range.ranges[0];
            (start, end, StatusCode::PARTIAL_CONTENT)
        }
        None => (0, file_info.size - 1, StatusCode::OK),
    };

    // Read file data from torrent
    let data = coordinator
        .read_file_range(info_hash, file_index, start, end - start + 1)
        .await?;

    let content_type = from_path(&file_info.name)
        .first_or_octet_stream()
        .to_string();

    let mut response = Response::builder()
        .status(status_code)
        .header(CONTENT_TYPE, content_type)
        .header(ACCEPT_RANGES, "bytes")
        .header(CONTENT_LENGTH, data.len().to_string());

    if status_code == StatusCode::PARTIAL_CONTENT {
        response = response.header(
            CONTENT_RANGE,
            format!("bytes {}-{}/{}", start, end, file_info.size),
        );
    }

    response
        .body(axum::body::Body::from(data))
        .map_err(|e| StreamingError::ResponseError {
            reason: e.to_string(),
        })
}

#[cfg(test)]
mod tests {
    use hyper::header::HeaderValue;

    use super::*;

    #[test]
    fn test_parse_range_header_simple() {
        let mut headers = HeaderMap::new();
        headers.insert("range", HeaderValue::from_static("bytes=0-1023"));

        let range = parse_range_header(&headers, 2048);
        assert!(range.is_some());

        let range = range.unwrap();
        assert_eq!(range.ranges.len(), 1);
        assert_eq!(range.ranges[0], (0, 1023));
    }

    #[test]
    fn test_parse_range_header_suffix() {
        let mut headers = HeaderMap::new();
        headers.insert("range", HeaderValue::from_static("bytes=-1024"));

        let range = parse_range_header(&headers, 2048);
        assert!(range.is_some());

        let range = range.unwrap();
        assert_eq!(range.ranges.len(), 1);
        assert_eq!(range.ranges[0], (1024, 2047));
    }

    #[test]
    fn test_parse_range_header_open() {
        let mut headers = HeaderMap::new();
        headers.insert("range", HeaderValue::from_static("bytes=1024-"));

        let range = parse_range_header(&headers, 2048);
        assert!(range.is_some());

        let range = range.unwrap();
        assert_eq!(range.ranges.len(), 1);
        assert_eq!(range.ranges[0], (1024, 2047));
    }

    #[test]
    fn test_parse_range_header_invalid() {
        let mut headers = HeaderMap::new();
        headers.insert("range", HeaderValue::from_static("invalid"));

        let range = parse_range_header(&headers, 2048);
        assert!(range.is_none());
    }

    #[test]
    fn test_streaming_server_config_from_riptide() {
        let riptide_config = RiptideConfig::default();
        let server_config = StreamingServerConfig::from_riptide_config(&riptide_config);

        assert_eq!(server_config.bind_address.port(), 8080);
        assert!(server_config.enable_cors);
        assert_eq!(server_config.max_concurrent_streams, 10);
    }
}
