//! Page handlers for the web interface

use axum::extract::{Path, Query, State};
use axum::response::Html;
use serde::Deserialize;

use crate::server::AppState;
use crate::templates::{
    base_template, dashboard_content, library_content, search_content, torrents_content,
    video_player_content,
};

/// Download statistics for template rendering.
#[derive(Default)]
pub struct DownloadStats {
    pub active_torrents: u32,
    pub bytes_downloaded: u64,
    pub bytes_uploaded: u64,
}

pub async fn dashboard_page(State(state): State<AppState>) -> Html<String> {
    let api_stats = state.torrent_engine.get_download_stats().await.unwrap();

    // Convert API stats to DownloadStats format
    let stats = DownloadStats {
        active_torrents: api_stats.active_torrents as u32,
        bytes_downloaded: api_stats.bytes_downloaded,
        bytes_uploaded: api_stats.bytes_uploaded,
    };

    let content = dashboard_content(&stats);
    Html(base_template("Dashboard", "dashboard", &content))
}

pub async fn search_page(State(_state): State<AppState>) -> Html<String> {
    let content = search_content();
    Html(base_template("Search", "search", &content))
}

pub async fn torrents_page(State(_state): State<AppState>) -> Html<String> {
    let content = torrents_content();
    Html(base_template("Torrents", "torrents", &content))
}

pub async fn library_page(State(_state): State<AppState>) -> Html<String> {
    let content = library_content();
    Html(base_template("Library", "library", &content))
}

/// Query parameters for video player page.
#[derive(Deserialize)]
pub struct VideoPlayerQuery {
    pub local: Option<bool>,
}

pub async fn video_player_page(
    Path(info_hash): Path<String>,
    Query(query): Query<VideoPlayerQuery>,
    State(_state): State<AppState>,
) -> Html<String> {
    let is_local = query.local.unwrap_or(false);
    let content = video_player_content(&info_hash, is_local);
    Html(base_template("Video Player", "", &content))
}
