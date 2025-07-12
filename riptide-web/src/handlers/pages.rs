//! Page handlers for main web interface

use axum::extract::{Path, State};
use axum::response::Html;
use serde::{Deserialize, Serialize};

use crate::components::{layout, stats};
use crate::server::AppState;
use crate::templates::video_player;

/// Engine statistics for dashboard display.
#[derive(Serialize, Deserialize)]
pub struct DownloadStats {
    /// Number of active torrents
    pub active_torrents: u32,
    /// Number of torrents currently downloading
    pub active_downloads: u32,
    /// Total size of all torrents in bytes
    pub total_size: u64,
    /// Total downloaded size in bytes
    pub downloaded_size: u64,
    /// Current upload speed in bytes per second
    pub upload_speed: f64,
    /// Current download speed in bytes per second
    pub download_speed: f64,
}

/// Renders the main dashboard page.
///
/// Displays torrent engine statistics, activity feed, and active downloads
/// with real-time updates powered by HTMX. Includes statistical summary cards
/// showing current download/upload activity.
///
/// # Panics
/// Panics if engine communication fails or download statistics are unavailable.
pub async fn dashboard_page(State(state): State<AppState>) -> Html<String> {
    let stats = state.engine().download_statistics().await.unwrap();

    // Quick stats cards
    let stats_cards = [
        stats::stat_card(
            &stats.active_torrents.to_string(),
            "Active Torrents",
            None,
            None,
            None,
        ),
        stats::stat_card(
            &format!("{:.1}", stats.bytes_downloaded as f64 / 1_073_741_824.0),
            "Downloaded",
            Some("GB"),
            None,
            Some("text-green-400"),
        ),
        stats::stat_card(
            &format!("{:.1}", stats.bytes_uploaded as f64 / 1_073_741_824.0),
            "Uploaded",
            Some("GB"),
            None,
            Some("text-blue-400"),
        ),
        stats::stat_card(
            &format!("{:.1}", stats.average_progress * 100.0),
            "Average Progress",
            Some("%"),
            None,
            None,
        ),
    ];

    let content = format!(
        r#"{}

        <div class="mb-8">
            {}
        </div>

        <div class="grid grid-cols-1 lg:grid-cols-2 gap-8">
            <div id="activity-feed"
                 hx-get="/htmx/dashboard/activity"
                 hx-trigger="load, every 10s"
                 hx-swap="innerHTML">
                <div class="text-center py-8 text-gray-400">Loading activity...</div>
            </div>

            <div id="downloads-panel"
                 hx-get="/htmx/dashboard/downloads"
                 hx-trigger="load, every 5s"
                 hx-swap="innerHTML">
                <div class="text-center py-8 text-gray-400">Loading downloads...</div>
            </div>
        </div>"#,
        layout::page_header(
            "Dashboard",
            Some("Monitor your torrents and system activity"),
            None
        ),
        stats::stats_grid(&stats_cards)
    );

    render_page("Dashboard", "dashboard", &content)
}

/// Renders the torrents management page.
///
/// Provides torrent management interface with add torrent form, live torrent list,
/// and summary statistics. Features real-time updates and interactive controls
/// for torrent operations.
///
/// # Panics
/// Panics if engine communication fails or active sessions are unavailable.
pub async fn torrents_page(State(state): State<AppState>) -> Html<String> {
    let sessions = state.engine().active_sessions().await.unwrap();

    // Quick stats for header
    let total_torrents = sessions.len();
    let downloading = sessions.iter().filter(|s| s.progress < 1.0).count();
    let completed = sessions.iter().filter(|s| s.progress >= 1.0).count();

    // Add torrent form
    let add_form = layout::card(
        Some("Add New Torrent"),
        &format!(
            r#"<form hx-post="/htmx/torrents/add"
                      hx-target=".notification-area"
                      hx-swap="innerHTML"
                      hx-indicator=".add-spinner"
                      class="space-y-4">
                <div class="flex space-x-4">
                    {}
                    {}
                </div>
                <div class="notification-area mt-4"></div>
            </form>"#,
            layout::input(
                "magnet",
                "Paste magnet link here...",
                "url",
                Some("required class=\"flex-1\"")
            ),
            layout::button(
                r#"<span class="htmx-indicator hidden add-spinner">Adding...</span><span>Add Torrent</span>"#,
                "primary",
                Some("type=\"submit\"")
            )
        ),
        None,
    );

    // Torrent list with real-time updates
    let torrent_list = layout::card(
        Some(&format!(
            "Active Torrents ({total_torrents} total, {downloading} downloading)"
        )),
        r#"<div id="torrents-list"
                   hx-get="/htmx/torrents/list"
                   hx-trigger="load, every 5s"
                   hx-swap="innerHTML">
                <div class="text-center py-8 text-gray-400">Loading torrents...</div>
            </div>"#,
        Some(
            r#"<button class="text-riptide-400 hover:text-riptide-300 text-sm" onclick="location.reload()">Refresh</button>"#,
        ),
    );

    // Quick stats cards
    let stats_cards = [
        format!(
            r#"<div class="bg-gray-800 border border-gray-700 rounded-lg p-4">
            <div class="text-2xl font-bold text-white mb-1">{total_torrents}</div>
            <div class="text-gray-400 text-sm">Total Torrents</div>
        </div>"#
        ),
        format!(
            r#"<div class="bg-gray-800 border border-gray-700 rounded-lg p-4">
            <div class="text-2xl font-bold text-riptide-400 mb-1">{downloading}</div>
            <div class="text-gray-400 text-sm">Downloading</div>
        </div>"#
        ),
        format!(
            r#"<div class="bg-gray-800 border border-gray-700 rounded-lg p-4">
            <div class="text-2xl font-bold text-green-400 mb-1">{completed}</div>
            <div class="text-gray-400 text-sm">Completed</div>
        </div>"#
        ),
    ];

    let stats_grid = layout::grid("grid-cols-3", &stats_cards.join(""));

    // Main content
    let content = format!(
        r#"{}

        <div class="mb-6">
            {}
        </div>

        {}

        {}"#,
        layout::page_header(
            "Torrent Management",
            Some("Monitor downloads and manage your torrents"),
            Some(
                r#"<div class="flex items-center space-x-4">
                <div class="flex items-center space-x-2 text-sm text-gray-400">
                    <div class="w-2 h-2 bg-green-400 rounded-full status-pulse"></div>
                    <span>Auto-refresh</span>
                </div>
            </div>"#
            )
        ),
        stats_grid,
        add_form,
        torrent_list
    );

    render_page("Torrents", "torrents", &content)
}

