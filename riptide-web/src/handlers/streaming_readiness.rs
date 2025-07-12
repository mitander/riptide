//! Streaming readiness checking for torrent media
//!
//! Evaluates whether torrents are ready for streaming by checking download progress,
//! buffer status, and remux readiness. Provides detailed progress information for
//! frontend loading indicators.

use axum::extract::{Path, State};
use axum::response::Json;
use serde::{Deserialize, Serialize};
use tracing::info;

use crate::server::AppState;

/// Response structure for streaming readiness checks
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StreamingReadinessResponse {
    /// Whether the torrent is ready for streaming
    pub ready: bool,
    /// Container format of the media file
    pub container_format: String,
    /// Whether remuxing is required for streaming
    pub requires_remuxing: bool,
    /// Descriptive message about readiness status
    pub message: String,
    /// Total file size in bytes if known
    pub file_size: Option<u64>,
    /// Download/buffer progress as a fraction (0.0-1.0)
    pub progress: Option<f64>,
}

/// Internal enum for readiness check results
#[derive(Debug)]
enum ReadinessResult {
    /// Stream is ready for playback
    Ready,
    /// Stream is not ready with reason
    NotReady(String),
    /// Error occurred during readiness check
    #[allow(dead_code)]
    Error(String),
}

/// Checks if torrent stream is ready for playback.
///
/// Evaluates torrent download progress, buffer health, and remux status to determine
/// if streaming can begin. Returns readiness status with detailed progress information
/// for the frontend to display loading indicators.
///
/// # Arguments
/// * `state` - Application state containing torrent engine and streaming service
/// * `hash` - Torrent info hash as hex string
///
/// # Returns
/// JSON response with readiness status, progress percentage, and descriptive message.
/// Always returns 200 OK with readiness information in the response body.
///
/// # Examples
/// ```ignore
/// // HTTP request: GET /stream/abc123def456.../ready
/// // Response: { "ready": false, "progress": 0.45, "message": "Buffer: 1.8MB/2.0MB" }
/// ```
pub async fn streaming_readiness_handler(
    State(state): State<AppState>,
    Path(hash): Path<String>,
) -> Result<Json<StreamingReadinessResponse>, axum::http::StatusCode> {
    info!("Checking streaming readiness for {}", hash);

    // Parse info hash first
    let info_hash = match hash.parse() {
        Ok(hash) => hash,
        Err(_) => {
            return Ok(Json(StreamingReadinessResponse {
                ready: false,
                container_format: "unknown".to_string(),
                requires_remuxing: true,
                message: "Invalid info hash".to_string(),
                file_size: None,
                progress: None,
            }));
        }
    };

    // Get buffer status from torrent engine
    let buffer_status = match state.engine().buffer_status(info_hash).await {
        Ok(status) => status,
        Err(_) => {
            return Ok(Json(StreamingReadinessResponse {
                ready: false,
                container_format: "unknown".to_string(),
                requires_remuxing: true,
                message: "Torrent not found or not active".to_string(),
                file_size: None,
                progress: None,
            }));
        }
    };

    let readiness_check = check_streaming_readiness(&state, info_hash, &buffer_status).await;

    let (is_ready, message, progress) = match readiness_check {
        ReadinessResult::Ready => (true, "Ready to stream".to_string(), Some(1.0)),
        ReadinessResult::NotReady(msg) => {
            // Calculate progress based on buffer status
            let buffer_progress = if buffer_status.bytes_ahead > 0 {
                let target_buffer = 2 * 1024 * 1024; // 2MB target
                (buffer_status.bytes_ahead as f64 / target_buffer as f64).min(0.8)
            } else {
                0.1
            };

            let health_progress = buffer_status.buffer_health * 0.5;
            let duration_progress = if buffer_status.buffer_duration > 0.0 {
                (buffer_status.buffer_duration / 5.0).min(0.3)
            } else {
                0.0
            };

            let total_progress = (buffer_progress + health_progress + duration_progress).min(0.9);
            (false, msg, Some(total_progress.max(0.1)))
        }
        ReadinessResult::Error(err) => (false, format!("Error: {err}"), None),
    };

    Ok(Json(StreamingReadinessResponse {
        ready: is_ready,
        container_format: "unknown".to_string(),
        requires_remuxing: true,
        message,
        file_size: None,
        progress,
    }))
}

