//! Simple JSON API server for torrent management

use std::sync::Arc;

use axum::Router;
use axum::body::Body;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{Html, Json, Response};
use axum::routing::get;
use riptide_core::config::RiptideConfig;
use riptide_core::torrent::TorrentEngine;
use riptide_core::{LocalMovieManager, RuntimeMode};
use riptide_search::MediaSearchService;
use serde::{Deserialize, Serialize};
use serde_json::json;
use tokio::sync::RwLock;
use tower_http::cors::CorsLayer;

use crate::templates::{
    base_template, dashboard_content, library_content, search_content, torrents_content,
    video_player_content,
};

#[derive(Clone)]
pub struct AppState {
    torrent_engine: Arc<RwLock<TorrentEngine>>,
    search_service: MediaSearchService,
    movie_manager: Option<Arc<RwLock<LocalMovieManager>>>,
}

#[derive(Serialize)]
pub struct Stats {
    pub total_torrents: u32,
    pub active_downloads: u32,
    pub upload_speed: f64,
    pub download_speed: f64,
}

#[derive(Deserialize)]
pub struct AddTorrentQuery {
    magnet: String,
}

#[derive(Default)]
pub struct DownloadStats {
    pub active_torrents: u32,
    pub bytes_downloaded: u64,
    pub bytes_uploaded: u64,
}

pub async fn run_server(
    config: RiptideConfig,
    mode: RuntimeMode,
    movies_dir: Option<std::path::PathBuf>,
) -> Result<(), Box<dyn std::error::Error>> {
    let torrent_engine = Arc::new(RwLock::new(TorrentEngine::new(config)));
    let search_service = MediaSearchService::from_runtime_mode(mode);

    // Initialize movie manager for demo mode with local files
    let movie_manager = if let Some(dir) = movies_dir.as_ref().filter(|_| mode.is_demo()) {
        let mut manager = LocalMovieManager::new();

        match manager.scan_directory(dir).await {
            Ok(count) => {
                println!("Found {} movie files in {}", count, dir.display());
                Some(Arc::new(RwLock::new(manager)))
            }
            Err(e) => {
                eprintln!("Warning: Failed to scan movies directory: {e}");
                None
            }
        }
    } else {
        None
    };

    let state = AppState {
        torrent_engine,
        search_service,
        movie_manager,
    };

    let app = Router::new()
        .route("/", get(dashboard_page))
        .route("/torrents", get(torrents_page))
        .route("/library", get(library_page))
        .route("/search", get(search_page))
        .route("/player/{info_hash}", get(video_player_page))
        .route("/stream/{info_hash}", get(stream_torrent))
        .route("/api/movies/stream/{info_hash}", get(stream_local_movie))
        .route("/api/stats", get(api_stats))
        .route("/api/torrents", get(api_torrents))
        .route("/api/torrents/add", get(api_add_torrent))
        .route("/api/movies/local", get(api_local_movies))
        .route("/api/movies/add", get(api_add_local_movie))
        .route("/api/library", get(api_library))
        .route("/api/search", get(api_search))
        .route("/api/settings", get(api_settings))
        .layer(CorsLayer::permissive())
        .with_state(state);

    println!("Riptide media server running on http://127.0.0.1:3000");
    let listener = tokio::net::TcpListener::bind("127.0.0.1:3000").await?;
    axum::serve(listener, app).await?;
    Ok(())
}

async fn dashboard_page(State(state): State<AppState>) -> Html<String> {
    let engine = state.torrent_engine.read().await;
    let api_stats = engine.get_download_stats().await;
    drop(engine);

    // Convert API stats to DownloadStats format
    let stats = DownloadStats {
        active_torrents: api_stats.active_torrents as u32,
        bytes_downloaded: api_stats.bytes_downloaded,
        bytes_uploaded: api_stats.bytes_uploaded,
    };

    let content = dashboard_content(&stats);
    Html(base_template("Dashboard", "dashboard", &content))
}

async fn search_page(State(_state): State<AppState>) -> Html<String> {
    let content = search_content();
    Html(base_template("Search", "search", &content))
}

