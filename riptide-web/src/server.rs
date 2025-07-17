//! Modern JSON API server for Riptide
//!
//! Provides a comprehensive REST API for torrent management and streaming.
//! Serves a single-page application for the web interface.

use std::collections::HashMap;
use std::sync::Arc;

use axum::Router;
use axum::extract::State;
use riptide_core::config::RiptideConfig;
use riptide_core::server_components::{ConversionProgress, ServerComponents};
use riptide_core::storage::{DataSource, PieceDataSource};
use riptide_core::streaming::PieceProviderAdapter;
use riptide_core::torrent::{InfoHash, PieceStore, TorrentEngineHandle};
use riptide_core::{FileLibrary, HttpStreaming, RuntimeMode};
use riptide_search::MediaSearch;
use tokio::sync::RwLock;
use tower_http::cors::CorsLayer;

/// Type alias for conversion progress tracking
type ConversionProgressRef = Arc<RwLock<HashMap<String, ConversionProgress>>>;

use crate::handlers::streaming::{
    cleanup_sessions, debug_stream_data, debug_stream_status, stream_torrent, streaming_health,
    streaming_stats,
};
use crate::handlers::streaming_readiness::streaming_readiness_handler;
use crate::handlers::{
    api_add_torrent, api_dashboard_activity, api_dashboard_downloads, api_download_torrent,
    api_library, api_search, api_search_movies, api_seek_torrent, api_settings, api_stats,
    api_system_status, api_torrents,
};

// Removed PieceStoreType enum - using trait objects instead

/// Cache entry for converted files
#[derive(Debug, Clone)]
pub struct ConvertedFile {
    /// Path to the converted output file
    pub output_path: std::path::PathBuf,
    /// Size of the converted file in bytes
    pub size: u64,
    /// When this conversion was created
    pub created_at: std::time::Instant,
}

/// Unified app state that works with both production and simulation engines.
/// All services are provided by CLI dependency injection - web layer is mode-agnostic.
#[derive(Clone)]
pub struct AppState {
    /// Core server components (engine, file library, etc.)
    pub services: Arc<ServerComponents>,
    /// Media search functionality
    pub media_search: MediaSearch,
    /// Cache of converted files by info hash
    pub conversion_cache: Arc<RwLock<HashMap<InfoHash, ConvertedFile>>>,
    /// Data source for torrent piece access
    pub data_source: Arc<dyn DataSource>,
    /// When this server instance was started
    pub server_started_at: std::time::Instant,
}

impl AppState {
    /// Get the torrent engine handle.
    pub fn engine(&self) -> &TorrentEngineHandle {
        self.services.engine()
    }

    /// Creates an HttpStreaming instance for the specified torrent.
    ///
    /// This method creates a new streaming session bound to a specific
    /// torrent by wrapping the data source with a PieceProviderAdapter.
    ///
    /// # Errors
    ///
    /// - `StreamingError::FormatDetectionFailed` - Cannot read header or detect format
    pub async fn create_http_streaming(
        &self,
        info_hash: InfoHash,
    ) -> Result<HttpStreaming, riptide_core::StreamingError> {
        let adapter = Arc::new(PieceProviderAdapter::new(
            self.data_source.clone(),
            info_hash,
        ));
        HttpStreaming::new(adapter).await
    }

    /// Get the file manager if available (Development mode only).
    ///
    /// # Errors
    ///
    /// - `ServiceError::NotAvailable` - If file manager is not available in this mode
    pub fn file_library(
        &self,
    ) -> Result<&Arc<RwLock<FileLibrary>>, riptide_core::server_components::ServiceError> {
        self.services.file_library()
    }

    /// Get the piece store if available (Development mode only).
    ///
    /// # Errors
    ///
    /// - `ServiceError::NotAvailable` - If piece store is not available in this mode
    pub fn piece_store(
        &self,
    ) -> Result<&Arc<dyn PieceStore>, riptide_core::server_components::ServiceError> {
        self.services.piece_store()
    }

    /// Get conversion progress tracker if available (Development mode only).
    ///
    /// # Errors
    ///
    /// - `ServiceError::NotAvailable` - If conversion tracking is not available in this mode
    pub fn conversion_progress(
        &self,
    ) -> Result<&ConversionProgressRef, riptide_core::server_components::ServiceError> {
        self.services.conversion_progress()
    }

