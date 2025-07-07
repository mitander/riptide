//! Dashboard page - real-time overview of torrents and system status

use axum::extract::State;
use axum::response::Html;

use crate::components::{layout, stats};
use crate::server::AppState;

/// Renders the main dashboard page with real-time components
pub async fn dashboard_page(State(state): State<AppState>) -> Html<String> {
    let engine_stats = state.engine().download_statistics().await.unwrap();
    let sessions = state.engine().get_active_sessions().await.unwrap();

    // Initial stats for server-side rendering
    let active_torrents = sessions.len();
    let downloading_count = sessions.iter().filter(|s| s.progress < 1.0).count();
    let completed_count = sessions.iter().filter(|s| s.progress >= 1.0).count();
    let download_speed = (engine_stats.bytes_downloaded as f64) / 1_048_576.0;
    let upload_speed = (engine_stats.bytes_uploaded as f64) / 1_048_576.0;
    let total_downloaded = (engine_stats.bytes_downloaded as f64) / 1_073_741_824.0;

    let library_size = if let Ok(movie_manager) = state.file_manager() {
        let manager = movie_manager.read().await;
        manager.all_files().len()
    } else {
        0
    };

    let connected_peers: usize = sessions
        .iter()
        .map(|s| s.completed_pieces.iter().filter(|&&x| x).count())
        .sum::<usize>()
        .min(50);

    // Create initial stats grid
    let initial_stats = vec![
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

    // Live stats container with HTMX auto-refresh
    let live_stats = format!(
        r#"<div id="dashboard-stats" 
               hx-get="/htmx/dashboard/stats" 
               hx-trigger="load, every 5s" 
               hx-swap="innerHTML">
            {}
        </div>"#,
        stats::stats_grid(&initial_stats)
    );

    // Quick actions grid
    let quick_actions = layout::grid(
        "grid-cols-1 md:grid-cols-3",
        r#"<a href="/search" class="group bg-gray-800 border border-gray-700 rounded-lg p-6 hover:border-riptide-500 transition-colors">
                <div class="text-3xl mb-3">🔍</div>
                <h3 class="text-white font-semibold mb-2">Search Content</h3>
                <p class="text-gray-400 text-sm">Find movies, TV shows, and more</p>
            </a>
            <a href="/torrents" class="group bg-gray-800 border border-gray-700 rounded-lg p-6 hover:border-riptide-500 transition-colors">
                <div class="text-3xl mb-3">⬇️</div>
                <h3 class="text-white font-semibold mb-2">Manage Downloads</h3>
                <p class="text-gray-400 text-sm">View progress and control torrents</p>
            </a>
            <a href="/library" class="group bg-gray-800 border border-gray-700 rounded-lg p-6 hover:border-riptide-500 transition-colors">
                <div class="text-3xl mb-3">📚</div>
                <h3 class="text-white font-semibold mb-2">Browse Library</h3>
                <p class="text-gray-400 text-sm">Stream your downloaded content</p>
            </a>"#,
    );

    // Quick add torrent form
    let quick_add = layout::card(
        Some("Quick Add Torrent"),
        &crate::components::torrent::torrent_form(),
        None,
    );

    // Recent activity with live updates
    let _recent_activity = layout::card(
        Some("Recent Activity"),
        r#"<div id="dashboard-activity"
                   hx-get="/htmx/dashboard/activity" 
                   hx-trigger="load, every 10s" 
                   hx-swap="innerHTML">
                <div class="text-center py-8 text-gray-400">Loading activity...</div>
            </div>"#,
        None,
    );

    // Active downloads preview
    let active_downloads = layout::card(
        Some("Active Downloads"),
        r#"<div id="dashboard-downloads"
                   hx-get="/htmx/dashboard/downloads" 
                   hx-trigger="load, every 3s" 
                   hx-swap="innerHTML">
                <div class="text-center py-8 text-gray-400">Loading downloads...</div>
            </div>"#,
        Some(
            r#"<a href="/torrents" class="text-riptide-400 hover:text-riptide-300 text-sm">View All →</a>"#,
        ),
    );

    // System status sidebar
    let system_status = layout::card(
        Some("System Status"),
        &format!(
            r#"<div id="system-metrics"
                   hx-get="/htmx/system/metrics" 
                   hx-trigger="load, every 30s" 
                   hx-swap="innerHTML">
                <div class="space-y-3">
                    {}
                    {}
                    {}
                </div>
            </div>"#,
            stats::status_indicator("online", "BitTorrent Engine"),
            stats::status_indicator("active", "Network Connected"),
            stats::status_indicator("active", "Storage Available")
        ),
        None,
    );

    // Main layout
    let content = format!(
        r#"{}
        
        {}
        
        <div class="grid grid-cols-1 lg:grid-cols-3 gap-6">
            <div class="lg:col-span-2 space-y-6">
                {}
                {}
                {}
            </div>
            <div class="space-y-6">
                {}
            </div>
        </div>"#,
        layout::page_header(
            "Dashboard",
            Some("Monitor your downloads and server status"),
            Some(
                r#"<div class="flex items-center space-x-2 text-sm text-gray-400">
                <div class="w-2 h-2 bg-green-400 rounded-full status-pulse"></div>
                <span>Live Updates</span>
            </div>"#
            )
        ),
        live_stats,
        layout::card(Some("Quick Actions"), &quick_actions, None),
        quick_add,
        active_downloads,
        system_status
    );

    // Render full page with base template
    render_page("Dashboard", "dashboard", &content)
}

/// Helper function to render a page with the base template
pub fn render_page(title: &str, active_nav: &str, content: &str) -> Html<String> {
    let html = format!(
        r#"<!DOCTYPE html>
        <html lang="en">
        <head>
            <title>{} - Riptide</title>
            <meta charset="utf-8">
            <meta name="viewport" content="width=device-width, initial-scale=1">
            <script src="https://cdn.tailwindcss.com"></script>
            <script src="https://unpkg.com/htmx.org@1.9.10"></script>
            <script>
                tailwind.config = {{
                    darkMode: 'class',
                    theme: {{
                        extend: {{
                            colors: {{
                                'riptide': {{
                                    50: '#eff6ff',
                                    400: '#60b2ff',
                                    500: '#4a9eff',
                                    600: '#3a8edf',
                                    900: '#0a0a0a'
                                }}
                            }}
                        }}
                    }}
                }}
            </script>
            <style>
                .htmx-indicator {{ opacity: 0; transition: opacity 0.3s; }}
                .htmx-request .htmx-indicator {{ opacity: 1; }}
                .htmx-request.htmx-indicator {{ opacity: 1; }}
                
                @keyframes pulse-green {{
                    0%, 100% {{ opacity: 1; }}
                    50% {{ opacity: 0.5; }}
                }}
                .status-pulse {{ animation: pulse-green 2s infinite; }}
                
                .fadeInDown {{
                    animation: fadeInDown 0.3s ease-out;
                }}
                
                @keyframes fadeInDown {{
                    from {{ opacity: 0; transform: translateY(-10px); }}
                    to {{ opacity: 1; transform: translateY(0); }}
                }}
            </style>
        </head>
        <body class="bg-gray-900 text-white min-h-screen font-sans">
            {}
            
            <main class="max-w-7xl mx-auto px-4 py-8">
                {}
            </main>

            <!-- Toast notification container -->
            <div id="toast-container" class="fixed top-4 right-4 space-y-2 z-50"></div>
            
            <!-- Modal container -->
            <div id="modal" class="fixed inset-0 bg-black bg-opacity-50 hidden items-center justify-center z-50"></div>
        </body>
        </html>"#,
        title,
        layout::nav_bar(active_nav),
        content
    );

    Html(html)
}