async fn torrents_page(State(_state): State<AppState>) -> Html<String> {
    let content = torrents_content();
    Html(base_template("Torrents", "torrents", &content))
}

async fn library_page(State(_state): State<AppState>) -> Html<String> {
    let content = library_content();
    Html(base_template("Library", "library", &content))
}

#[derive(Deserialize)]
struct VideoPlayerQuery {
    local: Option<bool>,
}

async fn video_player_page(
    Path(info_hash): Path<String>,
    Query(query): Query<VideoPlayerQuery>,
    State(_state): State<AppState>,
) -> Html<String> {
    let is_local = query.local.unwrap_or(false);
    let content = video_player_content(&info_hash, is_local);
    Html(base_template("Video Player", "", &content))
}

async fn api_stats(State(state): State<AppState>) -> Json<Stats> {
    // Update progress simulation before reading stats
    {
        let mut engine = state.torrent_engine.write().await;
        engine.simulate_download_progress();
    }

    let engine = state.torrent_engine.read().await;
    let stats = engine.get_download_stats().await;

    Json(Stats {
        total_torrents: stats.active_torrents as u32,
        active_downloads: stats.active_torrents as u32,
        upload_speed: (stats.bytes_uploaded as f64) / 1_048_576.0,
        download_speed: (stats.bytes_downloaded as f64) / 1_048_576.0,
    })
}

async fn api_torrents(State(state): State<AppState>) -> Json<serde_json::Value> {
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

    // Fallback to demo data if no real torrents or local movies
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

async fn api_add_torrent(
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
                    "message": format!("Added but failed to start download: {}", e)
                }))),
            }
        }
        Err(e) => Ok(Json(json!({
            "success": false,
            "message": format!("Failed: {}", e)
        }))),
    }
}

async fn api_search(
    State(state): State<AppState>,
    Query(params): Query<std::collections::HashMap<String, String>>,
) -> Json<serde_json::Value> {
    let query = params.get("q").map(|s| s.as_str()).unwrap_or("");

    if query.is_empty() {
        return Json(json!([]));
    }

    match state.search_service.search_with_metadata(query).await {
        Ok(results) => Json(json!(results)),
        Err(e) => {
            eprintln!("Search error: {e:?}");
            Json(json!([
                {"title": format!("Movie: {}", query), "type": "movie"},
                {"title": format!("Show: {}", query), "type": "tv"}
            ]))
        }
    }
}

