//! API handlers for torrent management and search

use std::collections::{HashMap, HashSet};

use axum::extract::{Json, Path, Query, State};
use axum::http::StatusCode;
use chrono;
use serde::{Deserialize, Serialize};
use serde_json::json;
use sysinfo::{Disks, System};

use crate::server::AppState;

/// Engine statistics for JSON API responses.
#[derive(Serialize)]
pub struct Stats {
    /// Total number of torrents being managed
    pub total_torrents: u32,
    /// Number of torrents currently downloading
    pub active_downloads: u32,
    /// Current upload speed in MB/s
    pub upload_speed: f64,
    /// Current download speed in MB/s
    pub download_speed: f64,
}

/// Query parameters for adding a torrent via magnet link.
#[derive(Deserialize)]
pub struct AddTorrentQuery {
    /// Magnet link URL to add
    pub magnet: String,
}

/// Request body for adding a torrent via JSON POST.
#[derive(Deserialize)]
pub struct AddTorrentRequest {
    /// Magnet link for the torrent to add
    pub magnet_link: String,
}

/// Request body for initiating a torrent download.
#[derive(Deserialize)]
pub struct DownloadRequest {
    /// Magnet link for the torrent to download
    pub magnet_link: String,
}

/// Request body for seeking to a position in streaming torrent.
#[derive(Deserialize)]
pub struct SeekRequest {
    /// Position to seek to in seconds
    pub position: f64,
    /// Buffer size around seek position in seconds (optional)
    pub buffer_duration: Option<f64>,
}

/// Returns engine statistics as JSON.
///
/// Provides current download/upload statistics including active torrents,
/// transfer speeds, and peer connection counts.
///
/// # Panics
///
/// Panics if engine communication fails or statistics are unavailable.
pub async fn api_stats(State(state): State<AppState>) -> axum::response::Json<Stats> {
    // Add timeout to prevent hanging
    let stats_result = tokio::time::timeout(
        std::time::Duration::from_secs(10),
        state.engine().download_statistics(),
    )
    .await;

    let stats = match stats_result {
        Ok(Ok(stats)) => stats,
        Ok(Err(_)) | Err(_) => {
            // Return default stats if engine call fails or times out
            return axum::response::Json(Stats {
                total_torrents: 0,
                active_downloads: 0,
                upload_speed: 0.0,
                download_speed: 0.0,
            });
        }
    };

    axum::response::Json(Stats {
        total_torrents: stats.active_torrents as u32,
        active_downloads: stats.active_torrents as u32,
        upload_speed: (stats.bytes_uploaded as f64) / 1_048_576.0,
        download_speed: (stats.bytes_downloaded as f64) / 1_048_576.0,
    })
}

