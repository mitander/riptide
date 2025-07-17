//! HTTP handlers for media streaming endpoints.
//!
//! Provides endpoints for streaming torrent content to web browsers with
//! support for HTTP range requests, seeking, and real-time transcoding.

use axum::body::Body;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, Request, StatusCode};
use axum::response::{Json, Response};
use riptide_core::torrent::InfoHash;
use serde::Deserialize;
use serde_json::json;
use tracing::{error, info, warn};

use crate::server::AppState;

/// Client capabilities for browser compatibility.
#[derive(Debug, Clone)]
pub struct ClientCapabilities {
    /// Whether client supports MP4 format.
    pub supports_mp4: bool,
    /// Whether client supports WebM format.
    pub supports_webm: bool,
    /// Whether client supports HLS streaming.
    pub supports_hls: bool,
    /// User agent string for capability detection.
    pub user_agent: String,
}

/// Query parameters for streaming requests.
#[derive(Debug, Deserialize)]
pub struct StreamQuery {
    /// Time offset in seconds for seeking.
    pub t: Option<f64>,
}

/// Stream torrent content with HTTP range support.
///
/// This endpoint serves media content from torrents with proper HTTP semantics
/// including range requests for seeking and progressive download support.
#[axum::debug_handler]
pub async fn stream_torrent(
    State(state): State<AppState>,
    Path(info_hash_str): Path<String>,
    Query(query): Query<StreamQuery>,
    headers: HeaderMap,
) -> Response {
    // Parse info hash
    let info_hash = match InfoHash::from_hex(&info_hash_str) {
        Ok(hash) => hash,
        Err(_) => {
            error!("Invalid info hash: {}", info_hash_str);
            return error_response(StatusCode::BAD_REQUEST, "Invalid info hash");
        }
    };

    info!(
        "Streaming request for {}: range={:?}, time_offset={:?}",
        info_hash,
        headers.get("range"),
        query.t
    );

    // Handle time-based seeking if requested
    if let Some(time_offset) = query.t
        && time_offset > 0.0
    {
        // For time-based seeking, we'd need to convert time to byte offset
        // This requires parsing media metadata which is complex
        warn!(
            "Time-based seeking not yet implemented: {}s for {}",
            time_offset, info_hash
        );
    }

    // Create HTTP streaming instance for this torrent
    let http_streaming = match state.create_http_streaming(info_hash).await {
        Ok(streaming) => streaming,
        Err(err) => {
            error!(
                "Failed to create streaming session for {}: {}",
                info_hash, err
            );
            return error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Failed to initialize streaming",
            );
        }
    };

    // Build HTTP request to pass to streaming service
    let mut request_builder = Request::builder().method("GET").uri("/");

    // Copy range header if present
    if let Some(range) = headers.get("range") {
        request_builder = request_builder.header("range", range);
    }

    let request = match request_builder.body(()) {
        Ok(req) => req,
        Err(err) => {
            error!("Failed to build request: {}", err);
            return error_response(StatusCode::INTERNAL_SERVER_ERROR, "Invalid request format");
        }
    };

    // Serve the stream
    http_streaming.serve_http_stream(request).await
}

/// Get streaming statistics and health information.
pub async fn streaming_stats(State(_state): State<AppState>) -> Json<serde_json::Value> {
    // With the new architecture, statistics are much simpler
    // We don't maintain global streaming state anymore
    Json(json!({
        "active_sessions": 0,
        "total_bytes_streamed": 0,
        "concurrent_remux_sessions": 0,
        "architecture": "stateless_pipeline"
    }))
}

/// Check overall streaming service health.
pub async fn streaming_health(State(_state): State<AppState>) -> Json<serde_json::Value> {
    Json(json!({
        "status": "healthy",
        "pipeline": "ready",
        "version": "2.0"
    }))
}

/// Debug endpoint to test streaming with specific parameters.
///
/// # Panics
/// Panics if the HTTP request builder fails to construct a valid request.
#[axum::debug_handler]
pub async fn debug_stream_data(
    State(state): State<AppState>,
    Path(info_hash_str): Path<String>,
) -> Json<serde_json::Value> {
    // Parse info hash
    let info_hash = match InfoHash::from_hex(&info_hash_str) {
        Ok(hash) => hash,
        Err(_) => {
            return Json(json!({
                "error": "Invalid info hash",
                "info_hash": info_hash_str
            }));
        }
    };

    // Create streaming instance
    let http_streaming = match state.create_http_streaming(info_hash).await {
        Ok(streaming) => streaming,
        Err(err) => {
            return Json(json!({
                "error": format!("Failed to create streaming session: {}", err),
                "info_hash": info_hash.to_string()
            }));
        }
    };

    // Test basic streaming capability
    let request = Request::builder()
        .method("GET")
        .uri("/")
        .header("range", "bytes=0-99")
        .body(())
        .unwrap();

    let response = http_streaming.serve_http_stream(request).await;

    Json(json!({
        "info_hash": info_hash.to_string(),
        "status": response.status().as_u16(),
        "headers": response.headers().len(),
        "test": "basic_range_request"
    }))
}

/// Debug endpoint to check streaming status.
#[axum::debug_handler]
pub async fn debug_stream_status(
    State(state): State<AppState>,
    Path(info_hash_str): Path<String>,
) -> Json<serde_json::Value> {
    // Parse info hash
    let info_hash = match InfoHash::from_hex(&info_hash_str) {
        Ok(hash) => hash,
        Err(_) => {
            return Json(json!({
                "error": "Invalid info hash",
                "info_hash": info_hash_str
            }));
        }
    };

    // Check if we can create a streaming session
    match state.create_http_streaming(info_hash).await {
        Ok(_) => Json(json!({
            "info_hash": info_hash.to_string(),
            "status": "ready",
            "pipeline": "available"
        })),
        Err(err) => Json(json!({
            "info_hash": info_hash.to_string(),
            "status": "error",
            "error": err.to_string()
        })),
    }
}

/// Cleanup endpoint (no-op in new architecture).
pub async fn cleanup_sessions(State(_state): State<AppState>) -> Json<serde_json::Value> {
    // In the new stateless architecture, there's nothing to clean up
    Json(json!({
        "message": "No cleanup needed in stateless architecture",
        "cleaned": 0
    }))
}

/// Helper function to create error responses.
fn error_response(status: StatusCode, message: &str) -> Response {
    Response::builder()
        .status(status)
        .header("content-type", "text/plain")
        .body(Body::from(message.to_string()))
        .unwrap()
}

/// Extract media duration and file size for a torrent.
pub async fn extract_media_info(
    State(state): State<AppState>,
    Path(info_hash_str): Path<String>,
) -> Option<Json<serde_json::Value>> {
    // Parse info hash
    let info_hash = InfoHash::from_hex(&info_hash_str).ok()?;

    // Get file size from data source
    let file_size = state.data_source.file_size(info_hash).await.ok()?;

    // For duration extraction, we'd need to read media metadata
    // This is complex and requires the old extract_duration logic
    // For now, return basic file info
    Some(Json(json!({
        "info_hash": info_hash.to_string(),
        "file_size": file_size,
        "duration": null, // TODO: Implement duration extraction
        "format": "unknown" // TODO: Implement format detection
    })))
}
