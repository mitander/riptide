//! Torrent management HTMX handlers

use axum::extract::{Form, State};
use axum::response::Html;
use serde::Deserialize;

use crate::components::{activity, torrent};
use crate::server::AppState;

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

    match state.torrent_engine.add_magnet(&form.magnet).await {
        Ok(info_hash) => {
            // Start downloading immediately after adding
            match state.torrent_engine.start_download(info_hash).await {
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
    let sessions = state.torrent_engine.get_active_sessions().await.unwrap();

    // Get movie manager data for better naming
    let movie_titles: std::collections::HashMap<_, _> =
        if let Some(ref movie_manager) = state.movie_manager {
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

            let speed = if session.progress < 1.0 && session.progress > 0.0 {
                Some("2.1 MB/s") // TODO: Calculate real speed
            } else {
                None
            };

            let eta = if session.progress < 1.0 && session.progress > 0.0 {
                Some("12m 34s") // TODO: Calculate real ETA
            } else {
                None
            };

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
pub async fn torrent_details(State(_state): State<AppState>) -> Html<String> {
    // TODO: Implement torrent details fetching
    Html(torrent::torrent_details(torrent::TorrentDetailsParams {
        name: "Sample Movie 2024 1080p BluRay x264",
        info_hash: "1234567890abcdef1234567890abcdef12345678",
        total_size: 1_500_000_000, // 1.5 GB
        downloaded: 750_000_000,   // 750 MB downloaded
        uploaded: 200_000_000,     // 200 MB uploaded
        ratio: 0.27,               // ratio
        peers: 5,                  // peers
        seeds: 12,                 // seeds
    }))
}
