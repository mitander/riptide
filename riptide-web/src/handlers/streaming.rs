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

/// Debug endpoint to inspect stream status and diagnose issues
pub async fn debug_stream_status(
    State(state): State<AppState>,
    Path(info_hash_str): Path<String>,
) -> impl IntoResponse {
    let info_hash = match InfoHash::from_hex(&info_hash_str) {
        Ok(hash) => hash,
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                axum::Json(serde_json::json!({
                    "error": "Invalid info hash",
                    "info_hash": info_hash_str
                })),
            );
        }
    };

    let streaming_service = state.streaming_service();

    // Try to get basic stream info
    let mut debug_info = serde_json::json!({
        "info_hash": info_hash_str,
        "timestamp": chrono::Utc::now().to_rfc3339(),
    });

    // Test HEAD request
    let head_request = StreamingRequest {
        info_hash,
        range: None,
        client_capabilities: ClientCapabilities {
            supports_mp4: true,
            supports_webm: false,
            supports_hls: false,
            user_agent: "Debug-Agent".to_string(),
        },
        preferred_quality: None,
        time_offset: None,
    };

    match streaming_service
        .handle_streaming_request(head_request)
        .await
    {
        Ok(response) => {
            debug_info["head_request"] = serde_json::json!({
                "status": response.status.as_u16(),
                "content_type": response.content_type,
                "headers": response.headers.iter().map(|(k, v)| {
                    (k.to_string(), v.to_str().unwrap_or("invalid").to_string())
                }).collect::<std::collections::HashMap<_, _>>(),
                "body_present": true,
            });
        }
        Err(e) => {
            debug_info["head_request"] = serde_json::json!({
                "error": e.to_string(),
            });
        }
    }

    // Test small GET request
    let get_request = StreamingRequest {
        info_hash,
        range: Some(crate::streaming::SimpleRangeRequest {
            start: 0,
            end: Some(1023),
        }),
        client_capabilities: ClientCapabilities {
            supports_mp4: true,
            supports_webm: false,
            supports_hls: false,
            user_agent: "Debug-Agent".to_string(),
        },
        preferred_quality: None,
        time_offset: None,
    };

    match streaming_service
        .handle_streaming_request(get_request)
        .await
    {
        Ok(response) => {
            debug_info["get_request"] = serde_json::json!({
                "status": response.status.as_u16(),
                "content_type": response.content_type,
                "headers": response.headers.iter().map(|(k, v)| {
                    (k.to_string(), v.to_str().unwrap_or("invalid").to_string())
                }).collect::<std::collections::HashMap<_, _>>(),
                "body_present": true,
            });
        }
        Err(e) => {
            debug_info["get_request"] = serde_json::json!({
                "error": e.to_string(),
            });
        }
    }

    // Get streaming service stats
    let stats = streaming_service.statistics().await;
    debug_info["service_stats"] = serde_json::json!({
        "active_sessions": stats.active_sessions,
        "max_concurrent_streams": stats.max_concurrent_streams,
        "active_remuxing_jobs": stats.active_remuxing_jobs,
        "adaptive_streaming_enabled": stats.adaptive_streaming_enabled,
    });

    (StatusCode::OK, axum::Json(debug_info))
}

/// Debug endpoint to test actual video data and validate MP4 structure
pub async fn debug_stream_data(
    State(state): State<AppState>,
    Path(info_hash_str): Path<String>,
) -> impl IntoResponse {
    let info_hash = match InfoHash::from_hex(&info_hash_str) {
        Ok(hash) => hash,
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                axum::Json(serde_json::json!({
                    "error": "Invalid info hash",
                    "info_hash": info_hash_str
                })),
            );
        }
    };

    let streaming_service = state.streaming_service();

    // Get first 64KB of data to analyze
    let data_request = StreamingRequest {
        info_hash,
        range: Some(crate::streaming::SimpleRangeRequest {
            start: 0,
            end: Some(65535), // 64KB
        }),
        client_capabilities: ClientCapabilities {
            supports_mp4: true,
            supports_webm: false,
            supports_hls: false,
            user_agent: "Debug-Agent".to_string(),
        },
        preferred_quality: None,
        time_offset: None,
    };

    match streaming_service
        .handle_streaming_request(data_request)
        .await
    {
        Ok(response) => {
            let body_bytes = match axum::body::to_bytes(response.body, usize::MAX).await {
                Ok(bytes) => bytes,
                Err(e) => {
                    return (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        axum::Json(serde_json::json!({
                            "error": "Failed to read response body",
                            "details": e.to_string()
                        })),
                    );
                }
            };

            let mut debug_info = serde_json::json!({
                "info_hash": info_hash_str,
                "status": response.status.as_u16(),
                "content_type": response.content_type,
                "data_size": body_bytes.len(),
                "timestamp": chrono::Utc::now().to_rfc3339(),
            });

            // Analyze first 32 bytes for format detection
            if body_bytes.len() >= 32 {
                let header_hex = body_bytes[0..32]
                    .iter()
                    .map(|b| format!("{b:02x}"))
                    .collect::<Vec<_>>()
                    .join(" ");

                debug_info["header_hex"] = serde_json::json!(header_hex);
                debug_info["header_ascii"] =
                    serde_json::json!(String::from_utf8_lossy(&body_bytes[0..32]));

                // Check for MP4 signatures
                let is_mp4_ftyp = body_bytes.len() >= 8 && &body_bytes[4..8] == b"ftyp";
                let is_mp4_moov = body_bytes.windows(4).any(|w| w == b"moov");
                let is_mp4_mdat = body_bytes.windows(4).any(|w| w == b"mdat");

                debug_info["format_analysis"] = serde_json::json!({
                    "has_ftyp": is_mp4_ftyp,
                    "has_moov": is_mp4_moov,
                    "has_mdat": is_mp4_mdat,
                    "appears_valid_mp4": is_mp4_ftyp && (is_mp4_moov || is_mp4_mdat),
                });
            }

            (StatusCode::OK, axum::Json(debug_info))
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            axum::Json(serde_json::json!({
                "error": "Failed to get stream data",
                "details": e.to_string()
            })),
        ),
    }
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
