//! API handlers for torrent management and search

use std::collections::HashMap;

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
    // Update progress simulation before reading stats
    {
        let mut engine = state.torrent_engine.write().await;
        engine.simulate_download_progress();
    }

    let engine = state.torrent_engine.read().await;
    let stats = engine.download_stats().await;

    Json(Stats {
        total_torrents: stats.active_torrents as u32,
        active_downloads: stats.active_torrents as u32,
        upload_speed: (stats.bytes_uploaded as f64) / 1_048_576.0,
        download_speed: (stats.bytes_downloaded as f64) / 1_048_576.0,
    })
}

pub async fn api_torrents(State(state): State<AppState>) -> Json<serde_json::Value> {
    // Update progress simulation before reading
    {
        let mut engine = state.torrent_engine.write().await;
        engine.simulate_download_progress();
    }

    let engine = state.torrent_engine.read().await;
    let sessions = engine.active_sessions();

    let torrents: Vec<serde_json::Value> = sessions.iter().map(|session| {
        json!({
            "name": format!("Torrent {}", &session.info_hash.to_string()[..8]),
            "progress": (session.progress * 100.0) as u32,
            "speed": 0, // TODO: Track download speed
            "size": "Unknown", // TODO: Get from metadata
            "status": if session.progress >= 1.0 { "completed" } else { "downloading" },
            "info_hash": session.info_hash.to_string(),
            "pieces": format!("{}/{}", session.completed_pieces.iter().filter(|&&x| x).count(), session.piece_count)
        })
    }).collect();

    // Add local movies to torrents list if available
    let mut display_torrents = torrents;

    // Add local movies to torrents list if available
    if let Some(ref movie_manager) = state.movie_manager {
        let manager = movie_manager.read().await;
        let local_movies: Vec<serde_json::Value> = manager
            .all_movies()
            .iter()
            .map(|movie| {
                json!({
                    "name": movie.title,
                    "progress": 100, // Local movies are always "complete"
                    "speed": 0,
                    "size": format!("{:.1} GB", movie.size as f64 / 1_073_741_824.0),
                    "status": "completed",
                    "info_hash": movie.info_hash.to_string(),
                    "is_local": true
                })
            })
            .collect();
        display_torrents.extend(local_movies);
    }

    // Fallback to demo data if no torrents or local movies
    let final_torrents = if display_torrents.is_empty() {
        vec![
            json!({
                "name": "Demo.Movie.2024.1080p.BluRay.x264",
                "progress": 45,
                "speed": 2500,
                "size": "1.5 GB",
                "status": "downloading"
            }),
            json!({
                "name": "Demo.Series.S01E01.720p.WEB-DL.x264",
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

    let mut engine = state.torrent_engine.write().await;
    match engine.add_magnet(&params.magnet).await {
        Ok(info_hash) => {
            // Start downloading immediately after adding
            match engine.start_download(info_hash).await {
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
    let engine = state.torrent_engine.read().await;
    let _stats = engine.download_stats().await;
    let sessions = engine.active_sessions();

    let mut library_items = Vec::new();
    let mut total_size = 0u64;

    // Add completed torrents to library
    for session in sessions.iter() {
        if session.progress >= 1.0 {
            // 100% complete
            let estimated_size = session.piece_count as u64 * session.piece_size as u64;
            library_items.push(json!({
                "id": session.info_hash.to_string(),
                "title": format!("Downloaded: {}", &session.info_hash.to_string()[..16]),
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

    // Add local movies to library
    if let Some(ref movie_manager) = state.movie_manager {
        let manager = movie_manager.read().await;
        for movie in manager.all_movies() {
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

    // Add demo items if no real content
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

    let mut engine = state.torrent_engine.write().await;
    match engine.add_magnet(&payload.magnet_link).await {
        Ok(info_hash) => {
            // Start downloading immediately after adding
            match engine.start_download(info_hash).await {
                Ok(()) => Ok(Json(json!({
                    "success": true,
                    "message": "Download started successfully",
                    "info_hash": info_hash.to_string()
                }))),
                Err(e) => {
                    let error_str = e.to_string();
                    let error_msg = if error_str.contains("404 Not Found") {
                        "This torrent is not available on public trackers. Try a different torrent or one with embedded tracker URLs.".to_string()
                    } else if error_str.contains("No peers available") {
                        "BitTorrent peer connections not yet implemented. Riptide currently supports torrent search and tracker communication, but full peer-to-peer downloading requires additional development. Consider using this as a torrent search tool for now.".to_string()
                    } else if error_str.contains("Tracker connection failed")
                        || error_str.contains("HTTP request failed")
                    {
                        "Tracker connection failed. The tracker servers may be offline or unreachable. This is common with older torrents. Try:\n• A different torrent from the same content\n• Torrents from more recent search results\n• Checking your internet connection".to_string()
                    } else if error_str.contains("Failed to connect to tracker") {
                        "Unable to reach tracker servers. This could be due to network issues or the trackers being offline.".to_string()
                    } else if error_str.contains("Tracker request timed out") {
                        "Tracker request timed out. The tracker servers are responding slowly or may be overloaded.".to_string()
                    } else {
                        format!("Download failed: {e}")
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
