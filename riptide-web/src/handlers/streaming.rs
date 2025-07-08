//! Simplified streaming handler using HttpStreamingService
//!
//! This replaces the complex streaming.rs with a clean integration
//! of FileAssembler and FFmpeg remuxing components.

use axum::body::Body;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, Response, StatusCode};
use axum::response::IntoResponse;
use riptide_core::torrent::InfoHash;
use riptide_core::video::VideoQuality;
use serde::Deserialize;
use tracing::{error, info};

use super::range::{extract_range_header, parse_range_header};
use crate::server::AppState;
use crate::streaming::{
    ClientCapabilities, SimpleRangeRequest, StreamingRequest, StreamingResponse,
};

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

/// Main streaming handler - simplified version using HttpStreamingService
pub async fn stream_torrent(
    State(state): State<AppState>,
    Path(info_hash_str): Path<String>,
    Query(query): Query<StreamingQuery>,
    headers: HeaderMap,
) -> Result<Response<Body>, StatusCode> {
    // Parse info hash
    let info_hash = InfoHash::from_hex(&info_hash_str).map_err(|_| {
        error!("Invalid info hash: {}", info_hash_str);
        StatusCode::BAD_REQUEST
    })?;

    // Get streaming service from app state
    let streaming_service = state.streaming_service();

    // Extract range from headers
    let range_request = extract_range_header(&headers)
        .map(|range_str| parse_range_header(&range_str, u64::MAX))
        .map(|(start, end, _)| SimpleRangeRequest {
            start,
            end: if end == u64::MAX { None } else { Some(end) },
        });

    // Parse client capabilities from User-Agent
    let client_capabilities = parse_client_capabilities(&headers);

    // Parse preferred quality
    let preferred_quality = query.quality.as_ref().and_then(|q| match q.as_str() {
        "low" => Some(VideoQuality::Low),
        "medium" => Some(VideoQuality::Medium),
        "high" => Some(VideoQuality::High),
        _ => None,
    });

    // Convert time offset to Duration
    let time_offset = query.t.map(std::time::Duration::from_secs);

    // Build streaming request
    let request = StreamingRequest {
        info_hash,
        range: range_request,
        client_capabilities,
        preferred_quality,
        time_offset,
    };

    info!(
        "Streaming request for {}: range={:?}, time_offset={:?}, quality={:?}",
        info_hash, request.range, time_offset, preferred_quality
    );

    // Handle streaming request
    match streaming_service.handle_streaming_request(request).await {
        Ok(response) => Ok(convert_streaming_response(response)),
        Err(e) => {
            error!("Streaming request failed: {}", e);
            Ok(e.into_response())
        }
    }
}

/// Convert StreamingResponse to Axum Response
fn convert_streaming_response(response: StreamingResponse) -> Response<Body> {
    let mut builder = Response::builder().status(response.status);

    // Add headers
    for (key, value) in response.headers.iter() {
        builder = builder.header(key, value);
    }

    // Build response
    builder.body(response.body).unwrap_or_else(|_| {
        Response::builder()
            .status(StatusCode::INTERNAL_SERVER_ERROR)
            .body(Body::from("Failed to build response"))
            .unwrap()
    })
}

/// Parse client capabilities from HTTP headers
fn parse_client_capabilities(headers: &HeaderMap) -> ClientCapabilities {
    let user_agent = headers
        .get("User-Agent")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("Unknown")
        .to_string();

    // Simple user agent detection
    let supports_mp4 = true; // Nearly all browsers support MP4
    let supports_webm = user_agent.contains("Firefox") || user_agent.contains("Chrome");
    let supports_hls = (user_agent.contains("Safari") && !user_agent.contains("Chrome"))
        || user_agent.contains("Mobile");

    ClientCapabilities {
        supports_mp4,
        supports_webm,
        supports_hls,
        user_agent,
    }
}

/// Health check endpoint for streaming service
pub async fn streaming_health(State(state): State<AppState>) -> impl IntoResponse {
    let streaming_service = state.streaming_service();
    let stats = streaming_service.statistics().await;

    let health_info = serde_json::json!({
        "status": "healthy",
        "active_sessions": stats.active_sessions,
        "max_concurrent_streams": stats.max_concurrent_streams,
        "active_remuxing_jobs": stats.active_remuxing_jobs,
        "adaptive_streaming_enabled": stats.adaptive_streaming_enabled,
    });

    (StatusCode::OK, axum::Json(health_info))
}

/// Get streaming statistics
pub async fn streaming_stats(State(state): State<AppState>) -> impl IntoResponse {
    let streaming_service = state.streaming_service();
    let stats = streaming_service.statistics().await;

    axum::Json(stats)
}

/// Force cleanup of inactive sessions
pub async fn cleanup_sessions(State(state): State<AppState>) -> impl IntoResponse {
    let streaming_service = state.streaming_service();
    streaming_service
        .cleanup_inactive_sessions(std::time::Duration::from_secs(300))
        .await;

    (StatusCode::OK, "Session cleanup completed")
}

#[cfg(test)]
mod tests {
    use axum::http::HeaderValue;

    use super::*;

    #[test]
    fn test_parse_client_capabilities() {
        let mut headers = HeaderMap::new();

        // Test Chrome
        headers.insert(
            "User-Agent",
            HeaderValue::from_static("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/91.0.4472.124 Safari/537.36")
        );

        let capabilities = parse_client_capabilities(&headers);
        assert!(capabilities.supports_mp4);
        assert!(capabilities.supports_webm);
        assert!(!capabilities.supports_hls);
        assert!(capabilities.user_agent.contains("Chrome"));

        // Test Safari
        headers.insert(
            "User-Agent",
            HeaderValue::from_static("Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/14.1.1 Safari/605.1.15")
        );

        let capabilities = parse_client_capabilities(&headers);
        assert!(capabilities.supports_mp4);
        assert!(!capabilities.supports_webm);
        assert!(capabilities.supports_hls);
        assert!(capabilities.user_agent.contains("Safari"));

        // Test Firefox
        headers.insert(
            "User-Agent",
            HeaderValue::from_static(
                "Mozilla/5.0 (Windows NT 10.0; Win64; x64; rv:89.0) Gecko/20100101 Firefox/89.0",
            ),
        );

        let capabilities = parse_client_capabilities(&headers);
        assert!(capabilities.supports_mp4);
        assert!(capabilities.supports_webm);
        assert!(!capabilities.supports_hls);
        assert!(capabilities.user_agent.contains("Firefox"));
    }

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

    #[test]
    fn test_quality_parsing() {
        // Test valid qualities
        assert_eq!(
            "low"
                .parse::<String>()
                .ok()
                .as_ref()
                .and_then(|q| match q.as_str() {
                    "low" => Some(VideoQuality::Low),
                    "medium" => Some(VideoQuality::Medium),
                    "high" => Some(VideoQuality::High),
                    _ => None,
                }),
            Some(VideoQuality::Low)
        );

        // Test invalid quality
        assert_eq!(
            "invalid"
                .parse::<String>()
                .ok()
                .as_ref()
                .and_then(|q| match q.as_str() {
                    "low" => Some(VideoQuality::Low),
                    "medium" => Some(VideoQuality::Medium),
                    "high" => Some(VideoQuality::High),
                    _ => None,
                }),
            None
        );
    }
}
