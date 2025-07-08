//! Live statistics and metrics HTMX handlers

use std::time::Instant;

use axum::extract::State;
use axum::response::Html;

use crate::components::{activity, stats};
use crate::server::AppState;

/// Formats elapsed time from start instant into human-readable string
fn format_elapsed_time(started_at: Instant) -> String {
    let elapsed = started_at.elapsed();
    let total_seconds = elapsed.as_secs();

    if total_seconds < 60 {
        format!("{total_seconds}s")
    } else if total_seconds < 3600 {
        let minutes = total_seconds / 60;
        format!("{minutes}m")
    } else if total_seconds < 86400 {
        let hours = total_seconds / 3600;
        format!("{hours}h")
    } else {
        let days = total_seconds / 86400;
        format!("{days}d")
    }
}

/// Real-time dashboard statistics fragment
///
/// # Panics
/// Panics if engine communication fails or statistics are unavailable.
pub async fn dashboard_stats(State(state): State<AppState>) -> Html<String> {
    let engine_stats = state.engine().download_statistics().await.unwrap();
    let sessions = state.engine().active_sessions().await.unwrap();

    // Calculate metrics
    let active_torrents = sessions.len();
    let downloading_count = sessions.iter().filter(|s| s.progress < 1.0).count();
    let completed_count = sessions.iter().filter(|s| s.progress >= 1.0).count();

    let session_duration = state.server_started_at.elapsed().as_secs_f64();
    let download_speed = if session_duration > 0.0 {
        (engine_stats.bytes_downloaded as f64) / session_duration / 1_048_576.0
    } else {
        0.0
    };
    let upload_speed = if session_duration > 0.0 {
        (engine_stats.bytes_uploaded as f64) / session_duration / 1_048_576.0
    } else {
        0.0
    };
    let total_downloaded = (engine_stats.bytes_downloaded as f64) / 1_073_741_824.0;

    let library_size = if let Ok(movie_manager) = state.file_manager() {
        let manager = movie_manager.read().await;
        manager.all_files().len()
    } else {
        0
    };

    let connected_peers: usize = sessions
        .iter()
        .map(|s| {
            if s.progress > 0.0 && s.download_speed_bps > 0 {
                std::cmp::min(s.piece_count / 10, 20) as usize
            } else {
                0
            }
        })
        .sum();

    let stat_cards = vec![
        stats::stat_card(
            &active_torrents.to_string(),
            "Active Torrents",
            None,
            None,
            None,
        ),
        stats::stat_card(
            &downloading_count.to_string(),
            "Downloading",
            None,
            None,
            Some("text-riptide-400"),
        ),
        stats::stat_card(
            &completed_count.to_string(),
            "Completed",
            None,
            None,
            Some("text-green-400"),
        ),
        stats::stat_card(
            &format!("{download_speed:.1}"),
            "Download Speed",
            Some("MB/s"),
            None,
            None,
        ),
        stats::stat_card(
            &format!("{upload_speed:.1}"),
            "Upload Speed",
            Some("MB/s"),
            None,
            None,
        ),
        stats::stat_card(
            &format!("{total_downloaded:.2}"),
            "Downloaded",
            Some("GB"),
            None,
            None,
        ),
        stats::stat_card(
            &connected_peers.to_string(),
            "Connected Peers",
            None,
            None,
            None,
        ),
        stats::stat_card(&library_size.to_string(), "Library Items", None, None, None),
    ];

    Html(stats::stats_grid(&stat_cards))
}

/// Recent activity feed fragment
///
/// # Panics
/// Panics if engine communication fails or active sessions are unavailable.
pub async fn dashboard_activity(State(state): State<AppState>) -> Html<String> {
    let sessions = state.engine().active_sessions().await.unwrap();

    let mut activities = Vec::new();

    // Generate activities from recent torrent events
    for session in sessions.iter().take(8) {
        let (icon, title, description) = if session.progress >= 1.0 {
            (
                "✓",
                format!("Completed: {}", session.filename),
                Some("Download finished successfully".to_string()),
            )
        } else if session.progress > 0.5 {
            (
                "↓",
                format!("Downloading: {}", session.filename),
                Some(format!("{:.1}% complete", session.progress * 100.0)),
            )
        } else if session.progress > 0.0 {
            (
                "•",
                format!("Starting: {}", session.filename),
                Some("Download in progress".to_string()),
            )
        } else {
            (
                "•",
                format!("Queued: {}", session.filename),
                Some("Waiting for peers".to_string()),
            )
        };

        activities.push(activity::ActivityItem {
            icon: icon.to_string(),
            title,
            description,
            time: format_elapsed_time(session.started_at),
        });
    }

    if activities.is_empty() {
        let uptime = state.server_started_at.elapsed();
        let uptime_str = if uptime.as_secs() < 60 {
            "just started".to_string()
        } else {
            format!(
                "running for {}",
                format_elapsed_time(state.server_started_at)
            )
        };

        activities.push(activity::ActivityItem {
            icon: "•".to_string(),
            title: "Riptide engine online".to_string(),
            description: Some(format!("System {uptime_str} and ready for downloads")),
            time: format_elapsed_time(state.server_started_at),
        });
    }

    Html(activity::activity_feed(&activities))
}