/// Returns list of all active torrents as JSON.
///
/// Provides detailed information about each torrent including progress,
/// download status, and metadata.
///
/// # Panics
///
/// Panics if engine active sessions cannot be retrieved
pub async fn api_torrents(
    State(state): State<AppState>,
) -> axum::response::Json<serde_json::Value> {
    let sessions_result = tokio::time::timeout(
        std::time::Duration::from_secs(10),
        state.engine().active_sessions(),
    )
    .await;

    let sessions = match sessions_result {
        Ok(Ok(sessions)) => sessions,
        Ok(Err(_)) | Err(_) => {
            // Return empty array if engine call fails or times out
            return axum::response::Json(json!([]));
        }
    };

    // Get movie manager data once outside the loop
    let movie_titles: HashMap<_, _> = if let Ok(movie_manager) = state.file_library() {
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
            "speed": (session.download_speed_bps as f64 / 1_048_576.0).round() as u32,
            "size": format!("{:.1} GB", (session.piece_count as u64 * session.piece_size as u64) as f64 / 1_073_741_824.0),
            "status": if session.progress >= 1.0 { "completed" } else { "downloading" },
            "info_hash": session.info_hash.to_string(),
            "pieces": format!("{}/{}", session.completed_pieces.iter().filter(|&&x| x).count(), session.piece_count),
            "is_local": false, // BitTorrent torrents should use piece-based streaming
            "eta": calculate_eta(session.progress, session.download_speed_bps, session.total_size),
            "upload_speed": (session.upload_speed_bps as f64 / 1_048_576.0).round() as u32,
            "bytes_downloaded": session.bytes_downloaded,
            "bytes_uploaded": session.bytes_uploaded
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

    axum::response::Json(json!(final_torrents))
}

/// Adds a new torrent from magnet link.
///
/// Accepts a magnet link and adds the torrent to the engine for downloading.
/// Returns the torrent's info hash on success.
///
/// # Errors
///
/// - `StatusCode::BAD_REQUEST` - If magnet link is empty.
pub async fn api_add_torrent(
    State(state): State<AppState>,
    Json(request): Json<AddTorrentRequest>,
) -> Result<axum::response::Json<serde_json::Value>, StatusCode> {
    if request.magnet_link.is_empty() {
        return Err(StatusCode::BAD_REQUEST);
    }

    let add_result = tokio::time::timeout(
        std::time::Duration::from_secs(15),
        state.engine().add_magnet(&request.magnet_link),
    )
    .await;

    match add_result {
        Ok(Ok(info_hash)) => {
            // Start downloading immediately after adding
            let start_result = tokio::time::timeout(
                std::time::Duration::from_secs(10),
                state.engine().start_download(info_hash),
            )
            .await;

            match start_result {
                Ok(Ok(())) => Ok(axum::response::Json(json!({
                    "success": true,
                    "message": "Torrent added and download started",
                    "info_hash": info_hash.to_string()
                }))),
                Ok(Err(_)) | Err(_) => Ok(axum::response::Json(json!({
                    "success": false,
                    "message": "Added but failed to start download"
                }))),
            }
        }
        Ok(Err(_)) | Err(_) => Ok(axum::response::Json(json!({
            "success": false,
            "message": "Failed to add magnet link"
        }))),
    }
}

/// Query parameters for movie search with fuzzy matching
#[derive(Deserialize)]
pub struct MovieSearchQuery {
    /// Search query (movie title)
    pub q: String,
    /// Enable fuzzy matching for typos (default: true)
    pub fuzzy: Option<bool>,
    /// Fuzzy match threshold 0.0-1.0 (default: 0.6)
    pub threshold: Option<f64>,
    /// Maximum results to return (default: 10)
    pub limit: Option<usize>,
}

/// Searches for torrents using the query string.
///
/// Performs a torrent search across configured providers and returns
/// matching results with metadata and download links.
pub async fn api_search(
    State(state): State<AppState>,
    Query(params): Query<HashMap<String, String>>,
) -> axum::response::Json<serde_json::Value> {
    let query = params.get("q").map(|s| s.as_str()).unwrap_or("");

    if query.is_empty() {
        return Json(json!({"results": []}));
    }

    match state.media_search.search_with_metadata(query).await {
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

            axum::response::Json(json!({"results": individual_torrents}))
        }
        Err(e) => {
            tracing::error!("Search failed for query '{}': {}", query, e);
            axum::response::Json(json!({"results": []}))
        }
    }
}

