//! Modern HTMX + Tailwind web server for Riptide
//!
//! Provides both HTMX partial updates and JSON API endpoints.
//! All pages use server-side rendering with real-time updates.

use std::collections::HashMap;
use std::sync::Arc;

use axum::Router;
use axum::extract::State;
use riptide_core::FileLibraryManager;
use riptide_core::config::RiptideConfig;
use riptide_core::server_components::{ConversionProgress, ServerComponents};
use riptide_core::streaming::FfmpegProcessor;
use riptide_core::torrent::{InfoHash, PieceStore, TorrentEngineHandle};
use riptide_search::MediaSearchService;
use tokio::sync::RwLock;
use tower_http::cors::CorsLayer;
use tower_http::services::ServeDir;

use crate::handlers::{
    api_add_torrent, api_download_torrent, api_library, api_search, api_search_movies,
    api_seek_torrent, api_settings, api_stats, api_torrents, stream_torrent, video_player_page,
};
use crate::htmx::{
    add_torrent, dashboard_activity, dashboard_downloads, dashboard_stats, system_status,
    torrent_list,
};
// Import new architecture modules
use crate::pages::{dashboard_page, library_page, search_page, torrents_page};

// Removed PieceStoreType enum - using trait objects instead

/// Cache entry for converted files
#[derive(Debug, Clone)]
pub struct ConvertedFile {
    pub output_path: std::path::PathBuf,
    pub size: u64,
    pub created_at: std::time::Instant,
}

/// Unified app state that works with both production and simulation engines.
/// All services are provided by CLI dependency injection - web layer is mode-agnostic.
#[derive(Clone)]
pub struct AppState {
    pub services: Arc<ServerComponents>,
    pub search_service: MediaSearchService,
    pub conversion_cache: Arc<RwLock<HashMap<InfoHash, ConvertedFile>>>,
    pub server_started_at: std::time::Instant,
}

impl AppState {
    /// Get the torrent engine handle.
    pub fn engine(&self) -> &TorrentEngineHandle {
        self.services.engine()
    }

    /// Get the file manager if available (Development mode only).
    ///
    /// # Errors
    /// Returns error if file manager is not available in this mode.
    pub fn file_manager(
        &self,
    ) -> Result<&Arc<RwLock<FileLibraryManager>>, riptide_core::server_components::ServiceError>
    {
        self.services.file_manager()
    }

    /// Get the piece store if available (Development mode only).
    ///
    /// # Errors
    /// Returns error if piece store is not available in this mode.
    pub fn piece_store(
        &self,
    ) -> Result<&Arc<dyn PieceStore>, riptide_core::server_components::ServiceError> {
        self.services.piece_store()
    }

    /// Get conversion progress tracker if available (Development mode only).
    ///
    /// # Errors
    /// Returns error if conversion tracking is not available in this mode.
    pub fn conversion_progress(
        &self,
    ) -> Result<
        &Arc<RwLock<HashMap<String, ConversionProgress>>>,
        riptide_core::server_components::ServiceError,
    > {
        self.services.conversion_progress()
    }

    /// Get the FFmpeg processor for transcoding operations.
    pub fn ffmpeg_processor(&self) -> &Arc<dyn FfmpegProcessor> {
        self.services.ffmpeg_processor()
    }
}

/// Starts the Riptide web server with the provided configuration and components.
///
/// # Errors
/// Returns error if server fails to bind to address or start successfully.
pub async fn run_server(
    _config: RiptideConfig,
    components: ServerComponents,
    search_service: MediaSearchService,
) -> Result<(), Box<dyn std::error::Error>> {
    let conversion_cache = Arc::new(RwLock::new(HashMap::new()));

    let state = AppState {
        services: Arc::new(components),
        search_service,
        conversion_cache,
        server_started_at: std::time::Instant::now(),
    };

    let app = Router::new()
        // Main pages (HTMX + Tailwind)
        .route("/", axum::routing::get(dashboard_page))
        .route("/torrents", axum::routing::get(torrents_page))
        .route("/library", axum::routing::get(library_page))
        .route("/search", axum::routing::get(search_page))
        .route("/player/{info_hash}", axum::routing::get(video_player_page))
        // HTMX partial update endpoints
        .route("/htmx/dashboard/stats", axum::routing::get(dashboard_stats))
        .route(
            "/htmx/dashboard/activity",
            axum::routing::get(dashboard_activity),
        )
        .route(
            "/htmx/dashboard/downloads",
            axum::routing::get(dashboard_downloads),
        )
        .route("/htmx/torrents/list", axum::routing::get(torrent_list))
        .route("/htmx/torrents/add", axum::routing::post(add_torrent))
        .route("/htmx/system/status", axum::routing::get(system_status))
        // Streaming endpoints
        .route("/stream/{info_hash}", axum::routing::get(stream_torrent))
        // JSON API endpoints (for external clients)
        .route("/api/stats", axum::routing::get(api_stats))
        .route("/api/torrents", axum::routing::get(api_torrents))
        .route("/api/torrents/add", axum::routing::get(api_add_torrent))
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
        .nest_service("/static", ServeDir::new("riptide-web/static"))
        .layer(CorsLayer::permissive())
        .with_state(state);

    println!("Riptide media server running on http://127.0.0.1:3000");
    let listener = tokio::net::TcpListener::bind("127.0.0.1:3000").await?;
    axum::serve(listener, app).await?;
    Ok(())
}

/// API endpoint to get current conversion progress
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
