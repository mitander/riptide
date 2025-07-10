//! Streaming readiness endpoint
//!
//! Provides information about whether a torrent file is ready for streaming.

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::Json;
use riptide_core::torrent::InfoHash;
use serde::{Deserialize, Serialize};
use tracing::{info, warn};

use crate::server::AppState;
use crate::streaming::http_streaming::{ClientCapabilities, SimpleRangeRequest, StreamingRequest};

/// Response for streaming readiness check
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StreamingReadinessResponse {
    /// Whether the file is ready to stream immediately
    pub ready: bool,

    /// Container format of the source file
    pub container_format: String,

    /// Whether remuxing is required for this file
    pub requires_remuxing: bool,

    /// Human-readable status message
    pub message: String,

    /// File size if available
    pub file_size: Option<u64>,

    /// Rough progress estimate (0.0 - 1.0) if available
    pub progress: Option<f64>,
}

/// Handler for checking streaming readiness
pub async fn check_streaming_readiness(
    State(state): State<AppState>,
    Path(info_hash_str): Path<String>,
) -> Result<Json<StreamingReadinessResponse>, StatusCode> {
    let info_hash = InfoHash::from_hex(&info_hash_str).map_err(|_| {
        warn!("Invalid info hash format: {}", info_hash_str);
        StatusCode::BAD_REQUEST
    })?;

    info!("Checking streaming readiness for {}", info_hash);

    // Try to get file information through the streaming service

    // For now, we'll do a simple readiness check by trying a small test request
    let readiness_check = check_remux_readiness(&state, info_hash).await;

    // Default to unknown format - we'd need to implement format detection
    let format_str = "unknown";
    let file_size = None;

    // For this simplified version, assume all files might need remuxing
    let requires_remuxing = true;

    let (is_ready, message, progress) = match readiness_check {
        ReadinessResult::Ready => (true, "Ready to stream".to_string(), Some(1.0)),
        ReadinessResult::NotReady(msg) => (false, msg, Some(0.3)),
        ReadinessResult::Error(err) => (false, format!("Error: {err}"), None),
    };

    Ok(Json(StreamingReadinessResponse {
        ready: is_ready,
        container_format: format_str.to_string(),
        requires_remuxing,
        message,
        file_size,
        progress,
    }))
}

/// Result of readiness check
#[derive(Debug)]
enum ReadinessResult {
    Ready,
    NotReady(String),
    Error(String),
}

/// Simplified check for remux readiness using HttpStreamingService
async fn check_remux_readiness(state: &AppState, info_hash: InfoHash) -> ReadinessResult {
    // Create a small test request to check readiness
    let test_request = StreamingRequest {
        info_hash,
        range: Some(SimpleRangeRequest {
            start: 0,
            end: Some(1023),
        }),
        client_capabilities: ClientCapabilities {
            supports_mp4: true,
            supports_webm: false,
            supports_hls: false,
            user_agent: "riptide-readiness-check".to_string(),
        },
        preferred_quality: None,
        time_offset: None,
    };

    match state
        .streaming_service
        .handle_streaming_request(test_request)
        .await
    {
        Ok(response) => {
            // Check response status to determine readiness
            match response.status {
                StatusCode::OK | StatusCode::PARTIAL_CONTENT => {
                    // Success status means streaming is ready
                    ReadinessResult::Ready
                }
                StatusCode::ACCEPTED => {
                    // Accepted usually means processing in progress
                    ReadinessResult::NotReady("Processing in progress...".to_string())
                }
                StatusCode::SERVICE_UNAVAILABLE => {
                    ReadinessResult::NotReady("Service temporarily unavailable".to_string())
                }
                StatusCode::NOT_FOUND => ReadinessResult::Error("Torrent not found".to_string()),
                _ => ReadinessResult::NotReady("Preparing stream...".to_string()),
            }
        }
        Err(e) => ReadinessResult::Error(format!("Streaming error: {e}")),
    }
}