async fn api_library(State(state): State<AppState>) -> Json<serde_json::Value> {
    let engine = state.torrent_engine.read().await;
    let _stats = engine.get_download_stats().await;
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

async fn api_settings(State(_state): State<AppState>) -> Json<serde_json::Value> {
    Json(json!({
        "download_dir": "./downloads",
        "max_connections": 50,
        "dht_enabled": true
    }))
}

async fn stream_torrent(
    State(state): State<AppState>,
    Path(info_hash_str): Path<String>,
    headers: HeaderMap,
) -> Result<Response<Body>, StatusCode> {
    // Update download progress first
    {
        let mut engine = state.torrent_engine.write().await;
        engine.simulate_download_progress();
    }

    // Parse info hash
    let info_hash = match parse_info_hash(&info_hash_str) {
        Ok(hash) => hash,
        Err(_) => return Err(StatusCode::BAD_REQUEST),
    };

    // Get torrent session
    let engine = state.torrent_engine.read().await;
    let session = match engine.get_session(info_hash) {
        Ok(session) => session,
        Err(_) => return Err(StatusCode::NOT_FOUND),
    };

    // For demo, create fake video data based on completed pieces
    let total_size = session.piece_count as u64 * session.piece_size as u64;
    let completed_pieces = session.completed_pieces.iter().filter(|&&x| x).count() as u64;
    let available_size = completed_pieces * session.piece_size as u64;

    // Handle range requests for video streaming
    let range_header = headers.get("range");
    let (start, end, _content_length) = if let Some(range) = range_header {
        parse_range_header(range.to_str().unwrap_or(""), available_size)
    } else {
        (0, available_size.saturating_sub(1), available_size)
    };

    // Ensure we don't serve data beyond what's downloaded
    let safe_end = end.min(available_size.saturating_sub(1));
    let safe_length = safe_end.saturating_sub(start) + 1;

    if start > available_size {
        return Err(StatusCode::RANGE_NOT_SATISFIABLE);
    }

    // Get video data - real file if available, otherwise fake data
    let video_data = if let Some(ref movie_manager) = state.movie_manager {
        let manager = movie_manager.read().await;
        match manager
            .read_file_segment(info_hash, start, safe_length)
            .await
        {
            Ok(data) => data,
            Err(_) => create_fake_video_segment(start, safe_length),
        }
    } else {
        create_fake_video_segment(start, safe_length)
    };

    let mut response = Response::builder()
        .header("Content-Type", "video/mp4")
        .header("Accept-Ranges", "bytes")
        .header("Content-Length", safe_length.to_string())
        .header("Cache-Control", "no-cache");

    // Add range response headers if this is a range request
    if range_header.is_some() {
        response = response.status(StatusCode::PARTIAL_CONTENT).header(
            "Content-Range",
            format!("bytes {start}-{safe_end}/{total_size}"),
        );
    } else {
        response = response.status(StatusCode::OK);
    }

    response
        .body(Body::from(video_data))
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}

async fn api_local_movies(State(state): State<AppState>) -> Json<serde_json::Value> {
    if let Some(ref movie_manager) = state.movie_manager {
        let manager = movie_manager.read().await;
        let movies: Vec<serde_json::Value> = manager
            .all_movies()
            .iter()
            .map(|movie| {
                json!({
                    "title": movie.title,
                    "size": movie.size,
                    "file_path": movie.file_path.display().to_string(),
                    "info_hash": movie.info_hash.to_string(),
                })
            })
            .collect();

        Json(json!({
            "movies": movies,
            "total": movies.len()
        }))
    } else {
        Json(json!({
            "movies": [],
            "total": 0,
            "message": "No local movie directory configured"
        }))
    }
}

#[derive(Deserialize)]
struct AddLocalMovieQuery {
    info_hash: String,
}

async fn api_add_local_movie(
    State(state): State<AppState>,
    Query(params): Query<AddLocalMovieQuery>,
) -> Json<serde_json::Value> {
    if let Some(ref movie_manager) = state.movie_manager {
        let manager = movie_manager.read().await;

        // Parse the info hash
        if let Ok(info_hash) = parse_info_hash(&params.info_hash) {
            if let Some(movie) = manager.get_movie(info_hash) {
                // Add this movie as a simulated torrent
                let mut engine = state.torrent_engine.write().await;

                // Create a fake magnet link for the movie
                let magnet = format!(
                    "magnet:?xt=urn:btih:{}&dn={}",
                    info_hash,
                    urlencoding::encode(&movie.title)
                );

                match engine.add_magnet(&magnet).await {
                    Ok(hash) => {
                        // Start downloading immediately
                        match engine.start_download(hash).await {
                            Ok(()) => Json(json!({
                                "success": true,
                                "message": format!("Added local movie: {}", movie.title),
                                "info_hash": hash.to_string()
                            })),
                            Err(e) => Json(json!({
                                "success": false,
                                "message": format!("Failed to start download: {}", e)
                            })),
                        }
                    }
                    Err(e) => Json(json!({
                        "success": false,
                        "message": format!("Failed to add movie: {}", e)
                    })),
                }
            } else {
                Json(json!({
                    "success": false,
                    "message": "Movie not found"
                }))
            }
        } else {
            Json(json!({
                "success": false,
                "message": "Invalid info hash"
            }))
        }
    } else {
        Json(json!({
            "success": false,
            "message": "No local movie directory configured"
        }))
    }
}

/// Stream local movie file directly (bypasses torrent engine)
async fn stream_local_movie(
    State(state): State<AppState>,
    Path(info_hash_str): Path<String>,
    headers: HeaderMap,
) -> Result<Response<Body>, StatusCode> {
    // Parse info hash
    let info_hash = match parse_info_hash(&info_hash_str) {
        Ok(hash) => hash,
        Err(_) => return Err(StatusCode::BAD_REQUEST),
    };

    // Check if we have a local movie manager
    let movie_manager = match state.movie_manager.as_ref() {
        Some(manager) => manager,
        None => return Err(StatusCode::NOT_FOUND),
    };

    let manager = movie_manager.read().await;
    let movie = match manager.get_movie(info_hash) {
        Some(movie) => movie,
        None => return Err(StatusCode::NOT_FOUND),
    };

    // Handle range requests for video streaming
    let range_header = headers.get("range");
    let (start, end, __content_length) = if let Some(range) = range_header {
        parse_range_header(range.to_str().unwrap_or(""), movie.size)
    } else {
        (0, movie.size.saturating_sub(1), movie.size)
    };

    let safe_length = end.saturating_sub(start) + 1;

    if start > movie.size {
        return Err(StatusCode::RANGE_NOT_SATISFIABLE);
    }

    // Read the actual movie file segment
    let video_data = match manager
        .read_file_segment(info_hash, start, safe_length)
        .await
    {
        Ok(data) => data,
        Err(_) => return Err(StatusCode::INTERNAL_SERVER_ERROR),
    };

    // Determine content type based on file extension
    let content_type = if movie
        .file_path
        .extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| ext.to_lowercase())
        .as_deref()
        == Some("mkv")
    {
        "video/x-matroska" // MKV files - limited browser support
    } else {
        "video/mp4"
    };

    let mut response = Response::builder()
        .header("Content-Type", content_type)
        .header("Accept-Ranges", "bytes")
        .header("Content-Length", video_data.len().to_string())
        .header("Cache-Control", "no-cache");

    if range_header.is_some() {
        response = response.status(StatusCode::PARTIAL_CONTENT).header(
            "Content-Range",
            format!(
                "bytes {}-{}/{}",
                start,
                start + video_data.len() as u64 - 1,
                movie.size
            ),
        );
    }

    response
        .body(Body::from(video_data))
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}

