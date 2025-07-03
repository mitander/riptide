//! Torrent-related components - list items, forms, status indicators

use super::layout::button;
use super::stats::progress_bar;

/// Parameters for rendering a torrent list item
pub struct TorrentListItemParams<'a> {
    pub name: &'a str,
    pub info_hash: &'a str,
    pub progress: u32,
    pub size: &'a str,
    pub speed: Option<&'a str>,
    pub status: &'a str,
    pub eta: Option<&'a str>,
}

/// Renders a torrent list item with progress and actions
pub fn torrent_list_item(params: TorrentListItemParams) -> String {
    let TorrentListItemParams {
        name,
        info_hash,
        progress,
        size,
        speed,
        status,
        eta,
    } = params;
    let speed_html = speed
        .map(|s| format!(r#"<span class="text-riptide-400">{s}</span>"#))
        .unwrap_or_else(|| "<span class=\"text-gray-500\">--</span>".to_string());

    let eta_html = eta
        .map(|e| format!(r#"<span class="text-gray-400">ETA: {e}</span>"#))
        .unwrap_or_default();

    let status_badge = torrent_status_badge(status);

    format!(
        r#"<div class="bg-gray-800 border border-gray-700 rounded-lg p-4 hover:border-gray-600 transition-colors">
            <div class="flex items-start justify-between mb-4">
                <div class="flex-1 min-w-0">
                    <h4 class="text-white font-medium truncate mb-1">{}</h4>
                    <p class="text-gray-400 text-sm font-mono">{}</p>
                </div>
                <div class="ml-4 flex-shrink-0">
                    {}
                </div>
            </div>
            
            <div class="mb-4">
                {}
            </div>
            
            <div class="flex items-center justify-between text-sm">
                <div class="flex items-center space-x-4">
                    <span class="text-gray-400">Size: <span class="text-white">{}</span></span>
                    <span class="text-gray-400">Speed: {}</span>
                    {}
                </div>
                
                <div class="flex items-center space-x-2">
                    {}
                    {}
                </div>
            </div>
        </div>"#,
        name,
        &info_hash[..16],
        status_badge,
        progress_bar(progress, Some("Progress"), None),
        size,
        speed_html,
        eta_html,
        button(
            "Pause",
            "ghost",
            Some(r#"hx-post="/api/torrents/pause" hx-target="closest .bg-gray-800""#)
        ),
        button(
            "Details",
            "secondary",
            Some("hx-get='/torrents/details' hx-target='#modal'")
        )
    )
}

/// Renders a form for adding new torrents
pub fn torrent_form() -> String {
    format!(
        r#"<form hx-post="/api/torrents/add" 
                hx-target=".add-result" 
                hx-swap="innerHTML"
                hx-indicator=".add-spinner"
                class="space-y-4">
            <div class="flex space-x-4">
                {}
                {}
            </div>
            <div class="add-result min-h-6"></div>
        </form>"#,
        crate::components::layout::input(
            "magnet",
            "Paste magnet link here...",
            "url",
            Some("required")
        ),
        button(
            r#"<span class="htmx-indicator" id="add-spinner">Adding...</span><span>Add Torrent</span>"#,
            "primary",
            Some("type=\"submit\"")
        )
    )
}

/// Renders a status badge for torrents
pub fn torrent_status_badge(status: &str) -> String {
    let (bg_class, text_class, icon) = match status {
        "downloading" => ("bg-riptide-500 bg-opacity-20", "text-riptide-400", "⬇️"),
        "completed" => ("bg-green-500 bg-opacity-20", "text-green-400", "✅"),
        "seeding" => ("bg-blue-500 bg-opacity-20", "text-blue-400", "⬆️"),
        "paused" => ("bg-yellow-500 bg-opacity-20", "text-yellow-400", "⏸️"),
        "error" => ("bg-red-500 bg-opacity-20", "text-red-400", "❌"),
        "queued" => ("bg-gray-500 bg-opacity-20", "text-gray-400", "⏳"),
        _ => ("bg-gray-500 bg-opacity-20", "text-gray-400", "❓"),
    };

    format!(
        r#"<span class="inline-flex items-center px-2 py-1 rounded-full text-xs font-medium {bg_class} {text_class}">
            <span class="mr-1">{icon}</span>
            {status}
        </span>"#
    )
}

/// Renders a quick actions panel for torrents
pub fn torrent_actions(info_hash: &str) -> String {
    format!(
        r#"<div class="flex items-center space-x-2">
            {}
            {}
            {}
            {}
        </div>"#,
        button(
            "▶️",
            "ghost",
            Some(&format!(r#"hx-post="/api/torrents/{info_hash}/start""#))
        ),
        button(
            "⏸️",
            "ghost",
            Some(&format!(r#"hx-post="/api/torrents/{info_hash}/pause""#))
        ),
        button(
            "🗑️",
            "danger",
            Some(&format!(
                r#"hx-delete="/api/torrents/{info_hash}" hx-confirm="Delete this torrent?""#
            ))
        ),
        button(
            "📊",
            "secondary",
            Some(&format!(
                "hx-get='/torrents/{info_hash}/stats' hx-target='#modal'"
            ))
        )
    )
}

/// Parameters for rendering torrent details modal
pub struct TorrentDetailsParams<'a> {
    pub name: &'a str,
    pub info_hash: &'a str,
    pub total_size: u64,
    pub downloaded: u64,
    pub uploaded: u64,
    pub ratio: f64,
    pub peers: u32,
    pub seeds: u32,
}

/// Renders a torrent details modal content
pub fn torrent_details(params: TorrentDetailsParams) -> String {
    let TorrentDetailsParams {
        name,
        info_hash,
        total_size,
        downloaded,
        uploaded,
        ratio,
        peers,
        seeds,
    } = params;
    let downloaded_gb = downloaded as f64 / 1_073_741_824.0;
    let uploaded_gb = uploaded as f64 / 1_073_741_824.0;
    let total_gb = total_size as f64 / 1_073_741_824.0;

    format!(
        r#"<div class="bg-gray-800 rounded-lg p-6 max-w-2xl w-full">
            <div class="flex items-start justify-between mb-6">
                <div>
                    <h3 class="text-xl font-semibold text-white mb-2">{name}</h3>
                    <p class="text-gray-400 font-mono text-sm">{info_hash}</p>
                </div>
                <button class="text-gray-400 hover:text-white" onclick="closeModal()">
                    <svg class="w-6 h-6" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                        <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M6 18L18 6M6 6l12 12"></path>
                    </svg>
                </button>
            </div>
            
            <div class="grid grid-cols-2 gap-6">
                <div class="space-y-4">
                    <div>
                        <p class="text-gray-400 text-sm">Total Size</p>
                        <p class="text-white font-medium">{total_gb:.2} GB</p>
                    </div>
                    <div>
                        <p class="text-gray-400 text-sm">Downloaded</p>
                        <p class="text-white font-medium">{downloaded_gb:.2} GB</p>
                    </div>
                    <div>
                        <p class="text-gray-400 text-sm">Uploaded</p>
                        <p class="text-white font-medium">{uploaded_gb:.2} GB</p>
                    </div>
                </div>
                
                <div class="space-y-4">
                    <div>
                        <p class="text-gray-400 text-sm">Share Ratio</p>
                        <p class="text-white font-medium">{ratio:.2}</p>
                    </div>
                    <div>
                        <p class="text-gray-400 text-sm">Connected Peers</p>
                        <p class="text-white font-medium">{peers}</p>
                    </div>
                    <div>
                        <p class="text-gray-400 text-sm">Available Seeds</p>
                        <p class="text-white font-medium">{seeds}</p>
                    </div>
                </div>
            </div>
        </div>"#
    )
}

/// Renders an empty state for when no torrents exist
pub fn torrents_empty_state() -> String {
    format!(
        r#"<div class="text-center py-12">
            <div class="text-6xl mb-4">📦</div>
            <h3 class="text-xl font-medium text-white mb-2">No torrents yet</h3>
            <p class="text-gray-400 mb-6">Add a magnet link above to start downloading</p>
            {}
        </div>"#,
        button(
            "Browse Popular Content",
            "primary",
            Some(r#"onclick="window.location.href='/search'""#)
        )
    )
}
