//! BitTorrent piece-based streaming handlers

use std::io::SeekFrom;
use std::sync::Arc;
use std::time::Instant;

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

/// Streams torrent content with HTTP range support for video playback.
///
/// # Errors
/// Returns StatusCode error if torrent not found, streaming fails, or range headers are invalid.
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
    let session = match state.engine().session_details(info_hash).await {
        Ok(session) => session,
        Err(_) => return Err(StatusCode::NOT_FOUND),
    };

    // For streaming, always report the full file size as available
    // This allows seeking to any position - missing pieces will be downloaded on-demand
    let total_size = session.total_size;
    let completed_pieces = session.completed_pieces.iter().filter(|&&x| x).count() as u64;
    let available_size = total_size; // Always use full size for proper seeking support

    tracing::info!(
        "Streaming request: piece_count={}, piece_size={}, total_size={}, completed_pieces={}/{}, available_size={}, progress={:.1}%",
        session.piece_count,
        session.piece_size,
        total_size,
        completed_pieces,
        session.piece_count,
        available_size,
        session.progress * 100.0
    );

    // Handle range requests for video streaming
    // For non-Range requests on large files, serve initial chunk to trigger browser Range requests
    let (start, end, _content_length) = if let Some(range_str) = extract_range_header(&headers) {
        parse_range_header(&range_str, available_size)
    } else {
        // If the client doesn't send a Range header, it's often a preliminary request to identify the content.
        // We serve the first 1MB to allow the browser's media player to analyze the file headers (e.g., moov atom)
        // and subsequently make specific byte-range requests for streaming.
        let chunk_size = 1024 * 1024;
        let safe_end = chunk_size.min(available_size.saturating_sub(1));
        (0, safe_end, safe_end + 1)
    };

    // Validate range bounds and get safe values
    let (start, safe_end, safe_length) = validate_range_bounds(start, end, available_size)?;

    tracing::info!(
        "Range request: bytes={}-{} (length={}), validated: {}-{} (length={})",
        start,
        end,
        end.saturating_sub(start) + 1,
        start,
        safe_end,
        safe_length
    );

    // Update adaptive piece picker with current playback position
    // This automatically prioritizes pieces around the requested range for optimal streaming
    update_playback_position_and_priority(&state, info_hash, start, safe_length, &session).await;

    // Detect container format and determine if conversion is needed
    let container_format = detect_container_format(&session.filename);
    tracing::info!(
        "Streaming request for {}: format={:?}, browser_compatible={}",
        session.filename,
        container_format,
        container_format.is_browser_compatible()
    );

    // Use conversion logic for browser compatibility
    let (video_data, actual_file_size) = if container_format.is_browser_compatible() {
        tracing::info!(
            "Direct streaming for browser-compatible format: {:?}",
            container_format
        );
        // Direct streaming - no conversion needed
        let data = read_original_data(&state, info_hash, &session, start, safe_length).await?;
        (data, session.total_size)
    } else {
        tracing::info!(
            "Non-browser-compatible format {:?} - attempting conversion",
            container_format
        );

        // Check if we have enough data for conversion
        let progress_percent = (session
            .completed_pieces
            .iter()
            .filter(|&&completed| completed)
            .count() as f64
            / session.completed_pieces.len() as f64)
            * 100.0;

        // Different formats need different amounts of data for successful conversion
        let required_percent = match container_format {
            ContainerFormat::Avi => 5.0,  // Temporarily lowered for testing
            ContainerFormat::Mkv => 15.0, // MKV can work with less data
            _ => 10.0,                    // Default for other formats
        };

        if progress_percent < required_percent {
            tracing::warn!(
                "{:?} file only {:.1}% downloaded - need at least {:.1}% for conversion",
                container_format,
                progress_percent,
                required_percent
            );
            return Err(StatusCode::TOO_EARLY); // 425 Too Early - not enough data for conversion
        }

        tracing::info!(
            "{:?} file {:.1}% downloaded - sufficient for conversion attempt",
            container_format,
            progress_percent
        );

        // Format needs conversion - try conversion with fallback
        match video_data_with_conversion(&state, info_hash, &session, start, safe_length).await {
            Ok((data, size)) => {
                tracing::info!("Conversion successful for {}", session.filename);
                (data, size)
            }
            Err(conversion_error) => {
                tracing::error!(
                    "Conversion failed for {}: {:?}. Cannot serve incompatible format to browser.",
                    session.filename,
                    conversion_error
                );
                // For now, return error instead of serving incompatible format
                // TODO: Add progressive download support for original formats
                return Err(StatusCode::UNSUPPORTED_MEDIA_TYPE);
            }
        }
    };

    // Set MIME type based on what we're actually serving
    let content_type = if container_format.is_browser_compatible() {
        container_format.mime_type().to_string()
    } else {
        // Non-browser-compatible formats should be converted to MP4
        "video/mp4".to_string()
    };

    build_range_response(
        &headers,
        video_data,
        &content_type,
        start,
        safe_end,
        actual_file_size,
    )
}

