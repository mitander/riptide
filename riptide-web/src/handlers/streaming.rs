//! BitTorrent piece-based streaming handlers

use std::sync::Arc;

use axum::body::Body;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{Json, Response};
use riptide_core::streaming::PieceBasedStreamReader;
use riptide_core::torrent::InfoHash;
use serde::Deserialize;
use serde_json::json;

use crate::handlers::range::{
    build_range_response, extract_range_header, parse_range_header, validate_range_bounds,
};
use crate::server::{AppState, PieceStoreType};

pub async fn stream_torrent(
    State(state): State<AppState>,
    Path(info_hash_str): Path<String>,
    headers: HeaderMap,
) -> Result<Response<Body>, StatusCode> {
    // Update download progress first
    state.torrent_engine.simulate_download_progress();

    let info_hash = match InfoHash::from_hex(&info_hash_str) {
        Ok(hash) => hash,
        Err(_) => return Err(StatusCode::BAD_REQUEST),
    };

    // Get torrent session
    let session = match state.torrent_engine.get_session(info_hash).await {
        Ok(session) => session,
        Err(_) => return Err(StatusCode::NOT_FOUND),
    };

    // For demo, create fake video data based on completed pieces
    let total_size = session.piece_count as u64 * session.piece_size as u64;
    let completed_pieces = session.completed_pieces.iter().filter(|&&x| x).count() as u64;
    let available_size = completed_pieces * session.piece_size as u64;

    // Handle range requests for video streaming
    let (start, end, _content_length) = if let Some(range_str) = extract_range_header(&headers) {
        parse_range_header(&range_str, available_size)
    } else {
        (0, available_size.saturating_sub(1), available_size)
    };

    // Validate range bounds and get safe values
    let (start, safe_end, safe_length) = validate_range_bounds(start, end, available_size)?;

    // Use BitTorrent piece-based streaming if available
    let video_data = if let Some(ref piece_store) = state.piece_store {
        match piece_store {
            PieceStoreType::Simulation(sim_store) => {
                // Create piece reader with session piece size
                let piece_reader =
                    PieceBasedStreamReader::new(Arc::clone(sim_store), session.piece_size as u32);

                // Read the requested range using piece reconstruction
                piece_reader
                    .read_range(info_hash, start..start + safe_length)
                    .await
                    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
            }
        }
    } else if let Some(ref movie_manager) = state.movie_manager {
        // Use local file reading
        let manager = movie_manager.read().await;
        manager
            .read_file_segment(info_hash, start, safe_length)
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
    } else {
        // No data source available - this is a configuration error
        return Err(StatusCode::INTERNAL_SERVER_ERROR);
    };

    build_range_response(
        &headers,
        video_data,
        "video/mp4",
        start,
        safe_end,
        total_size,
    )
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
        if let Ok(info_hash) = InfoHash::from_hex(&params.info_hash) {
            if let Some(movie) = manager.get_movie(info_hash) {
                // Add this movie as a simulated torrent
                // Create a fake magnet link for the movie
                let magnet = format!(
                    "magnet:?xt=urn:btih:{}&dn={}",
                    info_hash,
                    urlencoding::encode(&movie.title)
                );

                match state.torrent_engine.add_magnet(&magnet).await {
                    Ok(hash) => {
                        // Start downloading immediately
                        match state.torrent_engine.start_download(hash).await {
                            Ok(()) => Json(json!({
                                "success": true,
                                "message": format!("Added local movie: {}", movie.title),
                                "info_hash": hash.to_string()
                            })),
                            Err(e) => Json(json!({
                                "success": false,
                                "message": format!("Failed to start download: {e}")
                            })),
                        }
                    }
                    Err(e) => Json(json!({
                        "success": false,
                        "message": format!("Failed to add movie: {e}")
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
    let info_hash = match InfoHash::from_hex(&info_hash_str) {
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
    let (start, end, _content_length) = if let Some(range_str) = extract_range_header(&headers) {
        parse_range_header(&range_str, movie.size)
    } else {
        (0, movie.size.saturating_sub(1), movie.size)
    };

    // Validate range bounds and get safe values
    let (start, safe_end, safe_length) = validate_range_bounds(start, end, movie.size)?;

    // Check if the file format is supported by browsers
    let file_extension = movie
        .file_path
        .extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| ext.to_lowercase());

    // Set content type based on file extension, but serve all formats
    // Let the browser decide what it can handle
    let content_type = match file_extension.as_deref() {
        Some("mp4") | Some("m4v") => "video/mp4",
        Some("webm") => "video/webm",
        Some("ogg") | Some("ogv") => "video/ogg",
        Some("mkv") => "video/x-matroska",
        Some("avi") => "video/x-msvideo",
        Some("mov") => "video/quicktime",
        Some("flv") => "video/x-flv",
        Some("wmv") => "video/x-ms-wmv",
        Some("3gp") => "video/3gpp",
        Some("ts") | Some("m2ts") => "video/mp2t",
        _ => "video/mp4", // Default to MP4 for unknown extensions
    };

    println!(
        "STREAMING: {} ({:?}) as {}",
        movie.title, file_extension, content_type
    );

    // Read the actual movie file segment
    let video_data = match manager
        .read_file_segment(info_hash, start, safe_length)
        .await
    {
        Ok(data) => data,
        Err(_) => return Err(StatusCode::INTERNAL_SERVER_ERROR),
    };

    build_range_response(
        &headers,
        video_data,
        content_type,
        start,
        safe_end,
        movie.size,
    )
}
