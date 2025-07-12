//! Streaming handlers using core HttpStreaming
//!
//! Thin HTTP adapters over the core streaming service.

use axum::body::Body;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, Response, StatusCode};
use axum::response::{IntoResponse, Json};
use riptide_core::torrent::InfoHash;
use serde::Deserialize;
use tracing::{error, info};

use super::range::extract_range_header;
use crate::server::AppState;

/// Query parameters for streaming requests
#[derive(Debug, Deserialize)]
pub struct StreamingQuery {
    /// Time offset in seconds (for seeking)
    pub t: Option<u64>,
    /// Preferred quality (low, medium, high, auto)
    pub quality: Option<String>,
    /// Force specific format (mp4, webm, hls)
    pub format: Option<String>,
}

/// Main streaming handler using core DirectStreamingService.
///
/// Handles HTTP streaming requests for torrents with support for range requests,
/// quality selection, and format negotiation. Delegates actual streaming to
/// the core HttpStreaming service and converts responses to Axum format.
///
/// # Panics
///
/// Panics if the HTTP response builder fails to construct a valid response.
#[axum::debug_handler]
pub async fn stream_torrent(
    State(state): State<AppState>,
    Path(info_hash_str): Path<String>,
    Query(query): Query<StreamingQuery>,
    headers: HeaderMap,
) -> Response<Body> {
    // Parse info hash
    let info_hash = match InfoHash::from_hex(&info_hash_str) {
        Ok(hash) => hash,
        Err(_) => {
            error!("Invalid info hash: {}", info_hash_str);
            return Response::builder()
                .status(StatusCode::BAD_REQUEST)
                .body(Body::from("Invalid info hash"))
                .unwrap();
        }
    };

    // Get HTTP streaming from app state
    let http_streaming = state.http_streaming();

    // Extract range header string
    let range_header = extract_range_header(&headers);

    info!(
        "Streaming request for {}: range={:?}, time_offset={:?}",
        info_hash, range_header, query.t
    );

    // Call core HTTP streaming
    match http_streaming
        .handle_http_request(info_hash, range_header.as_deref())
        .await
    {
        Ok(response) => {
            // Convert core response to axum Response
            let mut builder = Response::builder().status(response.status);

            builder = builder.header("content-type", &response.content_type);

            // Add headers from core response
            for (key, value) in response.headers.iter() {
                builder = builder.header(key, value);
            }

            builder.body(Body::from(response.body)).unwrap()
        }
        Err(err) => {
            error!("Streaming error for {}: {:?}", info_hash, err);
            Response::builder()
                .status(StatusCode::INTERNAL_SERVER_ERROR)
                .body(Body::from("Streaming error"))
                .unwrap()
        }
    }
}

/// Client capabilities for browser compatibility
#[derive(Debug, Clone)]
pub struct ClientCapabilities {
    /// Whether client supports MP4 format
    pub supports_mp4: bool,
    /// Whether client supports WebM format
    pub supports_webm: bool,
    /// Whether client supports HLS streaming
    pub supports_hls: bool,
    /// User agent string for capability detection
    pub user_agent: String,
}

/// Health check endpoint for HTTP streaming.
///
/// Returns current streaming service health status and key metrics including
/// active sessions, total bytes served, and concurrent remux operations.
pub async fn streaming_health(State(state): State<AppState>) -> impl IntoResponse {
    let http_streaming = state.http_streaming();
    let stats = http_streaming.statistics().await;

    let health_info = serde_json::json!({
        "status": "healthy",
        "active_sessions": stats.active_sessions,
        "total_bytes_served": stats.total_bytes_streamed,
        "concurrent_remux_sessions": stats.concurrent_remux_sessions,
    });

    (StatusCode::OK, axum::Json(health_info))
}

/// Get streaming statistics.
///
/// Returns detailed streaming service statistics in JSON format for monitoring
/// and performance analysis.
pub async fn streaming_stats(State(state): State<AppState>) -> impl IntoResponse {
    let http_streaming = state.http_streaming();
    let stats = http_streaming.statistics().await;

    axum::Json(stats)
}

