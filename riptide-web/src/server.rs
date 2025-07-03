//! Modern HTMX + Tailwind web server for Riptide
//!
//! Provides both HTMX partial updates and JSON API endpoints.
//! All pages use server-side rendering with real-time updates.

use std::collections::HashMap;
use std::sync::Arc;

use axum::Router;
use axum::routing::{get, post};
use riptide_core::config::RiptideConfig;
use riptide_core::streaming::{
    FfmpegProcessor, ProductionFfmpegProcessor, SimulationFfmpegProcessor,
};
use riptide_core::torrent::{
    InfoHash, PieceStore, TcpPeerManager, TorrentEngineHandle, TrackerManager, spawn_torrent_engine,
};
use riptide_core::{FileLibraryManager, RuntimeMode};
use riptide_search::MediaSearchService;
use tokio::sync::RwLock;
use tower_http::cors::CorsLayer;
use tower_http::services::ServeDir;

use crate::handlers::{
    api_add_torrent, api_download_torrent, api_library, api_search, api_settings, api_stats,
    api_torrents, stream_torrent, video_player_page,
};
use crate::htmx::{dashboard_activity, dashboard_downloads, dashboard_stats};
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

/// Unified app state that works with both production and simulation engines
#[derive(Clone)]
pub struct AppState {
    pub torrent_engine: TorrentEngineHandle,
    pub search_service: MediaSearchService,
    pub movie_manager: Option<Arc<RwLock<FileLibraryManager>>>,
    pub piece_store: Option<Arc<dyn PieceStore>>,
    pub ffmpeg_processor: Arc<dyn FfmpegProcessor>,
    pub conversion_cache: Arc<RwLock<HashMap<InfoHash, ConvertedFile>>>,
}

pub async fn run_server(
    config: RiptideConfig,
    mode: RuntimeMode,
    movies_dir: Option<std::path::PathBuf>,
) -> Result<(), Box<dyn std::error::Error>> {
    // For development mode, we need to keep references to simulation managers for setup
    let (torrent_engine, sim_memory_store, _sim_tracker_manager, sim_piece_store) = match mode {
        RuntimeMode::Production => {
            let peer_manager = TcpPeerManager::new_default();
            let tracker_manager = TrackerManager::new(config.network.clone());
            let engine = spawn_torrent_engine(config.clone(), peer_manager, tracker_manager);
            (
                engine,
                None::<Arc<riptide_sim::InMemoryPieceStore>>,
                None::<()>,
                None::<Arc<dyn PieceStore>>,
            )
        }
        RuntimeMode::Development => {
            // In development mode, we use simulation components that are aware of the content.
            let piece_store = Arc::new(riptide_sim::InMemoryPieceStore::new());
            let peer_manager = riptide_sim::ContentAwarePeerManager::new(
                riptide_sim::InMemoryPeerConfig::default(),
                piece_store.clone(),
            );
            let tracker_manager = riptide_sim::SimulatedTrackerManager::new();

            let engine = spawn_torrent_engine(config.clone(), peer_manager, tracker_manager);

            (
                engine,
                Some(piece_store.clone()),
                None::<()>,
                Some(piece_store as Arc<dyn PieceStore>),
            )
        }
    };
    let search_service = MediaSearchService::from_runtime_mode(mode);

    // Initialize movie manager and piece store for development mode.
    let (movie_manager, piece_store) = if let Some(dir) = movies_dir.as_ref() {
        let mut manager = FileLibraryManager::new();
        match manager.scan_directory(dir).await {
            Ok(count) => {
                println!("Found {} movie files in {}", count, dir.display());

                if let Some(memory_store) = &sim_memory_store {
                    // Set up the simulated swarm with local movie content.
                    if let Err(e) =
                        setup_development_swarm(&mut manager, &torrent_engine, memory_store.clone())
                            .await
                    {
                        eprintln!("Warning: Failed to populate piece store: {e}");
                    }

                    (
                        Some(Arc::new(RwLock::new(manager))),
                        sim_piece_store.clone(),
                    )
                } else {
                    // Production mode or no simulation managers
                    (Some(Arc::new(RwLock::new(manager))), None)
                }
            }
            Err(e) => {
                eprintln!("Warning: Failed to scan movies directory: {e}");
                (None, None)
            }
        }
    } else {
        (None, None)
    };

    // Initialize FFmpeg processor based on runtime mode.
    let ffmpeg_processor: Arc<dyn FfmpegProcessor> = match mode {
        RuntimeMode::Production => Arc::new(ProductionFfmpegProcessor::new(None)),
        RuntimeMode::Development => Arc::new(SimulationFfmpegProcessor::new()),
    };

    // Create conversion cache.
    let conversion_cache = Arc::new(RwLock::new(HashMap::new()));

    let state = AppState {
        torrent_engine,
        search_service,
        movie_manager,
        piece_store,
        ffmpeg_processor,
        conversion_cache,
    };

    let app = Router::new()
        // Main pages (HTMX + Tailwind)
        .route("/", get(dashboard_page))
        .route("/torrents", get(torrents_page))
        .route("/library", get(library_page))
        .route("/search", get(search_page))
        .route("/player/{info_hash}", get(video_player_page))
        // HTMX partial update endpoints
        .route("/htmx/dashboard/stats", get(dashboard_stats))
        .route("/htmx/dashboard/activity", get(dashboard_activity))
        .route("/htmx/dashboard/downloads", get(dashboard_downloads))
        .route("/htmx/torrents/list", get(crate::htmx::torrent_list))
        .route("/htmx/torrents/add", post(crate::htmx::add_torrent))
        .route("/htmx/system/status", get(crate::htmx::system_status))
        // Streaming endpoints
        .route("/stream/{info_hash}", get(stream_torrent))
        // JSON API endpoints (for external clients)
        .route("/api/stats", get(api_stats))
        .route("/api/torrents", get(api_torrents))
        .route("/api/torrents/add", get(api_add_torrent))
        .route("/api/download", post(api_download_torrent))
        .route("/api/library", get(api_library))
        .route("/api/search", get(api_search))
        .route("/api/settings", get(api_settings))
        // Static assets (minimal)
        .nest_service("/static", ServeDir::new("riptide-web/static"))
        .layer(CorsLayer::permissive())
        .with_state(state);

    println!("Riptide media server running on http://127.0.0.1:3000");
    let listener = tokio::net::TcpListener::bind("127.0.0.1:3000").await?;
    axum::serve(listener, app).await?;
    Ok(())
}