/// Parse HTTP Range header for video streaming
fn parse_range_header(range: &str, total_size: u64) -> (u64, u64, u64) {
    if !range.starts_with("bytes=") {
        return (0, total_size.saturating_sub(1), total_size);
    }

    let range_spec = &range[6..]; // Remove "bytes="
    if let Some((start_str, end_str)) = range_spec.split_once('-') {
        let start = start_str.parse::<u64>().unwrap_or(0);
        let end = if end_str.is_empty() {
            total_size.saturating_sub(1)
        } else {
            end_str
                .parse::<u64>()
                .unwrap_or(total_size.saturating_sub(1))
        };
        let _content_length = end.saturating_sub(start) + 1;
        (start, end, _content_length)
    } else {
        (0, total_size.saturating_sub(1), total_size)
    }
}

/// Create fake video data for demo streaming
fn create_fake_video_segment(start: u64, length: u64) -> Vec<u8> {
    // Create fake MP4-like data with proper headers for demo
    let mut data = Vec::new();

    // Simple pattern that browsers might recognize as video
    for i in 0..length {
        let byte = ((start + i) % 256) as u8;
        data.push(byte);
    }

    data
}

/// Parse hex string into InfoHash
fn parse_info_hash(hex_str: &str) -> Result<riptide_core::torrent::InfoHash, String> {
    if hex_str.len() != 40 {
        return Err("Invalid hash length".to_string());
    }

    let hash_bytes = decode_hex(hex_str)?;
    if hash_bytes.len() != 20 {
        return Err("Invalid hash bytes".to_string());
    }

    let mut hash_array = [0u8; 20];
    hash_array.copy_from_slice(&hash_bytes);
    Ok(riptide_core::torrent::InfoHash::new(hash_array))
}

/// Simple hex decoder
fn decode_hex(hex_str: &str) -> Result<Vec<u8>, String> {
    if hex_str.len() % 2 != 0 {
        return Err("Invalid hex string length".to_string());
    }

    let mut bytes = Vec::new();
    for chunk in hex_str.as_bytes().chunks(2) {
        let hex_byte = std::str::from_utf8(chunk).map_err(|_| "Invalid UTF-8")?;
        let byte = u8::from_str_radix(hex_byte, 16).map_err(|_| "Invalid hex digit")?;
        bytes.push(byte);
    }

    Ok(bytes)
}