    // Get the FFmpeg processor for remuxing operations.
    // TODO: Implement ffmpeg processor access when available
    // pub fn ffmpeg(&self) -> &Arc<dyn FfmpegProcessor> {
    //     self.services.ffmpeg()
    // }
}

/// Starts the Riptide web server with the provided configuration and components.
///
/// # Errors
///
/// - `std::io::Error` - If server fails to bind to address or start successfully
///
/// # Panics
///
/// Panics if the server components don't include a required piece store.
pub async fn run_server(
    _config: RiptideConfig,
    components: ServerComponents,
    media_search: MediaSearch,
    _runtime_mode: RuntimeMode,
    host: String,
    port: u16,
) -> Result<(), Box<dyn std::error::Error>> {
    let conversion_cache = Arc::new(RwLock::new(HashMap::new()));

    // Create streaming service components
    let piece_store = components.piece_store().unwrap();
    let data_source: Arc<dyn DataSource> =
        Arc::new(PieceDataSource::new(piece_store.clone(), Some(100)));

    let state = AppState {
        services: Arc::new(components),
        media_search,
        conversion_cache,
        data_source,
        server_started_at: std::time::Instant::now(),
    };

    let app = Router::new()
        // Single page application entry point
        .route("/", axum::routing::get(serve_spa))
        // JSON API endpoints
        .route("/api/dashboard/stats", axum::routing::get(api_stats))
        .route(
            "/api/dashboard/activity",
            axum::routing::get(api_dashboard_activity),
        )
        .route(
            "/api/dashboard/downloads",
            axum::routing::get(api_dashboard_downloads),
        )
        .route("/api/system/status", axum::routing::get(api_system_status))
        // Streaming endpoints
        .route("/stream/{info_hash}", axum::routing::get(stream_torrent))
        .route(
            "/stream/{info_hash}/ready",
            axum::routing::get(streaming_readiness_handler),
        )
        .route("/stream/health", axum::routing::get(streaming_health))
        .route("/stream/stats", axum::routing::get(streaming_stats))
        .route("/stream/cleanup", axum::routing::post(cleanup_sessions))
        .route(
            "/debug/stream/{info_hash}/status",
            axum::routing::get(debug_stream_status),
        )
        .route(
            "/debug/stream/{info_hash}/data",
            axum::routing::get(debug_stream_data),
        )
        // JSON API endpoints (for external clients)
        .route("/api/stats", axum::routing::get(api_stats))
        .route("/api/torrents", axum::routing::get(api_torrents))
        .route("/api/torrents/add", axum::routing::post(api_add_torrent))
        .route("/api/download", axum::routing::post(api_download_torrent))
        .route("/api/library", axum::routing::get(api_library))
        .route("/api/search", axum::routing::get(api_search))
        .route("/api/search/movies", axum::routing::get(api_search_movies))
        .route("/api/settings", axum::routing::get(api_settings))
        .route(
            "/api/torrents/{info_hash}/seek",
            axum::routing::post(api_seek_torrent),
        )
        .route(
            "/api/conversions/progress",
            axum::routing::get(api_conversion_progress),
        )
        .layer(CorsLayer::permissive())
        .with_state(state);

    println!("Riptide media server running on http://{host}:{port}");
    let listener = tokio::net::TcpListener::bind(format!("{host}:{port}")).await?;
    axum::serve(listener, app).await?;
    Ok(())
}

/// API endpoint to get current conversion progress
///
/// Returns conversion progress for all active conversion jobs.
/// Returns empty map if conversion tracking is not available.
async fn api_conversion_progress(
    State(state): State<AppState>,
) -> axum::Json<HashMap<String, ConversionProgress>> {
    match state.conversion_progress() {
        Ok(progress_tracker) => {
            let progress = progress_tracker.read().await;
            axum::Json(progress.clone())
        }
        Err(_) => axum::Json(HashMap::new()),
    }
}

/// Serves the single page application index.html file.
///
/// Returns the main SPA entry point that will handle all frontend routing
/// and API communication via JavaScript.
async fn serve_spa() -> axum::response::Html<&'static str> {
    axum::response::Html(include_str!("../static/index.html"))
}