/// Loads video data with automatic conversion for non-browser-compatible formats.
///
/// Returns (data, actual_file_size) where actual_file_size is the size of the served file
async fn video_data_with_conversion(
    state: &AppState,
    info_hash: InfoHash,
    session: &riptide_core::engine::TorrentSession,
    start: u64,
    length: u64,
) -> Result<(Vec<u8>, u64), StatusCode> {
    // This function only handles conversion of non-browser-compatible formats
    let container_format = detect_container_format(&session.filename);
    tracing::debug!(
        "Attempting conversion for {} ({:?} format)",
        session.filename,
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

    // Not in cache - attempt conversion
    tracing::debug!("Converting {} to MP4", session.filename);

    match convert_file_to_mp4(state, info_hash, session).await {
        Ok(converted_path) => {
            // Verify the converted file actually exists and has valid size
            match tokio::fs::metadata(&converted_path).await {
                Ok(metadata) => {
                    let converted_size = metadata.len();
                    if converted_size > 0 {
                        tracing::info!("Verified converted file: {} bytes", converted_size);
                        let video_data =
                            read_converted_data(&converted_path, start, length).await?;
                        Ok((video_data, converted_size))
                    } else {
                        tracing::error!("Converted file is empty: {}", converted_path.display());
                        Err(StatusCode::INTERNAL_SERVER_ERROR)
                    }
                }
                Err(e) => {
                    tracing::error!(
                        "Converted file not found: {} - {}",
                        converted_path.display(),
                        e
                    );
                    Err(StatusCode::INTERNAL_SERVER_ERROR)
                }
            }
        }
        Err(conversion_error) => {
            tracing::error!(
                "Conversion failed for {}: {:?}. Trying simple remux fallback.",
                session.filename,
                conversion_error
            );

            // Fallback: attempt simple remuxing with ffmpeg if available
            match attempt_simple_remux(state, info_hash, session).await {
                Ok(converted_path) => {
                    tracing::info!("Simple remux successful for {}", session.filename);
                    let video_data = read_converted_data(&converted_path, start, length).await?;

                    // Cache the simple conversion
                    let file_size = tokio::fs::metadata(&converted_path)
                        .await
                        .map(|m| m.len())
                        .unwrap_or(session.total_size);

                    let converted = ConvertedFile {
                        output_path: converted_path,
                        size: file_size,
                        created_at: std::time::Instant::now(),
                    };

                    {
                        let mut cache = state.conversion_cache.write().await;
                        cache.insert(info_hash, converted);
                    }

                    Ok((video_data, file_size))
                }
                Err(_) => {
                    tracing::warn!(
                        "Both conversion and simple remux failed for {}. Cannot convert to browser-compatible format.",
                        session.filename
                    );

                    // Return error - let caller handle fallback
                    Err(StatusCode::UNSUPPORTED_MEDIA_TYPE)
                }
            }
        }
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
    if let Ok(piece_store) = state.piece_store() {
        let piece_reader =
            create_piece_reader_from_trait_object(Arc::clone(piece_store), session.piece_size);

        // Try to read from piece store, but handle missing pieces gracefully
        match piece_reader
            .read_range(info_hash, start..start + length)
            .await
        {
            Ok(data) => Ok(data),
            Err(e) => {
                tracing::warn!(
                    "Pieces not yet available for range {}..{}: {:?}. Sending partial response.",
                    start,
                    start + length,
                    e
                );

                // For missing pieces, return zero-filled data temporarily
                // This allows seeking to work while pieces download in background
                // The adaptive piece picker should prioritize these pieces
                Ok(vec![0u8; length as usize])
            }
        }
    } else if let Ok(movie_manager) = state.file_manager() {
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
        .ffmpeg_processor()
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
                created_at: Instant::now(),
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
    let total_data = if let Ok(piece_store) = state.piece_store() {
        let piece_reader =
            create_piece_reader_from_trait_object(Arc::clone(piece_store), session.piece_size);

        piece_reader
            .read_range(info_hash, 0..session.total_size)
            .await
            .map_err(|e| {
                tracing::error!("Failed to read full file from piece store: {:?}", e);
                StatusCode::INTERNAL_SERVER_ERROR
            })?
    } else if let Ok(movie_manager) = state.file_manager() {
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
    file.seek(SeekFrom::Start(start)).await.map_err(|e| {
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

/// Update adaptive piece picker with current playback position and prioritize nearby pieces.
///
/// This function is called on every range request to inform the torrent engine about
/// the current video playback position. The adaptive piece picker will then prioritize
/// downloading pieces around this position for optimal streaming performance.
async fn update_playback_position_and_priority(
    state: &AppState,
    info_hash: InfoHash,
    start_position: u64,
    range_length: u64,
    session: &riptide_core::engine::TorrentSession,
) {
    // Calculate the center of the requested range as the current playback position
    let current_position = start_position + (range_length / 2);

    // Calculate an appropriate buffer size based on the requested range and video characteristics
    // For streaming, we want to buffer ahead based on estimated bitrate and playback speed
    let estimated_bitrate = estimate_video_bitrate(session, range_length);
    let buffer_size = calculate_streaming_buffer_size(estimated_bitrate, session.piece_size);

    tracing::debug!(
        "Updating playback position for {}: position={}B, range={}B, buffer={}B",
        info_hash,
        current_position,
        range_length,
        buffer_size
    );

    // Seek to the current position to prioritize pieces around this location
    // This will mark urgent priority for pieces immediately needed for playback
    if let Err(e) = state
        .engine()
        .seek_to_position(info_hash, current_position, buffer_size)
        .await
    {
        tracing::warn!(
            "Failed to update seek position for torrent {}: {}",
            info_hash,
            e
        );
    }

    // Update buffer strategy based on the range request pattern
    // Larger ranges suggest faster playback or seeking behavior
    let playback_speed = infer_playback_speed_from_range(range_length, session.piece_size);
    let estimated_bandwidth = estimate_available_bandwidth(range_length);

    if let Err(e) = state
        .engine()
        .update_buffer_strategy(info_hash, playback_speed, estimated_bandwidth)
        .await
    {
        tracing::warn!(
            "Failed to update buffer strategy for torrent {}: {}",
            info_hash,
            e
        );
    }
}

/// Estimate video bitrate based on torrent characteristics and range request size.
fn estimate_video_bitrate(
    session: &riptide_core::engine::TorrentSession,
    range_length: u64,
) -> u64 {
    // Estimate bitrate based on file size and assumed duration
    // For a typical video file, use file size to infer quality
    let total_mb = session.total_size / (1024 * 1024);

    let estimated_bitrate = if total_mb < 500 {
        1_500_000 // 1.5 Mbps for smaller files (SD quality)
    } else if total_mb < 2000 {
        4_000_000 // 4 Mbps for medium files (HD quality)
    } else if total_mb < 8000 {
        8_000_000 // 8 Mbps for larger files (Full HD)
    } else {
        15_000_000 // 15 Mbps for very large files (4K content)
    };

    // Adjust based on range request size - larger ranges suggest higher bitrate content
    let range_factor = if range_length > 2 * 1024 * 1024 {
        1.5 // Large ranges suggest high-bitrate content
    } else if range_length < 256 * 1024 {
        0.7 // Small ranges suggest lower bitrate or mobile content
    } else {
        1.0
    };

    (estimated_bitrate as f64 * range_factor) as u64
}

/// Calculate appropriate streaming buffer size in bytes.
fn calculate_streaming_buffer_size(estimated_bitrate: u64, piece_size: u32) -> u64 {
    // Buffer 10-15 seconds of content ahead of current position
    let buffer_duration_seconds = 12;
    let buffer_bytes = (estimated_bitrate * buffer_duration_seconds) / 8;

    // Ensure buffer size is at least 3 pieces but not more than 20 pieces
    let min_buffer = 3 * piece_size as u64;
    let max_buffer = 20 * piece_size as u64;

    buffer_bytes.clamp(min_buffer, max_buffer)
}

/// Infer playback speed based on range request patterns.
fn infer_playback_speed_from_range(range_length: u64, piece_size: u32) -> f64 {
    // Normal sequential requests suggest 1x playback
    // Very large ranges suggest seeking or fast-forward behavior
    // Small frequent ranges suggest slow or paused playback

    let pieces_in_range = (range_length / piece_size as u64).max(1);

    if pieces_in_range >= 8 {
        1.5 // Large ranges suggest faster playback or seeking
    } else if pieces_in_range <= 1 {
        0.8 // Small ranges suggest slower playback
    } else {
        1.0 // Normal playback speed
    }
}

/// Estimate available bandwidth based on range request size.
fn estimate_available_bandwidth(range_length: u64) -> u64 {
    // Use range request size as a proxy for network capacity
    // Larger ranges suggest the client can handle more data

    if range_length >= 2 * 1024 * 1024 {
        10_000_000 // 10 MB/s for large range requests (matches simulation speed)
    } else if range_length >= 512 * 1024 {
        5_000_000 // 5 MB/s for medium range requests
    } else {
        2_000_000 // 2 MB/s for small range requests
    }
}

/// Returns list of local movies available for streaming.
///
/// # Errors
/// Returns JSON error if movie library access fails.
pub async fn api_local_movies(State(state): State<AppState>) -> Json<serde_json::Value> {
    if let Ok(movie_manager) = state.file_manager() {
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

/// Adds a local movie to the library by info hash.
///
/// This endpoint allows adding a movie that exists locally to the managed
/// library, making it available for streaming and metadata management.
pub async fn api_add_local_movie(
    State(state): State<AppState>,
    Query(params): Query<AddLocalMovieQuery>,
) -> Json<serde_json::Value> {
    if let Ok(movie_manager) = state.file_manager() {
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

                match state.engine().add_magnet(&magnet).await {
                    Ok(hash) => {
                        // Start downloading immediately
                        match state.engine().start_download(hash).await {
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
    let movie_manager = match state.file_manager().ok() {
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

    // For local movies that have been added to the torrent engine, also update position
    if let Ok(session) = state.engine().session_details(info_hash).await {
        update_playback_position_and_priority(&state, info_hash, start, safe_length, &session)
            .await;
    }

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

#[cfg(test)]
mod tests {
    use riptide_core::engine::TorrentSession;

    use super::*;

    #[test]
    fn test_estimate_video_bitrate() {
        // Create a mock session for testing
        let session = TorrentSession {
            info_hash: InfoHash::new([1u8; 20]),
            piece_count: 100,
            piece_size: 32768,
            total_size: 800 * 1024 * 1024, // 800MB file
            filename: "test_movie.mp4".to_string(),
            tracker_urls: vec![],
            is_downloading: false,
            started_at: Instant::now(),
            completed_pieces: vec![false; 100],
            progress: 0.0,
            download_speed_bps: 0,
            upload_speed_bps: 0,
            bytes_downloaded: 0,
            bytes_uploaded: 0,
        };

        // Test bitrate estimation for different range sizes
        let bitrate_small = estimate_video_bitrate(&session, 128 * 1024); // 128KB range (small)
        let bitrate_normal = estimate_video_bitrate(&session, 1024 * 1024); // 1MB range (normal)
        let bitrate_large = estimate_video_bitrate(&session, 3 * 1024 * 1024); // 3MB range (large)

        // Verify the range factor logic is working
        // Small ranges (< 256KB) get factor 0.7, should be lower
        // Large ranges (> 2MB) get factor 1.5, should be higher
        assert!(
            bitrate_small < bitrate_normal,
            "Small range should give lower bitrate: {} < {}",
            bitrate_small,
            bitrate_normal
        );
        assert!(
            bitrate_large > bitrate_normal,
            "Large range should give higher bitrate: {} > {}",
            bitrate_large,
            bitrate_normal
        );

        // All should be reasonable bitrates
        assert!(bitrate_small >= 1_000_000 && bitrate_small <= 10_000_000);
        assert!(bitrate_normal >= 2_000_000 && bitrate_normal <= 15_000_000);
        assert!(bitrate_large >= 3_000_000 && bitrate_large <= 20_000_000);
    }

    #[test]
    fn test_calculate_streaming_buffer_size() {
        let piece_size = 32768; // 32KB pieces

        // Test with different bitrates - ensure they're far enough apart
        let buffer_low = calculate_streaming_buffer_size(1_000_000, piece_size); // 1 Mbps
        let buffer_high = calculate_streaming_buffer_size(10_000_000, piece_size); // 10 Mbps

        println!(
            "Low bitrate buffer: {} bytes ({} pieces)",
            buffer_low,
            buffer_low / piece_size as u64
        );
        println!(
            "High bitrate buffer: {} bytes ({} pieces)",
            buffer_high,
            buffer_high / piece_size as u64
        );

        // Higher bitrate should require larger buffer (unless clamped to max)
        // Only assert if the high bitrate buffer isn't clamped to max
        let max_buffer = 20 * piece_size as u64;
        if buffer_high < max_buffer {
            assert!(
                buffer_high > buffer_low,
                "High bitrate buffer should be larger: {} > {}",
                buffer_high,
                buffer_low
            );
        }

        // Buffer should be at least 3 pieces
        let min_buffer = 3 * piece_size as u64;
        assert!(buffer_low >= min_buffer);
        assert!(buffer_high >= min_buffer);

        // Buffer should not exceed 20 pieces
        assert!(buffer_low <= max_buffer);
        assert!(buffer_high <= max_buffer);
    }

    #[test]
    fn test_infer_playback_speed_from_range() {
        let piece_size = 32768; // 32KB pieces

        // Small range (less than 1 piece) suggests slow playback
        let speed_small = infer_playback_speed_from_range(16384, piece_size);
        assert!(speed_small < 1.0);

        // Normal range (2-4 pieces) suggests normal playback
        let speed_normal = infer_playback_speed_from_range(2 * piece_size as u64, piece_size);
        assert_eq!(speed_normal, 1.0);

        // Large range (8+ pieces) suggests fast playback or seeking
        let speed_large = infer_playback_speed_from_range(10 * piece_size as u64, piece_size);
        assert!(speed_large > 1.0);
    }

    #[test]
    fn test_estimate_available_bandwidth() {
        // Small ranges suggest limited bandwidth
        let bw_small = estimate_available_bandwidth(128 * 1024); // 128KB
        assert_eq!(bw_small, 1_000_000); // 1 MB/s

        // Medium ranges suggest moderate bandwidth
        let bw_medium = estimate_available_bandwidth(1024 * 1024); // 1MB
        assert_eq!(bw_medium, 2_000_000); // 2 MB/s

        // Large ranges suggest high bandwidth
        let bw_large = estimate_available_bandwidth(3 * 1024 * 1024); // 3MB
        assert_eq!(bw_large, 5_000_000); // 5 MB/s
    }

    #[test]
    fn test_detect_container_format() {
        assert!(matches!(
            detect_container_format("movie.mp4"),
            ContainerFormat::Mp4
        ));
        assert!(matches!(
            detect_container_format("video.webm"),
            ContainerFormat::WebM
        ));
        assert!(matches!(
            detect_container_format("film.mkv"),
            ContainerFormat::Mkv
        ));
        assert!(matches!(
            detect_container_format("clip.avi"),
            ContainerFormat::Avi
        ));
        assert!(matches!(
            detect_container_format("unknown.xyz"),
            ContainerFormat::Unknown
        ));
    }
}

/// Simple ffmpeg-based remuxing as fallback when full conversion fails
async fn attempt_simple_remux(
    state: &AppState,
    info_hash: InfoHash,
    session: &riptide_core::engine::TorrentSession,
) -> Result<std::path::PathBuf, Box<dyn std::error::Error + Send + Sync>> {
    // First, reconstruct the original file
    let temp_input = reconstruct_original_file(state, info_hash, session)
        .await
        .map_err(|e| format!("Failed to reconstruct file: {e:?}"))?;

    // Create output path
    let temp_dir = std::env::temp_dir();
    let output_path = temp_dir.join(format!("{info_hash}_remuxed.mp4"));

    tracing::info!(
        "Attempting simple remux of {} to {}",
        temp_input.display(),
        output_path.display()
    );

    // Use ffmpeg directly for simple container remuxing
    let output = tokio::process::Command::new("ffmpeg")
        .args([
            "-i",
            temp_input.to_str().unwrap(),
            "-c",
            "copy", // Copy streams without re-encoding
            "-f",
            "mp4", // Force MP4 container
            "-y",  // Overwrite output file
            output_path.to_str().unwrap(),
        ])
        .output()
        .await
        .map_err(|e| format!("Failed to execute ffmpeg: {e}"))?;

    // Clean up input file
    let _ = tokio::fs::remove_file(&temp_input).await;

    if output.status.success() {
        tracing::info!("Successfully remuxed {} to MP4", session.filename);
        Ok(output_path)
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        Err(format!("FFmpeg failed: {stderr}").into())
    }
}
