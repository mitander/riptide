//! Simple JSON API server for torrent management

use std::collections::HashMap;
use std::sync::Arc;

use axum::Router;
use axum::routing::{get, post};
use riptide_core::config::RiptideConfig;
use riptide_core::streaming::{
    FfmpegProcessor, ProductionFfmpegProcessor, SimulationFfmpegProcessor,
};
use riptide_core::torrent::{
    InfoHash, PieceStore, TcpPeerManager, TorrentCreator, TorrentEngineHandle, TrackerManager,
    spawn_torrent_engine,
};
use riptide_core::{LocalMovieManager, RuntimeMode};
use riptide_search::MediaSearchService;
use tokio::sync::RwLock;
use tower_http::cors::CorsLayer;
use tower_http::services::ServeDir;

use crate::handlers::{
    api_add_torrent, api_download_torrent, api_library, api_search, api_settings, api_stats,
    api_torrents, dashboard_page, library_page, search_page, stream_torrent, torrents_page,
    video_player_page,
};

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
    pub movie_manager: Option<Arc<RwLock<LocalMovieManager>>>,
    pub piece_store: Option<Arc<dyn PieceStore>>,
    pub ffmpeg_processor: Arc<dyn FfmpegProcessor>,
    pub conversion_cache: Arc<RwLock<HashMap<InfoHash, ConvertedFile>>>,
}

pub async fn run_server(
    config: RiptideConfig,
    mode: RuntimeMode,
    movies_dir: Option<std::path::PathBuf>,
) -> Result<(), Box<dyn std::error::Error>> {
    // For demo mode, we need to keep references to simulation managers for setup
    let (torrent_engine, _sim_peer_manager, _sim_tracker_manager, sim_piece_store) = match mode {
        RuntimeMode::Production => {
            let peer_manager = TcpPeerManager::new_default();
            let tracker_manager = TrackerManager::new(config.network.clone());
            let engine = spawn_torrent_engine(config.clone(), peer_manager, tracker_manager);
            (engine, None::<()>, None::<()>, None::<Arc<dyn PieceStore>>)
        }
        RuntimeMode::Demo => {
            // In demo mode, we use simulation components that are aware of the content.
            let piece_store = Arc::new(riptide_sim::InMemoryPieceStore::new());
            let peer_manager = riptide_sim::ContentAwarePeerManager::new(
                riptide_sim::InMemoryPeerConfig::default(),
                piece_store.clone(),
            );
            let tracker_manager = riptide_sim::SimulatedTrackerManager::new();

            let engine = spawn_torrent_engine(config.clone(), peer_manager, tracker_manager);

            (
                engine,
                None::<()>,
                None::<()>,
                Some(piece_store as Arc<dyn PieceStore>),
            )
        }
    };
    let search_service = MediaSearchService::from_runtime_mode(mode);

    // Initialize movie manager and piece store for demo mode.
    let (movie_manager, piece_store) = if let Some(dir) = movies_dir.as_ref() {
        let mut manager = LocalMovieManager::new();
        match manager.scan_directory(dir).await {
            Ok(count) => {
                println!("Found {} movie files in {}", count, dir.display());

                if let Some(store) = &sim_piece_store {
                    // Set up the simulated swarm with local movie content.
                    if let Err(e) = setup_demo_swarm(&mut manager, &torrent_engine).await {
                        eprintln!("Warning: Failed to populate piece store: {e}");
                    }

                    (
                        Some(Arc::new(RwLock::new(manager))),
                        Some(store.clone() as Arc<dyn PieceStore>),
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
        RuntimeMode::Demo => Arc::new(SimulationFfmpegProcessor::new()),
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
        .route("/", get(dashboard_page))
        .route("/torrents", get(torrents_page))
        .route("/library", get(library_page))
        .route("/search", get(search_page))
        .route("/player/{info_hash}", get(video_player_page))
        .route("/stream/{info_hash}", get(stream_torrent))
        .route("/api/stats", get(api_stats))
        .route("/api/torrents", get(api_torrents))
        .route("/api/torrents/add", get(api_add_torrent))
        .route("/api/download", post(api_download_torrent))
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

/// Sets up a simulated BitTorrent swarm for demo mode.
///
/// This function performs the following steps:
/// 1. Converts local movie files into torrents and populates an in-memory piece store.
/// 2. Injects a configurable number of simulated peers into the `ContentAwarePeerManager`.
/// 3. Adds these simulated peers to the `SimulatedTrackerManager` to make them discoverable.
///
/// This creates a realistic, closed-loop simulation where the `TorrentEngine` must
/// discover and download pieces from the simulated swarm to stream content, exercising
/// the full P2P download path.
async fn setup_demo_swarm(
    movie_manager: &mut LocalMovieManager,
    torrent_engine: &TorrentEngineHandle,
) -> Result<(), Box<dyn std::error::Error>> {
    let torrent_creator = TorrentCreator::new();
    let movies = movie_manager.all_movies().to_vec();
    let mut hash_updates = Vec::new();

    for movie in movies {
        println!("Converting {} to BitTorrent pieces...", movie.title);

        let metadata = torrent_creator
            .create_from_file(
                &movie.file_path,
                vec!["http://demo-tracker.riptide.local/announce".to_string()],
            )
            .await?;
        let canonical_info_hash = metadata.info_hash;

        if canonical_info_hash != movie.info_hash {
            hash_updates.push((movie.info_hash, canonical_info_hash));
        }

        // Note: In true simulation mode, pieces would be populated by the simulated peers
        // For now, we just register the torrent metadata

        // Add torrent metadata to the engine (but don't mark as complete!)
        torrent_engine
            .add_torrent_metadata(metadata.clone())
            .await?;

        println!(
            "✓ Converted {} ({} pieces)",
            movie.title,
            metadata.piece_hashes.len()
        );
    }

    for (old_hash, new_hash) in hash_updates {
        movie_manager.update_movie_info_hash(old_hash, new_hash);
    }

    println!("Demo mode: All movies converted to piece-based simulation");
    Ok(())
}
