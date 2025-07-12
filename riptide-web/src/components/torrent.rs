//! Torrent-related components - list items, forms, status indicators

use super::layout::button;
use super::stats::progress_bar;

/// Parameters for rendering a torrent list item
pub struct TorrentListItemParams<'a> {
    /// Display name of the torrent
    pub name: &'a str,
    /// Torrent info hash for identification
    pub info_hash: &'a str,
    /// Download progress as percentage (0-100)
    pub progress: u32,
    /// Human-readable file size (e.g., "1.5 GB")
    pub size: &'a str,
    /// Current download speed if available
    pub speed: Option<&'a str>,
    /// Current status (downloading, completed, etc.)
    pub status: &'a str,
    /// Estimated time to completion if available
    pub eta: Option<&'a str>,
}

/// Renders a torrent list item with progress and actions.
///
/// Creates a comprehensive torrent card with name, progress bar, statistics,
/// and action buttons for pause, stream, and details. Includes hover effects
/// and responsive layout.
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
            "Stream",
            "primary",
            Some(&format!(
                "onclick=\"window.open('/player/{}', '_blank')\"",
                &info_hash[..40]
            ))
        ),
        button(
            "Details",
            "secondary",
            Some("hx-get='/torrents/details' hx-target='#modal'")
        )
    )
}

/// Renders a form for adding new torrents.
///
/// Creates HTMX-powered form with magnet link input and add button.
/// Includes loading indicators and result display area for user feedback.
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

/// Renders a status badge for torrents.
///
/// Creates colored status badges with icons and consistent styling.
/// Supports downloading, completed, seeding, paused, error, and queued states.
pub fn torrent_status_badge(status: &str) -> String {
    let (bg_class, text_class, icon) = match status {
        "downloading" => ("bg-riptide-500 bg-opacity-20", "text-riptide-400", "↓"),
        "completed" => ("bg-green-500 bg-opacity-20", "text-green-400", "✓"),
        "seeding" => ("bg-blue-500 bg-opacity-20", "text-blue-400", "↑"),
        "paused" => ("bg-yellow-500 bg-opacity-20", "text-yellow-400", "||"),
        "error" => ("bg-red-500 bg-opacity-20", "text-red-400", "✗"),
        "queued" => ("bg-gray-500 bg-opacity-20", "text-gray-400", "•"),
        _ => ("bg-gray-500 bg-opacity-20", "text-gray-400", "?"),
    };

    format!(
        r#"<span class="inline-flex items-center px-2 py-1 rounded-full text-xs font-medium {bg_class} {text_class}">
            <span class="mr-1">{icon}</span>
            {status}
        </span>"#
    )
}

/// Renders a quick actions panel for torrents.
///
/// Creates action button group with start, pause, delete, and stats controls.
/// Uses HTMX for seamless interactions without page reloads.
pub fn torrent_actions(info_hash: &str) -> String {
    format!(
        r#"<div class="flex items-center space-x-2">
            {}
            {}
            {}
            {}
        </div>"#,
        button(
            "▶",
            "ghost",
            Some(&format!(r#"hx-post="/api/torrents/{info_hash}/start""#))
        ),
        button(
            "||",
            "ghost",
            Some(&format!(r#"hx-post="/api/torrents/{info_hash}/pause""#))
        ),
        button(
            "✗",
            "danger",
            Some(&format!(
                r#"hx-delete="/api/torrents/{info_hash}" hx-confirm="Delete this torrent?""#
            ))
        ),
        button(
            "Stats",
            "secondary",
            Some(&format!(
                "hx-get='/torrents/{info_hash}/stats' hx-target='#modal'"
            ))
        )
    )
}

/// Parameters for rendering torrent details modal
pub struct TorrentDetailsParams<'a> {
    /// Display name of the torrent
    pub name: &'a str,
    /// Torrent info hash
    pub info_hash: &'a str,
    /// Total size in bytes
    pub total_size: u64,
    /// Downloaded bytes
    pub downloaded: u64,
    /// Uploaded bytes
    pub uploaded: u64,
    /// Share ratio (uploaded/downloaded)
    pub ratio: f64,
    /// Number of connected peers
    pub peers: u32,
    /// Number of available seeders
    pub seeds: u32,
}

/// Renders a torrent details modal content.
///
/// Creates detailed modal showing comprehensive torrent statistics including
/// sizes, transfer ratios, peer information, and formatted display values.
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

/// Renders an empty state for when no torrents exist.
///
/// Creates friendly empty state with icon, message, and call-to-action button
/// to guide users toward adding their first torrent or browsing content.
pub fn torrents_empty_state() -> String {
    format!(
        r#"<div class="text-center py-12">
            <div class="text-6xl mb-4">•</div>
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