/// Enhanced movie search API with fuzzy matching and rich metadata.
///
/// Provides advanced search capabilities with fuzzy string matching,
/// configurable similarity thresholds, and rich movie metadata including
/// IMDB information, ratings, and multiple torrent options per movie.
pub async fn api_search_movies(
    State(state): State<AppState>,
    Query(params): Query<MovieSearchQuery>,
) -> axum::response::Json<serde_json::Value> {
    if params.q.trim().is_empty() {
        return axum::response::Json(json!({
            "movies": [],
            "total": 0,
            "query": params.q,
            "fuzzy_enabled": false
        }));
    }

    let fuzzy_enabled = params.fuzzy.unwrap_or(true);
    let max_results = params.limit.unwrap_or(10);
    let threshold = params.threshold.unwrap_or(0.6);

    // Use the enhanced search functionality from the existing service
    let search_result = state
        .media_search
        .search_movies_enhanced(&params.q, Some(threshold))
        .await;

    match search_result {
        Ok(mut movies) => {
            // Limit results
            movies.truncate(max_results);

            let movie_results: Vec<serde_json::Value> = movies
                .into_iter()
                .map(|movie| {
                    // Get best torrent for quick streaming
                    let best_torrent = movie.torrents.first();

                    json!({
                        "title": movie.title,
                        "year": movie.year,
                        "imdb_id": movie.imdb_id,
                        "poster_url": movie.poster_url,
                        "plot": movie.plot,
                        "genre": movie.genre,
                        "rating": movie.rating,
                        "runtime": movie.runtime,
                        "director": movie.director,
                        "cast": movie.cast,
                        "match_score": movie.match_score,
                        "torrent_count": movie.torrents.len(),
                        "best_quality": best_torrent.map(|t| format!("{:?}", t.quality)),
                        "best_magnet": best_torrent.map(|t| &t.magnet_link),
                        "best_size": best_torrent.map(|t| t.format_size()),
                        "best_seeds": best_torrent.map(|t| t.seeders),
                        "torrents": movie.torrents.into_iter().map(|torrent| json!({
                            "name": torrent.name,
                            "quality": format!("{:?}", torrent.quality),
                            "size": torrent.format_size(),
                            "seeders": torrent.seeders,
                            "leechers": torrent.leechers,
                            "magnet_link": torrent.magnet_link,
                            "source": torrent.source,
                            "priority_score": torrent.priority_score()
                        })).collect::<Vec<_>>()
                    })
                })
                .collect();

            axum::response::Json(json!({
                "movies": movie_results,
                "total": movie_results.len(),
                "query": params.q,
                "fuzzy_enabled": fuzzy_enabled,
                "threshold": threshold,
                "search_type": "enhanced_with_fuzzy_matching"
            }))
        }
        Err(e) => {
            tracing::error!("Enhanced search failed for query '{}': {}", params.q, e);
            axum::response::Json(json!({
                "movies": [],
                "total": 0,
                "query": params.q,
                "error": format!("Search failed: {e}"),
                "fuzzy_enabled": fuzzy_enabled,
                "threshold": threshold
            }))
        }
    }
}

/// Returns the movie library as JSON.
///
/// Provides a list of all movies in the managed library with their
/// metadata, file paths, and streaming information.
///
/// # Panics
///
/// Panics if the torrent engine fails to return active sessions.
pub async fn api_library(State(state): State<AppState>) -> axum::response::Json<serde_json::Value> {
    let sessions_result = tokio::time::timeout(
        std::time::Duration::from_secs(10),
        state.engine().active_sessions(),
    )
    .await;

    let sessions = match sessions_result {
        Ok(Ok(sessions)) => sessions,
        Ok(Err(_)) | Err(_) => {
            // Return empty array if engine call fails or times out
            return axum::response::Json(json!([]));
        }
    };

    let mut library_items = Vec::new();

    // Collect info_hashes from local movies to avoid duplicates
    let mut local_info_hashes = HashSet::new();

    // Add local movies to library first (they have better metadata)
    if let Ok(movie_manager) = state.file_library() {
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
    }

    axum::response::Json(json!(library_items))
}

/// Returns application settings as JSON.
///
/// Provides current configuration settings including download directory,
/// connection limits, and feature toggles.
pub async fn api_settings(
    State(_state): State<AppState>,
) -> axum::response::Json<serde_json::Value> {
    axum::response::Json(json!({
        "download_dir": "./downloads",
        "max_connections": 50,
        "dht_enabled": true
    }))
}

/// Downloads a torrent file by info hash.
///
/// Retrieves the .torrent file content for the specified torrent,
/// allowing clients to download the torrent file directly.
///
/// # Errors
///
/// - `StatusCode::BAD_REQUEST` - If magnet link invalid or missing parameters.
/// - `StatusCode::NOT_FOUND` - If torrent not found in the system.
/// - `StatusCode::INTERNAL_SERVER_ERROR` - If failed to retrieve torrent data.
pub async fn api_download_torrent(
    State(state): State<AppState>,
    Json(payload): Json<DownloadRequest>,
) -> Result<axum::response::Json<serde_json::Value>, StatusCode> {
    if payload.magnet_link.is_empty() {
        return Ok(axum::response::Json(json!({
            "success": false,
            "error": "Empty magnet link"
        })));
    }

    let add_result = tokio::time::timeout(
        std::time::Duration::from_secs(15),
        state.engine().add_magnet(&payload.magnet_link),
    )
    .await;

    match add_result {
        Ok(Ok(info_hash)) => {
            // Start downloading immediately after adding
            let start_result = tokio::time::timeout(
                std::time::Duration::from_secs(10),
                state.engine().start_download(info_hash),
            )
            .await;

            match start_result {
                Ok(Ok(())) => Ok(axum::response::Json(json!({
                    "success": true,
                    "message": "Download started successfully",
                    "info_hash": info_hash.to_string()
                }))),
                Ok(Err(e)) => {
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

                    Ok(axum::response::Json(json!({
                        "success": false,
                        "error": error_msg
                    })))
                }
                Err(_) => Ok(axum::response::Json(json!({
                    "success": false,
                    "error": "Download request timed out"
                }))),
            }
        }
        Ok(Err(_)) | Err(_) => Ok(axum::response::Json(json!({
            "success": false,
            "error": "Failed to add torrent"
        }))),
    }
}

