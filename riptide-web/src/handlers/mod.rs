//! HTTP request handlers organized by functionality

pub mod api;
pub mod range;

pub mod streaming;
pub mod streaming_readiness;

// Re-export handler functions
pub use api::{
    ActivityItem, AddTorrentQuery, DownloadItem, DownloadRequest, MovieSearchQuery, SeekRequest,
    Stats, SystemStatus, api_add_torrent, api_dashboard_activity, api_dashboard_downloads,
    api_download_torrent, api_library, api_search, api_search_movies, api_seek_torrent,
    api_settings, api_stats, api_system_status, api_torrents,
};
pub use range::{
    build_range_response, extract_range_header, parse_range_header, validate_range_bounds,
};
pub use streaming::{cleanup_sessions, stream_torrent, streaming_health, streaming_stats};
pub use streaming_readiness::streaming_readiness_handler;
