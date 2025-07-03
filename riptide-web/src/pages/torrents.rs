//! Torrents page - manage downloads and view progress

use axum::extract::State;
use axum::response::Html;

use crate::components::layout;
use crate::pages::dashboard::render_page;
use crate::server::AppState;

/// Renders the torrents management page
pub async fn torrents_page(State(state): State<AppState>) -> Html<String> {
    let sessions = state.torrent_engine.get_active_sessions().await.unwrap();

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
            </form>
            <div class="notification-area mt-4"></div>"#,
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
