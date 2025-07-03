//! API handlers for torrent management and search

use std::collections::{HashMap, HashSet};

use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::Json;
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::server::AppState;

#[derive(Serialize)]
pub struct Stats {
    pub total_torrents: u32,
    pub active_downloads: u32,
    pub upload_speed: f64,
    pub download_speed: f64,
}

#[derive(Deserialize)]
pub struct AddTorrentQuery {
    pub magnet: String,
}

#[derive(Deserialize)]
pub struct DownloadRequest {
    pub magnet_link: String,
}

pub async fn api_stats(State(state): State<AppState>) -> Json<Stats> {
    let stats = state.torrent_engine.get_download_stats().await.unwrap();

    Json(Stats {
        total_torrents: stats.active_torrents as u32,
        active_downloads: stats.active_torrents as u32,
        upload_speed: (stats.bytes_uploaded as f64) / 1_048_576.0,
        download_speed: (stats.bytes_downloaded as f64) / 1_048_576.0,
    })
}

pub async fn api_torrents(State(state): State<AppState>) -> Json<serde_json::Value> {
    let sessions = state.torrent_engine.get_active_sessions().await.unwrap();

    // Get movie manager data once outside the loop
    let movie_titles: HashMap<_, _> = if let Some(ref movie_manager) = state.movie_manager {
        let manager = movie_manager.read().await;
        manager
            .all_files()
            .iter()
            .map(|movie| (movie.info_hash, movie.title.clone()))
            .collect()
    } else {
        HashMap::new()
    };

    let torrents: Vec<serde_json::Value> = sessions.iter().map(|session| {
        // Check if this torrent has a corresponding local movie for better naming
        let name = movie_titles.get(&session.info_hash)
            .cloned()
            .unwrap_or_else(|| session.filename.clone());

        json!({
            "name": name,
            "progress": (session.progress * 100.0) as u32,
            "speed": 0, // TODO: Track download speed
            "size": format!("{:.1} GB", (session.piece_count as u64 * session.piece_size as u64) as f64 / 1_073_741_824.0),
            "status": if session.progress >= 1.0 { "completed" } else { "downloading" },
            "info_hash": session.info_hash.to_string(),
            "pieces": format!("{}/{}", session.completed_pieces.iter().filter(|&&x| x).count(), session.piece_count),
            "is_local": false // BitTorrent torrents should use piece-based streaming
        })
    }).collect();

    // In development mode, local movies are converted to BitTorrent torrents above,
    // so we don't need to add them separately. This prevents duplication.
    let display_torrents = torrents;

    // Fallback to development data if no torrents or local movies
    let final_torrents = if display_torrents.is_empty() {
        vec![
            json!({
                "name": "Development.Movie.2024.1080p.BluRay.x264",
                "progress": 45,
                "speed": 2500,
                "size": "1.5 GB",
                "status": "downloading"
            }),
            json!({
                "name": "Development.Series.S01E01.720p.WEB-DL.x264",
                "progress": 78,
                "speed": 1800,
                "size": "850 MB",
                "status": "downloading"
            }),
        ]
    } else {
        display_torrents
    };

    Json(json!({
        "torrents": final_torrents,
        "total": final_torrents.len()
    }))
}

pub async fn api_add_torrent(
    State(state): State<AppState>,
    Query(params): Query<AddTorrentQuery>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    if params.magnet.is_empty() {
        return Err(StatusCode::BAD_REQUEST);
    }

    match state.torrent_engine.add_magnet(&params.magnet).await {
        Ok(info_hash) => {
            // Start downloading immediately after adding
            match state.torrent_engine.start_download(info_hash).await {
                Ok(()) => Ok(Json(json!({
                    "success": true,
                    "message": "Torrent added and download started",
                    "info_hash": info_hash.to_string()
                }))),
                Err(e) => Ok(Json(json!({
                    "success": false,
                    "message": format!("Added but failed to start download: {e}")
                }))),
            }
        }
        Err(e) => Ok(Json(json!({
            "success": false,
            "message": format!("Failed: {e}")
        }))),
    }
}

pub async fn api_search(
    State(state): State<AppState>,
    Query(params): Query<HashMap<String, String>>,
) -> Json<serde_json::Value> {
    let query = params.get("q").map(|s| s.as_str()).unwrap_or("");

    if query.is_empty() {
        return Json(json!({"results": []}));
    }

    match state.search_service.search_with_metadata(query).await {
        Ok(results) => {
            // Flatten MediaSearchResult into individual torrents for the frontend
            let mut individual_torrents = Vec::new();
            for media_result in results {
                for torrent in media_result.torrents {
                    individual_torrents.push(json!({
                        "title": torrent.name,
                        "quality": format!("{:?}", torrent.quality),
                        "size": torrent.format_size(),
                        "seeds": torrent.seeders,
                        "magnet_link": torrent.magnet_link,
                        "source": torrent.source
                    }));
                }
            }

            Json(json!({"results": individual_torrents}))
        }
        Err(e) => {
            tracing::error!("Search failed for query '{}': {}", query, e);
            Json(json!({"results": []}))
        }
    }
}