/// Internal function to evaluate streaming readiness based on multiple criteria
///
/// Checks buffer status, remux readiness, and file metadata to determine if
/// streaming can begin. Uses configurable thresholds for buffer size, health,
/// and duration requirements.
async fn check_streaming_readiness(
    state: &AppState,
    info_hash: riptide_core::torrent::InfoHash,
    buffer_status: &riptide_core::torrent::BufferStatus,
) -> ReadinessResult {
    // Try to get actual file size from torrent engine
    let _estimated_file_size = match state.engine().download_statistics().await {
        Ok(_stats) => {
            // Try to get file size from torrent metadata if available
            // For now, use a conservative estimate based on buffer status
            if buffer_status.current_position > 0 {
                // If we have position info, estimate based on that
                (buffer_status.current_position + buffer_status.bytes_ahead * 10)
                    .max(50 * 1024 * 1024)
            } else {
                // Conservative 50MB estimate for initial streaming
                50 * 1024 * 1024
            }
        }
        Err(_) => {
            // Very conservative estimate when we can't get stats
            10 * 1024 * 1024 // 10MB
        }
    };

    // Check remux streaming readiness - make this less strict initially
    let remux_ready = state
        .http_streaming
        .check_stream_readiness(info_hash)
        .await
        .map(|status| status.readiness == riptide_core::streaming::StreamReadiness::Ready)
        .unwrap_or(false);

    // More realistic buffer requirements for early streaming
    const MIN_BUFFER_SIZE: u64 = 2 * 1024 * 1024; // 2MB minimum buffer (reduced from 5MB)
    const MIN_BUFFER_HEALTH: f64 = 0.15; // 15% buffer health minimum (reduced from 30%)
    const MIN_BUFFER_DURATION: f64 = 5.0; // 5 seconds of content minimum (reduced from 10s)

    // Check multiple readiness criteria
    let has_sufficient_buffer = buffer_status.bytes_ahead >= MIN_BUFFER_SIZE;
    let has_good_health = buffer_status.buffer_health >= MIN_BUFFER_HEALTH;
    let has_sufficient_duration = buffer_status.buffer_duration >= MIN_BUFFER_DURATION;

    // For remux streaming, remux readiness is the primary requirement
    // Buffer requirements are less critical since we're serving from transcoded file
    let stream_ready = if remux_ready {
        // If remux is ready, we can stream regardless of original buffer status
        true
    } else {
        // If remux isn't ready, fall back to buffer requirements
        has_sufficient_buffer && (has_good_health || has_sufficient_duration)
    };

    if stream_ready {
        ReadinessResult::Ready
    } else {
        // Provide specific feedback about what's missing
        let mut missing_reasons = Vec::new();

        // Metadata is always required for proper streaming
        if !remux_ready {
            missing_reasons.push("Preparing stream metadata".to_string());
        }

        if !has_sufficient_buffer {
            let current_mb = buffer_status.bytes_ahead as f64 / 1024.0 / 1024.0;
            missing_reasons.push(format!(
                "Buffer: {:.1}MB/{:.1}MB",
                current_mb,
                MIN_BUFFER_SIZE as f64 / 1024.0 / 1024.0
            ));
        }

        if !has_good_health && buffer_status.buffer_health > 0.0 {
            missing_reasons.push(format!(
                "Buffer health: {:.1}%",
                buffer_status.buffer_health * 100.0
            ));
        }

        if !has_sufficient_duration && buffer_status.buffer_duration > 0.0 {
            missing_reasons.push(format!(
                "Duration: {:.1}s/{:.1}s",
                buffer_status.buffer_duration, MIN_BUFFER_DURATION
            ));
        }

        let message = if missing_reasons.is_empty() {
            "Building buffer for smooth streaming...".to_string()
        } else {
            format!("Building buffer: {}", missing_reasons.join(", "))
        };

        ReadinessResult::NotReady(message)
    }
}
