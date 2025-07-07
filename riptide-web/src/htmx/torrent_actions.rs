//! Torrent management HTMX handlers

use axum::extract::{Form, State};
use axum::response::Html;
use serde::Deserialize;

use crate::components::{activity, torrent};
use crate::server::AppState;

/// Calculates estimated time to completion for a download.
fn calculate_eta(progress: f32, download_speed_bps: u64, total_size: u64) -> Option<String> {
    if progress >= 1.0 || download_speed_bps == 0 {
        return None;
    }

    let remaining_bytes = total_size - (total_size as f32 * progress) as u64;
    let eta_seconds = remaining_bytes as f64 / download_speed_bps as f64;

    if eta_seconds > 86400.0 {
        // More than a day
        Some(format!("{:.0}d", eta_seconds / 86400.0))
    } else if eta_seconds > 3600.0 {
        // More than an hour
        Some(format!("{:.0}h", eta_seconds / 3600.0))
    } else if eta_seconds > 60.0 {
        // More than a minute
        Some(format!("{:.0}m", eta_seconds / 60.0))
    } else {
        // Less than a minute
        Some(format!("{eta_seconds:.0}s"))
    }
}

/// Form data for adding torrents
#[derive(Deserialize)]
pub struct AddTorrentForm {
    pub magnet: String,
}

/// Handles adding torrents via HTMX form submission
pub async fn add_torrent(
    State(state): State<AppState>,
    Form(form): Form<AddTorrentForm>,
) -> Html<String> {
    if form.magnet.trim().is_empty() {
        return Html(activity::notification_toast(
            "Please enter a magnet link",
            "error",
            true,
        ));
    }

    match state.engine().add_magnet(&form.magnet).await {
        Ok(info_hash) => {
            // Start downloading immediately after adding
            match state.engine().start_download(info_hash).await {
                Ok(()) => Html(activity::notification_toast(
                    &format!(
                        "✅ Torrent added successfully! Download started for {}",
                        &info_hash.to_string()[..8]
                    ),
                    "success",
                    true,
                )),
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

                    Html(activity::notification_toast(
                        &format!("❌ {error_msg}"),
                        "error",
                        true,
                    ))
                }
            }
        }
        Err(e) => Html(activity::notification_toast(
            &format!("❌ Failed to parse magnet link: {e}"),
            "error",
            true,
        )),
    }
}

/// Renders torrent list for the torrents page with real-time updates
pub async fn torrent_list(State(state): State<AppState>) -> Html<String> {
    let sessions = state.engine().active_sessions().await.unwrap();

    // Get movie manager data for better naming
    let movie_titles: std::collections::HashMap<_, _> =
        if let Ok(movie_manager) = state.file_manager() {
            let manager = movie_manager.read().await;
            manager
                .all_files()
                .iter()
                .map(|movie| (movie.info_hash, movie.title.clone()))
                .collect()
        } else {
            std::collections::HashMap::new()
        };

    if sessions.is_empty() {
        return Html(torrent::torrents_empty_state());
    }

    let torrents_html: String = sessions
        .iter()
        .map(|session| {
            let name = movie_titles
                .get(&session.info_hash)
                .cloned()
                .unwrap_or_else(|| session.filename.clone());

            let progress_percent = (session.progress * 100.0) as u32;
            let estimated_size =
                (session.piece_count as u64 * session.piece_size as u64) as f64 / 1_073_741_824.0;

            let status = if session.progress >= 1.0 {
                "completed"
            } else if session.progress > 0.0 {
                "downloading"
            } else {
                "queued"
            };

            let speed_formatted = session.download_speed_formatted();
            let speed = if session.download_speed_bps > 0 {
                Some(speed_formatted.as_str())
            } else {
                None
            };

            let eta_str = calculate_eta(
                session.progress,
                session.download_speed_bps,
                session.total_size,
            );
            let eta = eta_str.as_deref();

            torrent::torrent_list_item(torrent::TorrentListItemParams {
                name: &name,
                info_hash: &session.info_hash.to_string(),
                progress: progress_percent,
                size: &format!("{estimated_size:.1} GB"),
                speed,
                status,
                eta,
            })
        })
        .collect();

    Html(format!(
        r#"<div class="space-y-4">
            {torrents_html}
        </div>"#
    ))
}

/// Renders torrent details modal
///
/// # Panics
/// Panics if engine communication fails or active sessions are unavailable.
pub async fn torrent_details(State(state): State<AppState>) -> Html<String> {
    let sessions = state.engine().active_sessions().await.unwrap();

    // For now, show details for the first session if any exist
    // In a real implementation, this would take an info_hash parameter
    if let Some(session) = sessions.first() {
        let downloaded_bytes = (session.progress * session.total_size as f32) as u64;
        let ratio = if session.bytes_downloaded > 0 {
            session.bytes_uploaded as f64 / session.bytes_downloaded as f64
        } else {
            0.0
        };

        // Estimate peer/seed counts from completed pieces (rough approximation)
        let completed_count = session.completed_pieces.iter().filter(|&&x| x).count();
        let estimated_peers = std::cmp::min(completed_count / 10, 50) as u32;
        let estimated_seeds = std::cmp::max(estimated_peers / 3, 1);

        Html(torrent::torrent_details(torrent::TorrentDetailsParams {
            name: &session.filename,
            info_hash: &session.info_hash.to_string(),
            total_size: session.total_size,
            downloaded: downloaded_bytes,
            uploaded: session.bytes_uploaded,
            ratio,
            peers: estimated_peers,
            seeds: estimated_seeds,
        }))
    } else {
        Html(torrent::torrent_details(torrent::TorrentDetailsParams {
            name: "No active torrents",
            info_hash: "0000000000000000000000000000000000000000",
            total_size: 0,
            downloaded: 0,
            uploaded: 0,
            ratio: 0.0,
            peers: 0,
            seeds: 0,
        }))
    }
}