/// Sets up a simulated BitTorrent swarm for development mode.
///
/// This function performs the following steps:
/// 1. Converts local movie files into torrents and populates an in-memory piece store.
/// 2. Injects a configurable number of simulated peers into the `ContentAwarePeerManager`.
/// 3. Adds these simulated peers to the `SimulatedTrackerManager` to make them discoverable.
///
/// This creates a realistic, closed-loop simulation where the `TorrentEngine` must
/// discover and download pieces from the simulated swarm to stream content, exercising
/// the full P2P download path.
async fn setup_development_swarm(
    library_manager: &mut FileLibraryManager,
    torrent_engine: &TorrentEngineHandle,
    piece_store: Arc<riptide_sim::InMemoryPieceStore>,
) -> Result<(), Box<dyn std::error::Error>> {
    let movies = library_manager.all_files().to_vec();
    let movie_count = movies.len();

    println!("Starting background conversion of {movie_count} movies to BitTorrent pieces...");

    // Spawn background tasks for each movie conversion to avoid blocking server startup
    for movie in movies {
        let movie_title = movie.title.clone();
        let movie_path = movie.file_path.clone();
        let _old_info_hash = movie.info_hash;
        let torrent_engine = torrent_engine.clone();
        let piece_store = piece_store.clone();

        tokio::spawn(async move {
            println!("Converting {movie_title} to BitTorrent pieces...");

            // Create SimulationTorrentCreator per task since it needs to be mutable
            let mut sim_creator = riptide_core::torrent::SimulationTorrentCreator::new();

            match sim_creator
                .create_with_pieces(
                    &movie_path,
                    vec!["http://development-tracker.riptide.local/announce".to_string()],
                )
                .await
            {
                Ok((metadata, pieces)) => {
                    let canonical_info_hash = metadata.info_hash;
                    let piece_count = metadata.piece_hashes.len();

                    // Add torrent metadata to the engine
                    if let Err(e) = torrent_engine.add_torrent_metadata(metadata.clone()).await {
                        tracing::error!(
                            "Failed to add torrent metadata for {}: {}",
                            movie_title,
                            e
                        );
                        return;
                    }

                    // Populate the piece store with actual torrent pieces
                    if let Err(e) = piece_store
                        .add_torrent_pieces(canonical_info_hash, pieces)
                        .await
                    {
                        tracing::error!(
                            "Failed to populate piece store for {}: {}",
                            movie_title,
                            e
                        );
                        return;
                    }

                    // Start the download to initialize the session
                    if let Err(e) = torrent_engine.start_download(canonical_info_hash).await {
                        tracing::warn!("Failed to start download for {}: {}", movie_title, e);
                    }

                    // Let the simulation naturally download pieces from ContentAwarePeerManager
                    // This allows realistic download progress and streaming while downloading

                    println!("✓ Converted {movie_title} ({piece_count} pieces)");
                }
                Err(e) => {
                    tracing::error!("Failed to convert {} to torrent: {}", movie_title, e);
                }
            }
        });
    }

    // Update movie manager with canonical info hashes after spawning tasks
    // Note: This is a limitation of the current approach - we can't easily update
    // the movie manager from background tasks. In a future version, we should
    // use channels to communicate back the canonical info hashes.

    println!("Development mode: {movie_count} movies queued for background conversion");
    Ok(())
}