/// Renders the library page.
///
/// Displays the user's media library with browsing and search capabilities.
/// Currently shows basic page structure - full library features to be implemented.
pub async fn library_page(State(_state): State<AppState>) -> Html<String> {
    let content = layout::page_header("Library", Some("Browse your media collection"), None);
    render_page("Library", "library", &content)
}

/// Renders the search page.
///
/// Provides torrent and media search interface for discovering new content.
/// Currently shows basic page structure - full search features to be implemented.
pub async fn search_page(State(_state): State<AppState>) -> Html<String> {
    let content = layout::page_header("Search", Some("Discover new content"), None);
    render_page("Search", "search", &content)
}

/// Renders the video player page for streaming torrents.
///
/// Creates a full-page video player interface for streaming torrents or local media.
/// Supports both BitTorrent streaming and local file playback with adaptive controls.
/// Includes navigation and player controls optimized for streaming.
///
/// # Errors
/// Returns error HTML page if the info hash is invalid or torrent not found
pub async fn video_player_page(
    State(state): State<AppState>,
    Path(info_hash_str): Path<String>,
) -> Html<String> {
    // Parse info hash
    let info_hash = match riptide_core::torrent::InfoHash::from_hex(&info_hash_str) {
        Ok(hash) => hash,
        Err(_) => {
            return Html(
                r#"<!DOCTYPE html>
                <html><head><title>Error</title></head>
                <body><h1>Invalid info hash</h1></body></html>"#
                    .to_string(),
            );
        }
    };

    // Check if this is a torrent or local movie
    let is_local = if let Ok(movie_manager) = state.file_library() {
        let manager = movie_manager.read().await;
        manager.file_by_hash(info_hash).is_some()
    } else {
        false
    };

    // Get torrent session for title
    let title = match state.engine().session_details(info_hash).await {
        Ok(session) => session.filename.clone(),
        Err(_) => {
            if let Ok(movie_manager) = state.file_library() {
                let manager = movie_manager.read().await;
                manager
                    .file_by_hash(info_hash)
                    .map(|movie| movie.title.clone())
                    .unwrap_or_else(|| "Unknown".to_string())
            } else {
                "Unknown".to_string()
            }
        }
    };

    // Generate video player content
    let player_content = video_player::video_player_content(&info_hash_str, &title, is_local);

    // Wrap in full page template using local CSS
    Html(format!(
        r#"<!DOCTYPE html>
        <html lang="en">
        <head>
            <meta charset="UTF-8">
            <meta name="viewport" content="width=device-width, initial-scale=1.0">
            <title>Streaming: {title}</title>
            <link rel="stylesheet" href="/static/css/main.css">
            <script src="https://unpkg.com/htmx.org@1.9.3"></script>
            <script src="/static/js/main.js" defer></script>
        </head>
        <body>
            <!-- Navigation -->
            <nav>
                <div class="nav-container">
                    <div class="logo">Riptide</div>
                    <div class="nav-links">
                        <a href="/">Dashboard</a>
                        <a href="/search">Search</a>
                        <a href="/torrents">Torrents</a>
                        <a href="/library">Library</a>
                    </div>
                </div>
            </nav>

            <!-- Main Content -->
            <main class="container">
                {player_content}
            </main>
        </body>
        </html>"#
    ))
}

/// Common page template wrapper.
///
/// Generates complete HTML pages with consistent styling, navigation, and HTMX integration.
/// Includes Tailwind CSS configuration and custom color scheme for Riptide branding.
fn render_page(title: &str, _active_page: &str, content: &str) -> Html<String> {
    Html(format!(
        r#"<!DOCTYPE html>
        <html lang="en">
        <head>
            <meta charset="UTF-8">
            <meta name="viewport" content="width=device-width, initial-scale=1.0">
            <title>{title} - Riptide</title>
            <script src="https://unpkg.com/htmx.org@1.9.3"></script>
            <script src="https://cdn.tailwindcss.com"></script>
            <script>
                tailwind.config = {{
                    theme: {{
                        extend: {{
                            colors: {{
                                'riptide': {{
                                    '50': '#f0f9ff',
                                    '400': '#38bdf8',
                                    '500': '#0ea5e9',
                                    '600': '#0284c7',
                                }}
                            }}
                        }}
                    }}
                }}
            </script>
        </head>
        <body class="bg-slate-900 text-white min-h-screen">
            <div class="container mx-auto px-4 py-8">
                {content}
            </div>
        </body>
        </html>"#
    ))
}
