//! HTTP request handlers organized by functionality

pub mod api;
pub mod pages;
pub mod streaming;
pub mod utils;

// Re-export handler functions
pub use api::{
    AddTorrentQuery, Stats, api_add_torrent, api_library, api_search, api_settings, api_stats,
    api_torrents,
};
pub use pages::{
    DownloadStats, dashboard_page, library_page, search_page, torrents_page, video_player_page,
};
pub use streaming::{
    AddLocalMovieQuery, api_add_local_movie, api_local_movies, stream_local_movie, stream_torrent,
};
pub use utils::{create_fake_video_segment, decode_hex, parse_info_hash, parse_range_header};
