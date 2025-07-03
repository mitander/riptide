//! BitTorrent piece-based streaming handlers

use std::sync::Arc;

use axum::body::Body;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{Json, Response};
use riptide_core::streaming::{
    ContainerFormat, RemuxingOptions, create_piece_reader_from_trait_object,
};
use riptide_core::torrent::InfoHash;
use serde::Deserialize;
use serde_json::json;

use crate::handlers::range::{
    build_range_response, extract_range_header, parse_range_header, validate_range_bounds,
};
use crate::server::{AppState, ConvertedFile};

/// Detect container format from filename extension
fn detect_container_format(filename: &str) -> ContainerFormat {
    let extension = std::path::Path::new(filename)
        .extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| ext.to_lowercase());

    match extension.as_deref() {
        Some("mp4") | Some("m4v") => ContainerFormat::Mp4,
        Some("webm") => ContainerFormat::WebM,
        Some("mkv") => ContainerFormat::Mkv,
        Some("avi") => ContainerFormat::Avi,
        Some("mov") => ContainerFormat::Mov,
        _ => ContainerFormat::Unknown,
    }
}

/// Determine output MIME type for browser streaming
fn determine_content_type(filename: &str) -> String {
    let format = detect_container_format(filename);

    // For non-browser-compatible formats, we'll serve as MP4 after remuxing
    if format.is_browser_compatible() {
        format.mime_type().to_string()
    } else {
        // Non-compatible formats (MKV, AVI, etc.) will be remuxed to MP4
        "video/mp4".to_string()
    }
}

pub async fn stream_torrent(
    State(state): State<AppState>,
    Path(info_hash_str): Path<String>,
    headers: HeaderMap,
) -> Result<Response<Body>, StatusCode> {
    let info_hash = match InfoHash::from_hex(&info_hash_str) {
        Ok(hash) => hash,
        Err(_) => return Err(StatusCode::BAD_REQUEST),
    };

    // Get torrent session
    let session = match state.torrent_engine.get_session(info_hash).await {
        Ok(session) => session,
        Err(_) => return Err(StatusCode::NOT_FOUND),
    };

    // Calculate available size based on completed pieces
    let total_size = session.total_size;
    let completed_pieces = session.completed_pieces.iter().filter(|&&x| x).count() as u64;
    let available_size = if completed_pieces == session.piece_count as u64 {
        total_size
    } else {
        completed_pieces * session.piece_size as u64
    };

    tracing::debug!(
        "Streaming info: piece_count={}, piece_size={}, total_size={}, completed_pieces={}, available_size={}",
        session.piece_count,
        session.piece_size,
        total_size,
        completed_pieces,
        available_size
    );

    // Handle range requests for video streaming
    // For non-Range requests on large files, serve initial chunk to trigger browser Range requests
    let (start, end, _content_length) = if let Some(range_str) = extract_range_header(&headers) {
        parse_range_header(&range_str, available_size)
    } else {
        // For non-Range requests, serve first 1MB to allow browser to analyze file
        let chunk_size = 1024 * 1024; // 1MB
        let safe_end = chunk_size.min(available_size.saturating_sub(1));
        (0, safe_end, safe_end + 1)
    };

    // Validate range bounds and get safe values
    let (start, safe_end, safe_length) = validate_range_bounds(start, end, available_size)?;

    // Check if container format needs remuxing and handle conversion
    let (video_data, actual_file_size) =
        get_video_data_with_conversion(&state, info_hash, &session, start, safe_length).await?;

    // Determine MIME type from filename
    let content_type = determine_content_type(&session.filename);

    build_range_response(
        &headers,
        video_data,
        &content_type,
        start,
        safe_end,
        actual_file_size,
    )
}

/// Get video data with automatic conversion for non-browser-compatible formats
/// Returns (data, actual_file_size) where actual_file_size is the size of the served file
async fn get_video_data_with_conversion(
    state: &AppState,
    info_hash: InfoHash,
    session: &riptide_core::engine::TorrentSession,
    start: u64,
    length: u64,
) -> Result<(Vec<u8>, u64), StatusCode> {
    let container_format = detect_container_format(&session.filename);

    if container_format.is_browser_compatible() {
        // Direct streaming for MP4, WebM, etc. - no conversion needed
        let video_data = read_original_data(state, info_hash, session, start, length).await?;
        Ok((video_data, session.total_size))
    } else {
        // Format needs conversion (MKV -> MP4, etc.)
        tracing::debug!(
            "Container format {:?} needs conversion for browser compatibility",
            container_format
        );

        // Check cache first
        {
            let cache = state.conversion_cache.read().await;
            if let Some(converted) = cache.get(&info_hash) {
                tracing::debug!("Using cached converted file for {}", info_hash);
                let video_data = read_converted_data(&converted.output_path, start, length).await?;
                return Ok((video_data, converted.size));
            }
        }

        // Not in cache - need to convert
        tracing::debug!(
            "Converting {} from {:?} to MP4",
            session.filename,
            container_format
        );
        let converted_path = convert_file_to_mp4(state, info_hash, session).await?;

        // Get the converted file size from cache
        let cache = state.conversion_cache.read().await;
        let converted_size = cache
            .get(&info_hash)
            .map(|c| c.size)
            .unwrap_or(session.total_size);

        // Read from converted file
        let video_data = read_converted_data(&converted_path, start, length).await?;
        Ok((video_data, converted_size))
    }
}

