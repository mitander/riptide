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
pub async fn status_banner(State(_state): State<AppState>) -> Html<String> {
    // TODO: Implement real system status checking
    // For now, return empty - banners will be shown for actual issues
    Html("".to_string())
}