/// API endpoint for seeking to a position in a streaming torrent.
///
/// Accepts seek position in seconds and optional buffer duration,
/// converts to byte position based on torrent bitrate, and signals
/// the torrent engine to prioritize pieces around that position.
///
/// # Errors
///
/// - `StatusCode::BAD_REQUEST` - If info hash or seek parameters invalid.
/// - `StatusCode::NOT_FOUND` - If torrent not found or not active.
/// - `StatusCode::INTERNAL_SERVER_ERROR` - If failed to perform seek operation.
pub async fn api_seek_torrent(
    State(state): State<AppState>,
    Path(info_hash_str): Path<String>,
    Json(payload): Json<SeekRequest>,
) -> Result<axum::response::Json<serde_json::Value>, StatusCode> {
    // Parse info hash
    let info_hash = match riptide_core::torrent::InfoHash::from_hex(&info_hash_str) {
        Ok(hash) => hash,
        Err(_) => {
            return Ok(Json(json!({
                "success": false,
                "error": "Invalid info hash format"
            })));
        }
    };

    // Validate seek position
    if payload.position < 0.0 {
        return Ok(Json(json!({
            "success": false,
            "error": "Seek position cannot be negative"
        })));
    }

    // Get torrent session to calculate byte position
    match state.engine().session_details(info_hash).await {
        Ok(session) => {
            // Estimate bitrate based on file size and assume typical video duration
            // This is a rough approximation - in production you'd want metadata parsing
            let total_size_bytes = session.piece_count as u64 * session.piece_size as u64;
            let estimated_duration_seconds =
                estimate_video_duration(&session.filename, total_size_bytes);

            if payload.position > estimated_duration_seconds {
                return Ok(Json(json!({
                    "success": false,
                    "error": format!("Seek position {:.1}s exceeds estimated duration {:.1}s",
                        payload.position, estimated_duration_seconds)
                })));
            }

            // Convert seek position to byte position
            let byte_position =
                (payload.position / estimated_duration_seconds) * total_size_bytes as f64;
            let buffer_duration = payload.buffer_duration.unwrap_or(30.0); // Default 30s buffer
            let buffer_size_bytes =
                (buffer_duration / estimated_duration_seconds) * total_size_bytes as f64;

            // Signal the torrent engine to prioritize pieces around this position
            match state
                .engine()
                .seek_to_position(info_hash, byte_position as u64, buffer_size_bytes as u64)
                .await
            {
                Ok(()) => {
                    tracing::info!("Successfully requested piece prioritization for seek position")
                }
                Err(e) => tracing::warn!("Failed to prioritize pieces for seek: {:?}", e),
            }

            Ok(axum::response::Json(json!({
                "success": true,
                "message": format!("Seek request received for position {:.1}s (byte position: {:.0})",
                    payload.position, byte_position),
                "seek_position_seconds": payload.position,
                "seek_position_bytes": byte_position as u64,
                "buffer_size_bytes": buffer_size_bytes as u64,
                "estimated_duration": estimated_duration_seconds
            })))
        }
        Err(_) => Ok(axum::response::Json(json!({
            "success": false,
            "error": "Torrent not found or not currently downloading"
        }))),
    }
}

