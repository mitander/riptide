//! Streaming handlers for video content and local movies

use axum::body::Body;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{Json, Response};
use serde::Deserialize;
use serde_json::json;

use crate::handlers::utils::{create_fake_video_segment, parse_info_hash, parse_range_header};
use crate::server::AppState;

pub async fn stream_torrent(
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

pub async fn api_local_movies(State(state): State<AppState>) -> Json<serde_json::Value> {
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
pub struct AddLocalMovieQuery {
    pub info_hash: String,
}

pub async fn api_add_local_movie(
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
pub async fn stream_local_movie(
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