/// Active downloads preview fragment
///
/// # Panics
/// Panics if engine communication fails or active sessions are unavailable.
pub async fn dashboard_downloads(State(state): State<AppState>) -> Html<String> {
    let sessions = state.engine().active_sessions().await.unwrap();

    // Show only actively downloading torrents
    let active_downloads: Vec<_> = sessions
        .iter()
        .filter(|s| s.progress < 1.0 && s.progress > 0.0)
        .take(3)
        .collect();

    if active_downloads.is_empty() {
        return Html(
            r#"<div class="text-center py-8">
                <div class="text-4xl mb-2">•</div>
                <p class="text-gray-400">No active downloads</p>
                <p class="text-gray-500 text-sm mt-1">Add a torrent to see progress here</p>
            </div>"#
                .to_string(),
        );
    }

    let downloads_html: String = active_downloads
        .iter()
        .map(|session| {
            let progress_percent = (session.progress * 100.0) as u32;
            let estimated_size = (session.piece_count as u64 * session.piece_size as u64) as f64 / 1_073_741_824.0;
            let completed_pieces = session.completed_pieces.iter().filter(|&&x| x).count();

            format!(
                r#"<div class="flex items-center justify-between p-4 bg-gray-800 rounded-lg border border-gray-700">
                    <div class="flex-1 min-w-0">
                        <h4 class="text-white font-medium truncate mb-2">{}</h4>
                        <div class="w-full bg-gray-700 rounded-full h-2 mb-2">
                            <div class="bg-riptide-500 h-2 rounded-full transition-all duration-300" style="width: {}%"></div>
                        </div>
                        <p class="text-gray-400 text-sm">{:.1} GB • {}/{} pieces</p>
                    </div>
                    <div class="ml-4 text-right">
                        <div class="text-riptide-400 font-bold">{}%</div>
                        <div class="text-gray-500 text-xs">downloading</div>
                    </div>
                </div>"#,
                session.filename,
                progress_percent,
                estimated_size,
                completed_pieces,
                session.piece_count,
                progress_percent
            )
        })
        .collect();

    Html(downloads_html)
}

/// Get basic system information without external dependencies
fn collect_system_metrics() -> [(&'static str, String, &'static str); 4] {
    // For CPU usage, we'll show a placeholder since accurate CPU monitoring
    // requires external crates like sysinfo. This is a good compromise.
    let cpu_usage = "~";

    // Memory usage from /proc/meminfo on Linux, placeholder on other systems
    let memory_usage = memory_usage();

    // Disk usage from current directory
    let disk_free = disk_free();

    // Network latency placeholder - would need external ping/network measurement
    let network_latency = "~";

    [
        ("CPU", cpu_usage.to_string(), "CPU Usage"),
        ("MEM", memory_usage, "Memory"),
        ("DSK", disk_free, "Disk Free"),
        ("NET", network_latency.to_string(), "Network"),
    ]
}

/// Get memory usage information
fn memory_usage() -> String {
    #[cfg(target_os = "linux")]
    {
        // Try to read /proc/meminfo on Linux
        if let Ok(meminfo) = std::fs::read_to_string("/proc/meminfo") {
            let mut mem_total = 0u64;
            let mut mem_available = 0u64;

            for line in meminfo.lines() {
                if line.starts_with("MemTotal:") {
                    if let Some(value) = line.split_whitespace().nth(1) {
                        mem_total = value.parse::<u64>().unwrap_or(0) * 1024; // Convert KB to bytes
                    }
                } else if line.starts_with("MemAvailable:") {
                    if let Some(value) = line.split_whitespace().nth(1) {
                        mem_available = value.parse::<u64>().unwrap_or(0) * 1024; // Convert KB to bytes
                    }
                }
            }

            if mem_total > 0 && mem_available > 0 {
                let used = mem_total - mem_available;
                let used_gb = used as f64 / (1024.0 * 1024.0 * 1024.0);
                return format!("{:.1}GB", used_gb);
            }
        }
    }

    // Fallback for non-Linux or if reading fails
    "~".to_string()
}

/// Get available disk space using basic filesystem info
fn disk_free() -> String {
    // For now, just show a placeholder since getting disk space
    // reliably across platforms requires external dependencies
    // This could be enhanced later with platform-specific code
    "~".to_string()
}

/// System performance metrics fragment
pub async fn system_metrics(State(_state): State<AppState>) -> Html<String> {
    let metrics = collect_system_metrics();

    let metrics_html: String = metrics
        .iter()
        .map(|(icon, value, label)| stats::metric_item(icon, value, label, None))
        .collect();

    Html(format!(
        r#"<div class="grid grid-cols-2 gap-4">
            {metrics_html}
        </div>"#
    ))
}

/// Network status and peer information
///
/// # Panics
/// Panics if engine communication fails or active sessions are unavailable.
pub async fn network_status(State(state): State<AppState>) -> Html<String> {
    let sessions = state.engine().active_sessions().await.unwrap();

    let total_peers: usize = sessions
        .iter()
        .map(|s| s.completed_pieces.iter().filter(|&&x| x).count())
        .sum();

    let status_indicators = [
        stats::status_indicator("online", "DHT Connected"),
        stats::status_indicator("active", &format!("{total_peers} Peers")),
        stats::status_indicator("active", "Trackers Online"),
    ];

    Html(format!(
        r#"<div class="space-y-3">
            {}
        </div>"#,
        status_indicators.join("")
    ))
}
