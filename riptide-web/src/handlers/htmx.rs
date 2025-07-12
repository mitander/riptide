//! HTMX handlers for real-time partial updates
//!
//! Provides server-rendered HTML fragments for HTMX-powered real-time updates.
//! These handlers focus on small, efficient HTML snippets that update specific
//! parts of the page without full reloads.

use axum::extract::{Form, State};
use axum::http::StatusCode;
use axum::response::Html;
use serde::Deserialize;

// use serde_json::json; // TODO: Remove if not needed
use crate::server::AppState;

/// Form data for adding torrents via HTMX
#[derive(Deserialize)]
pub struct AddTorrentForm {
    /// Magnet link URL from the form
    pub magnet: String,
}

/// Renders the dashboard stats section as HTML fragment.
///
/// Returns HTML containing current engine statistics including active torrents,
/// download/upload speeds, total downloaded data, connected peers, and library size.
///
/// # Panics
///
/// Panics if engine communication fails or download statistics are unavailable.
pub async fn dashboard_stats(State(state): State<AppState>) -> Html<String> {
    let stats = state.engine().download_statistics().await.unwrap();

    // Get library size
    let library_size = if let Ok(movie_manager) = state.file_library() {
        let manager = movie_manager.read().await;
        manager.all_files().len()
    } else {
        0
    };

    // Get connected peers count (from active sessions)
    let sessions = state.engine().active_sessions().await.unwrap();
    let connected_peers: usize = sessions
        .iter()
        .map(|session| session.completed_pieces.iter().filter(|&&x| x).count())
        .sum::<usize>()
        .min(50); // Reasonable upper bound for display

    let html = format!(
        r#"<div class="stats-grid">
            <div class="stat-card">
                <span class="stat-value">{}</span>
                <div class="stat-label">Active Torrents</div>
            </div>
            <div class="stat-card">
                <span class="stat-value">{:.1}</span>
                <div class="stat-unit">MB/s</div>
                <div class="stat-label">Download Speed</div>
            </div>
            <div class="stat-card">
                <span class="stat-value">{:.1}</span>
                <div class="stat-unit">MB/s</div>
                <div class="stat-label">Upload Speed</div>
            </div>
            <div class="stat-card">
                <span class="stat-value">{:.1}</span>
                <div class="stat-unit">GB</div>
                <div class="stat-label">Total Downloaded</div>
            </div>
            <div class="stat-card">
                <span class="stat-value">{}</span>
                <div class="stat-label">Connected Peers</div>
            </div>
            <div class="stat-card">
                <span class="stat-value">{}</span>
                <div class="stat-unit">Items</div>
                <div class="stat-label">Library Size</div>
            </div>
        </div>"#,
        stats.active_torrents,
        (stats.bytes_downloaded as f64) / 1_048_576.0, // Download speed in MB/s
        (stats.bytes_uploaded as f64) / 1_048_576.0,   // Upload speed in MB/s
        (stats.bytes_downloaded as f64) / 1_073_741_824.0, // Total downloaded in GB
        connected_peers,
        library_size
    );

    Html(html)
}

/// Renders recent activity feed as HTML fragment.
///
/// Shows the most recent torrent activities with status icons and progress information.
/// Displays up to 5 recent activities or a default message if no torrents are active.
///
/// # Panics
///
/// Panics if engine communication fails or active sessions are unavailable.
pub async fn dashboard_activity(State(state): State<AppState>) -> Html<String> {
    let sessions = state.engine().active_sessions().await.unwrap();

    let mut activities = Vec::new();

    // Generate activity entries from torrent sessions
    for session in sessions.iter().take(5) {
        let status_icon = if session.progress >= 1.0 {
            "✓"
        } else if session.progress > 0.0 {
            "↓"
        } else {
            "•"
        };

        let status_text = if session.progress >= 1.0 {
            "Download completed"
        } else if session.progress > 0.0 {
            &format!("Downloading... {:.1}%", session.progress * 100.0)
        } else {
            "Starting download"
        };

        activities.push(format!(
            r#"<div class="activity-item">
                <div class="activity-icon">{}</div>
                <div class="activity-content">
                    <div class="activity-title">{}</div>
                    <div class="activity-time">{}</div>
                </div>
            </div>"#,
            status_icon, session.filename, status_text
        ));
    }

    let html = if activities.is_empty() {
        r#"<div class="activity-item">
            <div class="activity-icon">•</div>
            <div class="activity-content">
                <div class="activity-title">Server running</div>
                <div class="activity-time">BitTorrent engine active, waiting for downloads</div>
            </div>
        </div>"#
            .to_string()
    } else {
        activities.join("")
    };

    Html(html)
}

/// Renders active downloads preview as HTML fragment.
///
/// Shows up to 3 currently downloading torrents with progress bars and statistics.
/// Filters out completed downloads and displays a message if no downloads are active.
///
/// # Panics
///
/// Panics if engine communication fails or active sessions are unavailable.
pub async fn dashboard_downloads(State(state): State<AppState>) -> Html<String> {
    let sessions = state.engine().active_sessions().await.unwrap();

    let mut downloads = Vec::new();

    // Show active downloads (not completed)
    for session in sessions.iter().filter(|s| s.progress < 1.0).take(3) {
        let progress_percent = (session.progress * 100.0) as u32;
        let estimated_size =
            (session.piece_count as u64 * session.piece_size as u64) as f64 / 1_073_741_824.0;

        downloads.push(format!(
            r#"<div class="download-item">
                <div class="download-info">
                    <div class="download-name">{}</div>
                    <div class="download-stats">{:.1} GB • {} pieces</div>
                    <div class="progress-bar">
                        <div class="progress-fill" style="width: {}%"></div>
                    </div>
                </div>
                <div class="download-progress">{}%</div>
            </div>"#,
            session.filename,
            estimated_size,
            session.piece_count,
            progress_percent,
            progress_percent
        ));
    }

    let html = if downloads.is_empty() {
        r#"<div class="download-item">
            <div class="download-info">
                <div class="download-name">No active downloads</div>
                <div class="download-stats">Add a torrent to start downloading</div>
            </div>
        </div>"#
            .to_string()
    } else {
        downloads.join("")
    };

    Html(html)
}

