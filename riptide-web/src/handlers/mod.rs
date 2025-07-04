//! HTTP request handlers organized by functionality

pub mod api;
pub mod htmx;
pub mod pages;
pub mod range;
pub mod streaming;

// Re-export handler functions
pub use api::{
    AddTorrentQuery, DownloadRequest, SeekRequest, Stats, api_add_torrent, api_download_torrent,
    api_library, api_search, api_seek_torrent, api_settings, api_stats, api_torrents,
};
pub use htmx::{
    add_torrent_htmx, dashboard_activity, dashboard_downloads, dashboard_stats, torrents_list,
};
pub use pages::{
    DownloadStats, dashboard_page, library_page, search_page, torrents_page, video_player_page,
};
pub use range::{
    build_range_response, extract_range_header, parse_range_header, validate_range_bounds,
};
pub use streaming::stream_torrent;
