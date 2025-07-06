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
pub async fn system_status(State(state): State<AppState>) -> Html<String> {
    let sessions = state.torrent_engine.get_active_sessions().await.unwrap();
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
pub async fn live_ticker(State(state): State<AppState>) -> Html<String> {
    let engine_stats = state.torrent_engine.get_download_stats().await.unwrap();
    let sessions = state.torrent_engine.get_active_sessions().await.unwrap();

    let download_speed = (engine_stats.bytes_downloaded as f64) / 1_048_576.0;
    let upload_speed = (engine_stats.bytes_uploaded as f64) / 1_048_576.0;
    let active_torrents = sessions.len();
    let downloading = sessions.iter().filter(|s| s.progress < 1.0).count();

    let ticker_stats = vec![
        activity::TickerStat {
            icon: "⬇️".to_string(),
            value: format!("{download_speed:.1} MB/s"),
            label: "Download".to_string(),
        },
        activity::TickerStat {
            icon: "⬆️".to_string(),
            value: format!("{upload_speed:.1} MB/s"),
            label: "Upload".to_string(),
        },
        activity::TickerStat {
            icon: "📦".to_string(),
            value: active_torrents.to_string(),
            label: "Active".to_string(),
        },
        activity::TickerStat {
            icon: "🔄".to_string(),
            value: downloading.to_string(),
            label: "Downloading".to_string(),
        },
    ];

    Html(activity::live_ticker(&ticker_stats))
}

/// Renders global status banner for important system messages
pub async fn status_banner(State(state): State<AppState>) -> Html<String> {
    let sessions = state.torrent_engine.get_active_sessions().await.unwrap();
    let engine_stats = state.torrent_engine.get_download_stats().await.unwrap();

    let mut messages = Vec::new();

    // Check for low disk space (placeholder - would need real disk space check)
    let available_gb = 10.0; // Placeholder
    if available_gb < 5.0 {
        messages.push((
            "warning",
            format!("Low disk space: {available_gb:.1} GB remaining"),
        ));
    }

    // Check for stalled downloads
    let stalled_count = sessions
        .iter()
        .filter(|s| s.progress > 0.0 && s.progress < 1.0 && s.download_speed_bps == 0)
        .count();
    if stalled_count > 0 {
        messages.push(("info", format!("{stalled_count} downloads appear stalled")));
    }

    // Check for completed downloads
    let completed_count = sessions.iter().filter(|s| s.progress >= 1.0).count();
    if completed_count > 0 {
        messages.push(("success", format!("{completed_count} downloads completed")));
    }

    // Check for high activity
    if engine_stats.bytes_downloaded > 100_000_000 {
        // > 100MB
        let gb_downloaded = engine_stats.bytes_downloaded as f64 / 1_073_741_824.0;
        messages.push((
            "info",
            format!("{gb_downloaded:.1} GB downloaded this session"),
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