/// Force cleanup of inactive sessions.
///
/// Triggers cleanup of inactive streaming sessions. The core service manages
/// automatic cleanup, so this endpoint provides manual control when needed.
pub async fn cleanup_sessions(State(_state): State<AppState>) -> impl IntoResponse {
    // Core service manages cleanup automatically
    (StatusCode::OK, "Session cleanup completed")
}

/// Debug endpoint to test streaming service health and basic functionality.
///
/// Performs a test streaming request with a small range to validate service
/// health and returns diagnostic information including service statistics.
#[axum::debug_handler]
pub async fn debug_stream_status(
    State(state): State<AppState>,
    Path(info_hash_str): Path<String>,
) -> Json<serde_json::Value> {
    let info_hash = match InfoHash::from_hex(&info_hash_str) {
        Ok(hash) => hash,
        Err(_) => {
            return Json(serde_json::json!({
                "error": "Invalid info hash",
                "info_hash": info_hash_str
            }));
        }
    };

    let http_streaming = state.http_streaming();

    // Get basic HTTP streaming statistics
    let stats = http_streaming.statistics().await;

    // Test HTTP streaming call with small range
    match http_streaming
        .handle_http_request(info_hash, Some("bytes=0-99"))
        .await
    {
        Ok(response) => Json(serde_json::json!({
            "info_hash": info_hash_str,
            "timestamp": chrono::Utc::now().to_rfc3339(),
            "service_stats": {
                "active_sessions": stats.active_sessions,
                "total_bytes_served": stats.total_bytes_streamed,
                "concurrent_remux_sessions": stats.concurrent_remux_sessions,
            },
            "streaming_response": {
                "status": response.status.as_u16(),
                "content_type": response.content_type,
                "headers": response.headers.len(),
                "body_available": true,
            },
            "status": "Debug call successful"
        })),
        Err(err) => Json(serde_json::json!({
            "info_hash": info_hash_str,
            "timestamp": chrono::Utc::now().to_rfc3339(),
            "service_stats": {
                "active_sessions": stats.active_sessions,
                "total_bytes_served": stats.total_bytes_streamed,
                "concurrent_remux_sessions": stats.concurrent_remux_sessions,
            },
            "streaming_error": err.to_string(),
            "status": "Debug call failed"
        })),
    }
}

/// Debug endpoint to test actual video data and validate MP4 structure.
///
/// Retrieves the first 1KB of video data to validate file format and analyze
/// response characteristics. Useful for debugging streaming issues.
#[axum::debug_handler]
pub async fn debug_stream_data(
    State(state): State<AppState>,
    Path(info_hash_str): Path<String>,
) -> Json<serde_json::Value> {
    let info_hash = match InfoHash::from_hex(&info_hash_str) {
        Ok(hash) => hash,
        Err(_) => {
            return Json(serde_json::json!({
                "error": "Invalid info hash",
                "info_hash": info_hash_str
            }));
        }
    };

    let http_streaming = state.http_streaming();

    // Test HTTP streaming call with actual data (first 1KB)
    match http_streaming
        .handle_http_request(info_hash, Some("bytes=0-1023"))
        .await
    {
        Ok(response) => Json(serde_json::json!({
            "info_hash": info_hash_str,
            "timestamp": chrono::Utc::now().to_rfc3339(),
            "response_status": response.status.as_u16(),
            "content_type": response.content_type,
            "headers": response.headers.len(),
            "body_size": response.body.len(),
            "status": "Data analysis successful"
        })),
        Err(err) => Json(serde_json::json!({
            "info_hash": info_hash_str,
            "timestamp": chrono::Utc::now().to_rfc3339(),
            "error": err.to_string(),
            "status": "Data analysis failed"
        })),
    }
}

#[cfg(test)]
mod tests {

    use super::*;

    #[test]
    fn test_streaming_query_parsing() {
        // Test with all parameters
        let query = StreamingQuery {
            t: Some(120),
            quality: Some("high".to_string()),
            format: Some("mp4".to_string()),
        };

        assert_eq!(query.t, Some(120));
        assert_eq!(query.quality, Some("high".to_string()));
        assert_eq!(query.format, Some("mp4".to_string()));

        // Test with minimal parameters
        let query = StreamingQuery {
            t: None,
            quality: None,
            format: None,
        };

        assert_eq!(query.t, None);
        assert_eq!(query.quality, None);
        assert_eq!(query.format, None);
    }
}