/// Read data from original source (piece store or local file)
async fn read_original_data(
    state: &AppState,
    info_hash: InfoHash,
    session: &riptide_core::engine::TorrentSession,
    start: u64,
    length: u64,
) -> Result<Vec<u8>, StatusCode> {
    if let Some(ref piece_store) = state.piece_store {
        let piece_reader =
            create_piece_reader_from_trait_object(Arc::clone(piece_store), session.piece_size);

        piece_reader
            .read_range(info_hash, start..start + length)
            .await
            .map_err(|e| {
                tracing::error!("Failed to read from piece store: {:?}", e);
                StatusCode::INTERNAL_SERVER_ERROR
            })
    } else if let Some(ref movie_manager) = state.movie_manager {
        let manager = movie_manager.read().await;
        manager
            .read_file_segment(info_hash, start, length)
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
    } else {
        Err(StatusCode::INTERNAL_SERVER_ERROR)
    }
}

/// Convert file to MP4 and cache the result
async fn convert_file_to_mp4(
    state: &AppState,
    info_hash: InfoHash,
    session: &riptide_core::engine::TorrentSession,
) -> Result<std::path::PathBuf, StatusCode> {
    // First, we need to reconstruct the original file for FFmpeg
    let temp_input = reconstruct_original_file(state, info_hash, session).await?;

    // Create output path
    let temp_dir = std::env::temp_dir();
    let output_path = temp_dir.join(format!("{info_hash}_converted.mp4"));

    // Configure remuxing options (container-only, no re-encoding)
    let config = RemuxingOptions::default();

    // Perform conversion
    match state
        .ffmpeg_processor
        .remux_to_mp4(&temp_input, &output_path, &config)
        .await
    {
        Ok(result) => {
            tracing::debug!(
                "Successfully converted {} to MP4 in {:.2}s (output size: {} bytes)",
                session.filename,
                result.processing_time,
                result.output_size
            );

            // Cache the result
            let converted = ConvertedFile {
                output_path: output_path.clone(),
                size: result.output_size,
                created_at: std::time::Instant::now(),
            };

            {
                let mut cache = state.conversion_cache.write().await;
                cache.insert(info_hash, converted);
            }

            // Clean up temporary input file
            let _ = tokio::fs::remove_file(&temp_input).await;

            Ok(output_path)
        }
        Err(e) => {
            tracing::error!("Failed to convert {}: {:?}", session.filename, e);
            // Clean up temporary input file
            let _ = tokio::fs::remove_file(&temp_input).await;
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

/// Reconstruct the original file from pieces for FFmpeg processing
async fn reconstruct_original_file(
    state: &AppState,
    info_hash: InfoHash,
    session: &riptide_core::engine::TorrentSession,
) -> Result<std::path::PathBuf, StatusCode> {
    let temp_dir = std::env::temp_dir();
    let temp_path = temp_dir.join(format!(
        "{}_original{}",
        info_hash,
        std::path::Path::new(&session.filename)
            .extension()
            .map(|ext| format!(".{}", ext.to_string_lossy()))
            .unwrap_or_default()
    ));

    // Read entire file from piece store
    let total_data = if let Some(ref piece_store) = state.piece_store {
        let piece_reader =
            create_piece_reader_from_trait_object(Arc::clone(piece_store), session.piece_size);

        piece_reader
            .read_range(info_hash, 0..session.total_size)
            .await
            .map_err(|e| {
                tracing::error!("Failed to read full file from piece store: {:?}", e);
                StatusCode::INTERNAL_SERVER_ERROR
            })?
    } else if let Some(ref movie_manager) = state.movie_manager {
        let manager = movie_manager.read().await;
        manager
            .read_file_segment(info_hash, 0, session.total_size)
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
    } else {
        return Err(StatusCode::INTERNAL_SERVER_ERROR);
    };

    // Write to temporary file
    tokio::fs::write(&temp_path, total_data)
        .await
        .map_err(|e| {
            tracing::error!("Failed to write temporary file: {:?}", e);
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

    Ok(temp_path)
}

/// Read data from converted MP4 file
async fn read_converted_data(
    file_path: &std::path::Path,
    start: u64,
    length: u64,
) -> Result<Vec<u8>, StatusCode> {
    let file = tokio::fs::File::open(file_path).await.map_err(|e| {
        tracing::error!("Failed to open converted file {:?}: {:?}", file_path, e);
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    use tokio::io::{AsyncReadExt, AsyncSeekExt};
    let mut file = file;

    // Seek to start position
    file.seek(std::io::SeekFrom::Start(start))
        .await
        .map_err(|e| {
            tracing::error!("Failed to seek in converted file: {:?}", e);
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

    // Read the requested length (use read instead of read_exact to handle EOF gracefully)
    let mut buffer = vec![0u8; length as usize];
    let bytes_read = file.read(&mut buffer).await.map_err(|e| {
        tracing::error!("Failed to read from converted file: {:?}", e);
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    if bytes_read != length as usize {
        buffer.truncate(bytes_read);
    }

    Ok(buffer)
}

pub async fn api_local_movies(State(state): State<AppState>) -> Json<serde_json::Value> {
    if let Some(ref movie_manager) = state.movie_manager {
        let manager = movie_manager.read().await;
        let movies: Vec<serde_json::Value> = manager
            .all_files()
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

/// Query parameters for adding a local movie to the library.
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
            if let Some(movie) = manager.file_by_hash(info_hash) {
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
    let movie = match manager.file_by_hash(info_hash) {
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