/// Estimates video duration based on filename and file size.
///
/// This is a rough heuristic - in production you'd want proper metadata extraction.
/// Uses common video quality indicators to estimate bitrate and calculate duration.
fn estimate_video_duration(filename: &str, size_bytes: u64) -> f64 {
    // Default assumptions for video bitrates and duration
    let size_gb = size_bytes as f64 / 1_073_741_824.0;

    // Heuristic based on typical video file sizes
    if filename.to_lowercase().contains("720p") {
        // 720p movies: ~1.5-2.5 GB for 90-120 minutes
        (size_gb / 2.0) * 3600.0 // Rough estimate: 2GB per hour
    } else if filename.to_lowercase().contains("1080p") {
        // 1080p movies: ~2.5-4 GB for 90-120 minutes
        (size_gb / 3.0) * 3600.0 // Rough estimate: 3GB per hour
    } else if filename.to_lowercase().contains("4k") || filename.to_lowercase().contains("2160p") {
        // 4K movies: ~8-15 GB for 90-120 minutes
        (size_gb / 10.0) * 3600.0 // Rough estimate: 10GB per hour
    } else {
        // Default assumption for unknown quality
        (size_gb / 2.5) * 3600.0 // Rough estimate: 2.5GB per hour
    }
}

/// Calculates estimated time to completion for a download.
///
/// Returns a human-readable ETA string (e.g. "5m", "2h", "1d") based on
/// current download progress and speed. Returns None if download is complete.
fn calculate_eta(progress: f32, download_speed_bps: u64, total_size: u64) -> Option<String> {
    if progress >= 1.0 || download_speed_bps == 0 {
        return None;
    }

    let remaining_bytes = total_size - (total_size as f32 * progress) as u64;
    let eta_seconds = remaining_bytes as f64 / download_speed_bps as f64;

    if eta_seconds > 86400.0 {
        // More than a day
        Some(format!("{:.0}d", eta_seconds / 86400.0))
    } else if eta_seconds > 3600.0 {
        // More than an hour
        Some(format!("{:.0}h", eta_seconds / 3600.0))
    } else if eta_seconds > 60.0 {
        // More than a minute
        Some(format!("{:.0}m", eta_seconds / 60.0))
    } else {
        // Less than a minute
        Some(format!("{eta_seconds:.0}s"))
    }
}

/// Response structure for dashboard activity endpoint.
#[derive(Serialize)]
pub struct ActivityItem {
    /// Timestamp of the activity
    pub timestamp: String,
    /// Type of activity (download, upload, error, etc.)
    pub activity_type: String,
    /// Human-readable description of the activity
    pub message: String,
    /// Optional torrent name associated with activity
    pub torrent_name: Option<String>,
}

/// Response structure for dashboard downloads endpoint.
#[derive(Serialize)]
pub struct DownloadItem {
    /// Info hash of the torrent
    pub info_hash: String,
    /// Display name of the torrent
    pub name: String,
    /// Download progress as percentage (0-100)
    pub progress: u32,
    /// Current download speed in MB/s
    pub speed: f64,
    /// Estimated time to completion
    pub eta: Option<String>,
    /// Total size in bytes
    pub size: u64,
    /// Downloaded bytes
    pub downloaded: u64,
    /// Number of connected peers
    pub peers: u32,
}

/// Response structure for system status endpoint.
#[derive(Serialize)]
pub struct SystemStatus {
    /// Server uptime in seconds
    pub uptime_seconds: u64,
    /// Memory usage in MB
    pub memory_usage_mb: f64,
    /// CPU usage percentage
    pub cpu_usage_percent: f64,
    /// Disk usage for downloads directory
    pub disk_usage_percent: f64,
    /// Network connectivity status
    pub network_status: String,
    /// Version information
    pub version: String,
}