pub async fn api_library(State(state): State<AppState>) -> Json<serde_json::Value> {
    let sessions = state.torrent_engine.get_active_sessions().await.unwrap();

    let mut library_items = Vec::new();
    let mut total_size = 0u64;

    // Collect info_hashes from local movies to avoid duplicates
    let mut local_info_hashes = HashSet::new();

    // Add local movies to library first (they have better metadata)
    if let Some(ref movie_manager) = state.movie_manager {
        let manager = movie_manager.read().await;
        for movie in manager.all_files() {
            local_info_hashes.insert(movie.info_hash);
            library_items.push(json!({
                "id": movie.info_hash.to_string(),
                "title": movie.title,
                "type": "Movie",
                "year": null, // TODO: Extract year from title
                "size": format!("{:.1} GB", movie.size as f64 / 1_073_741_824.0),
                "added_date": "2025-06-29", // TODO: Use file modification date
                "poster_url": null,
                "rating": null,
                "info_hash": movie.info_hash.to_string(),
                "is_local": true
            }));
            total_size += movie.size;
        }
    }

    // Add completed torrents that don't already exist as local movies
    for session in sessions.iter() {
        if session.progress >= 1.0 && !local_info_hashes.contains(&session.info_hash) {
            // 100% complete and not already in library as local movie
            let estimated_size = session.piece_count as u64 * session.piece_size as u64;
            library_items.push(json!({
                "id": session.info_hash.to_string(),
                "title": session.filename.clone(),
                "type": "Movie",
                "year": null,
                "size": format!("{:.1} GB", estimated_size as f64 / 1_073_741_824.0),
                "added_date": "2025-06-29", // TODO: Use actual completion date
                "poster_url": null,
                "rating": null,
                "info_hash": session.info_hash.to_string(),
                "is_torrent": true
            }));
            total_size += estimated_size;
        }
    }

    // Add development items if no real content
    if library_items.is_empty() {
        library_items = vec![
            json!({
                "id": "movie_1",
                "title": "The Matrix",
                "type": "Movie",
                "year": 1999,
                "size": "1.4 GB",
                "added_date": "2025-06-25",
                "poster_url": null,
                "rating": 8.7
            }),
            json!({
                "id": "movie_2",
                "title": "Inception",
                "type": "Movie",
                "year": 2010,
                "size": "2.1 GB",
                "added_date": "2025-06-27",
                "poster_url": null,
                "rating": 8.8
            }),
        ];
        total_size = 4_300_000_000;
    }

    Json(json!({
        "items": library_items,
        "total_size": total_size
    }))
}

pub async fn api_settings(State(_state): State<AppState>) -> Json<serde_json::Value> {
    Json(json!({
        "download_dir": "./downloads",
        "max_connections": 50,
        "dht_enabled": true
    }))
}

pub async fn api_download_torrent(
    State(state): State<AppState>,
    Json(payload): Json<DownloadRequest>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    if payload.magnet_link.is_empty() {
        return Ok(Json(json!({
            "success": false,
            "error": "Empty magnet link"
        })));
    }

    match state.torrent_engine.add_magnet(&payload.magnet_link).await {
        Ok(info_hash) => {
            // Start downloading immediately after adding
            match state.torrent_engine.start_download(info_hash).await {
                Ok(()) => Ok(Json(json!({
                    "success": true,
                    "message": "Download started successfully",
                    "info_hash": info_hash.to_string()
                }))),
                Err(e) => {
                    use riptide_core::torrent::TorrentError;

                    let error_msg = match &e {
                        TorrentError::TorrentNotFoundOnTracker { .. } => {
                            "This torrent is not available on public trackers. Try a different torrent or one with embedded tracker URLs.".to_string()
                        }
                        TorrentError::NoPeersAvailable => {
                            "BitTorrent peer connections not yet implemented. Riptide currently supports torrent search and tracker communication, but full peer-to-peer downloading requires additional development. Consider using this as a torrent search tool for now.".to_string()
                        }
                        TorrentError::TrackerConnectionFailed { .. } | TorrentError::TrackerServerError { .. } => {
                            "Tracker connection failed. The tracker servers may be offline or unreachable. This is common with older torrents. Try:\n• A different torrent from the same content\n• Torrents from more recent search results\n• Checking your internet connection".to_string()
                        }
                        TorrentError::TrackerTimeout { .. } => {
                            "Tracker request timed out. The tracker servers are responding slowly or may be overloaded.".to_string()
                        }
                        TorrentError::Http(reqwest_error) => {
                            // Handle specific HTTP errors that might indicate torrent availability
                            if let Some(status) = reqwest_error.status() {
                                match status.as_u16() {
                                    404 => "This torrent is not available on public trackers. Try a different torrent or one with embedded tracker URLs.".to_string(),
                                    500..=599 => "Tracker server is experiencing issues. Try again later or use a different torrent.".to_string(),
                                    _ => format!("HTTP error ({status}): {reqwest_error}")
                                }
                            } else {
                                "Network connection failed. Check your internet connection and try again.".to_string()
                            }
                        }
                        _ => format!("Download failed: {e}")
                    };

                    Ok(Json(json!({
                        "success": false,
                        "error": error_msg
                    })))
                }
            }
        }
        Err(e) => Ok(Json(json!({
            "success": false,
            "error": format!("Failed to add torrent: {e}")
        }))),
    }
}
