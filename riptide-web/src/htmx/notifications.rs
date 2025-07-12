//! Notification and status HTMX handlers

use axum::extract::State;
use axum::response::Html;

use crate::components::{activity, stats};
use crate::server::AppState;

/// Renders a toast notification
pub async fn toast_notification(message: String, toast_type: String) -> Html<String> {
    Html(activity::notification_toast(&message, &toast_type, true))
}

/// Renders system status information
///
/// # Panics
///
/// Panics if engine communication fails or active sessions are unavailable.
pub async fn system_status(State(state): State<AppState>) -> Html<String> {
    let sessions = state.engine().active_sessions().await.unwrap();
    let active_count = sessions.len();

    let status_indicators = [
        stats::status_indicator("online", "BitTorrent Engine"),
        stats::status_indicator(
            if active_count > 0 { "active" } else { "online" },
            &format!("Network ({active_count} connections)"),
        ),
        stats::status_indicator("active", "Storage Available"),
        stats::status_indicator("online", "Trackers Connected"),
    ];

    Html(format!(
        r#"<div class="space-y-3">
            {}
        </div>"#,
        status_indicators.join("")
    ))
}

/// Renders a live ticker with current stats
///
/// # Panics
///
/// Panics if engine communication fails or active sessions are unavailable.
pub async fn live_ticker(State(state): State<AppState>) -> Html<String> {
    let sessions = state.engine().active_sessions().await.unwrap();

    let current_download_speed: u64 = sessions.iter().map(|s| s.download_speed_bps).sum();
    let current_upload_speed: u64 = sessions.iter().map(|s| s.upload_speed_bps).sum();

    let download_speed = current_download_speed as f64 / 1_048_576.0;
    let upload_speed = current_upload_speed as f64 / 1_048_576.0;
    let active_torrents = sessions.len();
    let downloading = sessions
        .iter()
        .filter(|s| s.progress < 1.0 && s.progress > 0.0)
        .count();

    let ticker_stats = vec![
        activity::TickerStat {
            icon: "↓".to_string(),
            value: format!("{download_speed:.1} MB/s"),
            label: "Download".to_string(),
        },
        activity::TickerStat {
            icon: "↑".to_string(),
            value: format!("{upload_speed:.1} MB/s"),
            label: "Upload".to_string(),
        },
        activity::TickerStat {
            icon: "•".to_string(),
            value: active_torrents.to_string(),
            label: "Active".to_string(),
        },
        activity::TickerStat {
            icon: "•".to_string(),
            value: downloading.to_string(),
            label: "Downloading".to_string(),
        },
    ];

    Html(activity::live_ticker(&ticker_stats))
}

/// Renders global status banner for important system messages
///
/// # Panics
///
/// Panics if engine communication fails or statistics are unavailable.
pub async fn status_banner(State(state): State<AppState>) -> Html<String> {
    let sessions = state.engine().active_sessions().await.unwrap();
    let engine_stats = state.engine().download_statistics().await.unwrap();

    let mut messages = Vec::new();

    let slow_threshold_bps = 100_000;

    let stalled_count = sessions
        .iter()
        .filter(|s| s.progress > 0.0 && s.progress < 1.0 && s.download_speed_bps == 0)
        .count();
    if stalled_count > 0 {
        messages.push(("warning", format!("{stalled_count} downloads stalled")));
    }

    let slow_count = sessions
        .iter()
        .filter(|s| {
            s.progress > 0.0
                && s.progress < 1.0
                && s.download_speed_bps > 0
                && s.download_speed_bps < slow_threshold_bps
        })
        .count();
    if slow_count > 0 {
        messages.push(("info", format!("{slow_count} downloads are slow")));
    }

    // Check for completed downloads
    let completed_count = sessions.iter().filter(|s| s.progress >= 1.0).count();
    if completed_count > 0 {
        messages.push(("success", format!("{completed_count} downloads completed")));
    }

    if engine_stats.bytes_downloaded > 10_000_000 {
        let session_duration = state.server_started_at.elapsed();
        let gb_downloaded = engine_stats.bytes_downloaded as f64 / 1_073_741_824.0;
        let hours = session_duration.as_secs() / 3600;
        let minutes = (session_duration.as_secs() % 3600) / 60;

        let time_str = if hours > 0 {
            format!("{hours}h {minutes}m")
        } else {
            format!("{minutes}m")
        };

        messages.push((
            "success",
            format!("{gb_downloaded:.1} GB downloaded in {time_str}"),
        ));
    }

    if messages.is_empty() {
        Html("".to_string())
    } else {
        let banners = messages
            .iter()
            .map(|(level, message)| {
                format!(
                    r#"<div class="bg-{level}-100 border-l-4 border-{level}-500 text-{level}-700 p-4 mb-2">
                    <p class="font-medium">{message}</p>
                </div>"#
                )
            })
            .collect::<Vec<_>>()
            .join("");
        Html(banners)
    }
}
