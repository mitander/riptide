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

/// Main streaming handler using core DirectStreamingService
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
    pub supports_mp4: bool,
    pub supports_webm: bool,
    pub supports_hls: bool,
    pub user_agent: String,
}

/// Health check endpoint for HTTP streaming
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

/// Get streaming statistics
pub async fn streaming_stats(State(state): State<AppState>) -> impl IntoResponse {
    let http_streaming = state.http_streaming();
    let stats = http_streaming.statistics().await;

    axum::Json(stats)
}

/// Force cleanup of inactive sessions
pub async fn cleanup_sessions(State(_state): State<AppState>) -> impl IntoResponse {
    // Core service manages cleanup automatically
    (StatusCode::OK, "Session cleanup completed")
}

/// Debug endpoint to inspect stream status and diagnose issues
/// Debug endpoint to test streaming service health and basic functionality
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

/// Debug endpoint to test actual video data and validate MP4 structure
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
