//! Simple JSON API server for torrent management

use std::sync::Arc;

use axum::Router;
use axum::routing::{get, post};
use riptide_core::config::RiptideConfig;
use riptide_core::engine::{ProductionTorrentEngine, TorrentEngineOps};
use riptide_core::{LocalMovieManager, RuntimeMode};
use riptide_search::MediaSearchService;
use tokio::sync::RwLock;
use tower_http::cors::CorsLayer;
use tower_http::services::ServeDir;

use crate::handlers::{
    api_add_local_movie, api_add_torrent, api_download_torrent, api_library, api_local_movies,
    api_search, api_settings, api_stats, api_torrents, dashboard_page, library_page, search_page,
    stream_local_movie, stream_torrent, torrents_page, video_player_page,
};

/// Unified app state that works with both production and simulation engines
#[derive(Clone)]
pub struct AppState {
    pub torrent_engine: Arc<RwLock<dyn TorrentEngineOps>>,
    pub search_service: MediaSearchService,
    pub movie_manager: Option<Arc<RwLock<LocalMovieManager>>>,
}

pub async fn run_server(
    config: RiptideConfig,
    mode: RuntimeMode,
    movies_dir: Option<std::path::PathBuf>,
) -> Result<(), Box<dyn std::error::Error>> {
    // Create appropriate engine based on runtime mode
    let torrent_engine: Arc<RwLock<dyn TorrentEngineOps>> = match mode {
        RuntimeMode::Production => {
            let engine = ProductionTorrentEngine::new_production(config.clone());
            Arc::new(RwLock::new(engine))
        }
        RuntimeMode::Demo => {
            // TODO: Create simulation engine using riptide-sim components
            // For now, use production engine in demo mode
            let engine = ProductionTorrentEngine::new_production(config.clone());
            Arc::new(RwLock::new(engine))
        }
    };
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
        .route("/api/download", post(api_download_torrent))
        .route("/api/movies/local", get(api_local_movies))
        .route("/api/movies/add", get(api_add_local_movie))
        .route("/api/library", get(api_library))
        .route("/api/search", get(api_search))
        .route("/api/settings", get(api_settings))
        .nest_service("/static", ServeDir::new("riptide-web/static"))
        .layer(CorsLayer::permissive())
        .with_state(state);

    println!("Riptide media server running on http://127.0.0.1:3000");
    let listener = tokio::net::TcpListener::bind("127.0.0.1:3000").await?;
    axum::serve(listener, app).await?;
    Ok(())
}
