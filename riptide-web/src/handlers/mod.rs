//! HTTP request handlers organized by functionality

pub mod api;
pub mod pages;
pub mod range;
pub mod streaming;

// Re-export handler functions
pub use api::{
    AddTorrentQuery, DownloadRequest, Stats, api_add_torrent, api_download_torrent, api_library,
    api_search, api_settings, api_stats, api_torrents,
};
pub use pages::{
    DownloadStats, dashboard_page, library_page, search_page, torrents_page, video_player_page,
};
pub use range::{
    build_range_response, extract_range_header, parse_range_header, validate_range_bounds,
};
pub use streaming::{
    AddLocalMovieQuery, api_add_local_movie, api_local_movies, stream_local_movie, stream_torrent,
};