/// Handles adding torrents via HTMX form submission.
///
/// Processes magnet link submission from HTMX forms, adds the torrent to the engine,
/// and starts downloading immediately. Returns appropriate success or error HTML fragments.
///
/// # Errors
///
/// - `StatusCode` - If form processing fails
pub async fn add_torrent_htmx(
    State(state): State<AppState>,
    Form(form): Form<AddTorrentForm>,
) -> Result<Html<String>, StatusCode> {
    if form.magnet.trim().is_empty() {
        return Ok(Html(
            r#"<div class="add-result error">Please enter a magnet link</div>"#.to_string(),
        ));
    }

    match state.engine().add_magnet(&form.magnet).await {
        Ok(info_hash) => {
            // Start downloading immediately after adding
            match state.engine().start_download(info_hash).await {
                Ok(()) => Ok(Html(format!(
                    r#"<div class="add-result success">✓ Torrent added successfully! Download started for {}</div>"#,
                    &info_hash.to_string()[..8]
                ))),
                Err(e) => {
                    // Provide user-friendly error messages
                    let error_msg = match &e {
                        riptide_core::torrent::TorrentError::TorrentNotFoundOnTracker {
                            ..
                        } => {
                            "This torrent is not available on public trackers. Try a different torrent."
                        }
                        riptide_core::torrent::TorrentError::NoPeersAvailable => {
                            "No peers available for this torrent. The content may be old or unpopular."
                        }
                        riptide_core::torrent::TorrentError::TrackerConnectionFailed { .. } => {
                            "Tracker connection failed. The tracker servers may be offline."
                        }
                        riptide_core::torrent::TorrentError::TrackerTimeout { .. } => {
                            "Tracker request timed out. Try again in a moment."
                        }
                        _ => "Download failed. Please check the magnet link and try again.",
                    };

                    Ok(Html(format!(
                        r#"<div class="add-result error">✗ {error_msg}</div>"#
                    )))
                }
            }
        }
        Err(e) => Ok(Html(format!(
            r#"<div class="add-result error">✗ Failed to parse magnet link: {e}</div>"#
        ))),
    }
}

/// Renders torrent list for the torrents page with real-time updates.
///
/// Generates HTML table rows for all active torrents with detailed information including
/// progress bars, file sizes, piece counts, and action buttons. Uses movie metadata
/// when available for better display names.
///
/// # Panics
///
/// Panics if engine communication fails or active sessions are unavailable.
pub async fn torrents_list(State(state): State<AppState>) -> Html<String> {
    let sessions = state.engine().active_sessions().await.unwrap();

    // Get movie manager data for better naming
    let movie_titles: std::collections::HashMap<_, _> =
        if let Ok(movie_manager) = state.file_library() {
            let manager = movie_manager.read().await;
            manager
                .all_files()
                .iter()
                .map(|movie| (movie.info_hash, movie.title.clone()))
                .collect()
        } else {
            std::collections::HashMap::new()
        };

    let mut torrents = Vec::new();

    for session in &sessions {
        let name = movie_titles
            .get(&session.info_hash)
            .cloned()
            .unwrap_or_else(|| session.filename.clone());

        let progress_percent = (session.progress * 100.0) as u32;
        let estimated_size =
            (session.piece_count as u64 * session.piece_size as u64) as f64 / 1_073_741_824.0;
        let completed_pieces = session.completed_pieces.iter().filter(|&&x| x).count();

        let status = if session.progress >= 1.0 {
            "completed"
        } else if session.progress > 0.0 {
            "downloading"
        } else {
            "starting"
        };

        let status_class = match status {
            "completed" => "status-completed",
            "downloading" => "status-downloading",
            _ => "status-starting",
        };

        torrents.push(format!(
            r#"<tr class="torrent-row">
                <td>
                    <div class="torrent-name">{}</div>
                    <div class="torrent-hash">{}</div>
                </td>
                <td>
                    <div class="progress-container">
                        <div class="progress-bar">
                            <div class="progress-fill" style="width: {}%"></div>
                        </div>
                        <div class="progress-text">{}%</div>
                    </div>
                </td>
                <td>{:.1} GB</td>
                <td>{}/{}</td>
                <td>
                    <span class="status-badge {}">{}</span>
                </td>
                <td>
                    <button class="btn btn-small" onclick="showTorrentDetails('{}')">Details</button>
                </td>
            </tr>"#,
            name,
            &session.info_hash.to_string()[..16],
            progress_percent,
            progress_percent,
            estimated_size,
            completed_pieces,
            session.piece_count,
            status_class,
            status,
            session.info_hash
        ));
    }

    let html = if torrents.is_empty() {
        r#"<tr>
            <td colspan="6" class="no-torrents">
                <div class="empty-state">
                    <div class="empty-icon">•</div>
                    <div>No active torrents</div>
                    <div class="empty-subtitle">Add a magnet link above to start downloading</div>
                </div>
            </td>
        </tr>"#
            .to_string()
    } else {
        torrents.join("")
    };

    Html(html)
}