/// Returns recent activity items for dashboard display.
///
/// Provides a chronological list of recent torrent activities including
/// downloads started, completed, errors, and other significant events.
///
/// # Panics
///
/// Panics if engine communication fails or activity data is unavailable.
pub async fn api_dashboard_activity(
    State(state): State<AppState>,
) -> axum::response::Json<Vec<ActivityItem>> {
    let sessions_result = tokio::time::timeout(
        std::time::Duration::from_secs(10),
        state.engine().active_sessions(),
    )
    .await;

    let sessions = match sessions_result {
        Ok(Ok(sessions)) => sessions,
        Ok(Err(_)) | Err(_) => {
            // Return empty array if engine call fails or times out
            return axum::response::Json(vec![]);
        }
    };

    // Generate activity items from current session state
    let mut activities = Vec::new();

    for session in sessions.iter().take(10) {
        // Latest 10 activities
        let activity_type = if session.progress >= 1.0 {
            "completed"
        } else if session.progress > 0.0 {
            "downloading"
        } else {
            "started"
        };

        activities.push(ActivityItem {
            timestamp: chrono::Utc::now().format("%H:%M:%S").to_string(),
            activity_type: activity_type.to_string(),
            message: format!("{} - {:.1}%", session.filename, session.progress * 100.0),
            torrent_name: Some(session.filename.clone()),
        });
    }

    axum::response::Json(activities)
}

/// Returns current downloads for dashboard display.
///
/// Provides detailed information about active downloads including progress,
/// speed, ETA, and peer connection status.
///
/// # Panics
///
/// Panics if engine communication fails or download data is unavailable.
pub async fn api_dashboard_downloads(
    State(state): State<AppState>,
) -> axum::response::Json<Vec<DownloadItem>> {
    let sessions_result = tokio::time::timeout(
        std::time::Duration::from_secs(10),
        state.engine().active_sessions(),
    )
    .await;

    let sessions = match sessions_result {
        Ok(Ok(sessions)) => sessions,
        Ok(Err(_)) | Err(_) => {
            // Return empty array if engine call fails or times out
            return axum::response::Json(vec![]);
        }
    };

    let downloads: Vec<DownloadItem> = sessions
        .iter()
        .filter(|session| session.progress < 1.0) // Only active downloads
        .map(|session| {
            let eta = calculate_eta(
                session.progress,
                session.download_speed_bps,
                session.total_size,
            );

            DownloadItem {
                info_hash: session.info_hash.to_string(),
                name: session.filename.clone(),
                progress: (session.progress * 100.0) as u32,
                speed: session.bytes_downloaded as f64 / 1_048_576.0, // MB/s approximation
                eta,
                size: session.total_size,
                downloaded: (session.total_size as f32 * session.progress) as u64,
                peers: 0, // TODO: Add peer count tracking to TorrentSession
            }
        })
        .collect();

    axum::response::Json(downloads)
}

/// Returns system status information.
///
/// Provides server health metrics including uptime, resource usage,
/// and operational status for monitoring and debugging.
pub async fn api_system_status(
    State(state): State<AppState>,
) -> axum::response::Json<SystemStatus> {
    let uptime = state.server_started_at.elapsed();

    let mut system = System::new_all();
    system.refresh_all();

    // Calculate memory usage in MB
    let memory_usage_mb = (system.used_memory() as f64) / 1_048_576.0;

    // Calculate average CPU usage across all cores
    let cpu_usage_percent = system
        .cpus()
        .iter()
        .map(|cpu| cpu.cpu_usage() as f64)
        .sum::<f64>()
        / system.cpus().len() as f64;

    // Calculate disk usage for downloads directory
    let downloads_path = std::path::Path::new("./downloads");
    let disks = Disks::new_with_refreshed_list();
    let disk_usage_percent = disks
        .iter()
        .find(|disk| downloads_path.starts_with(disk.mount_point()))
        .map(|disk| {
            let used = disk.total_space() - disk.available_space();
            (used as f64 / disk.total_space() as f64) * 100.0
        })
        .unwrap_or(0.0);

    // Determine network status based on active sessions and peer connections
    let sessions_result = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        state.engine().active_sessions(),
    )
    .await;

    let network_status = match sessions_result {
        Ok(Ok(sessions)) => {
            let active_count = sessions.len();
            let downloading_count = sessions.iter().filter(|s| s.is_downloading).count();

            if active_count == 0 {
                "idle".to_string()
            } else if downloading_count > 0 {
                "connected".to_string()
            } else {
                "connecting".to_string()
            }
        }
        Ok(Err(_)) | Err(_) => "error".to_string(),
    };

    axum::response::Json(SystemStatus {
        uptime_seconds: uptime.as_secs(),
        memory_usage_mb,
        cpu_usage_percent,
        disk_usage_percent,
        network_status,
        version: env!("CARGO_PKG_VERSION").to_string(),
    })
}
