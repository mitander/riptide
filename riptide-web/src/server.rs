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
    InfoHash, TcpPeerManager, TorrentCreator, TorrentEngineHandle, TrackerManager,
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

/// Concrete piece store types for different runtime modes
#[derive(Clone)]
pub enum PieceStoreType {
    Simulation(Arc<riptide_sim::InMemoryPieceStore>),
    // TODO: Add production piece store variant
}

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
    pub piece_store: Option<PieceStoreType>,
    pub ffmpeg_processor: Arc<dyn FfmpegProcessor>,
    pub conversion_cache: Arc<RwLock<HashMap<InfoHash, ConvertedFile>>>,
}

pub async fn run_server(
    config: RiptideConfig,
    mode: RuntimeMode,
    movies_dir: Option<std::path::PathBuf>,
) -> Result<(), Box<dyn std::error::Error>> {
    // Create appropriate engine based on runtime mode
    let torrent_engine: TorrentEngineHandle = match mode {
        RuntimeMode::Production => {
            let peer_manager = TcpPeerManager::new_default();
            let tracker_manager = TrackerManager::new(config.network.clone());
            spawn_torrent_engine(config.clone(), peer_manager, tracker_manager)
        }
        RuntimeMode::Demo => {
            // TODO: Create simulation engine using riptide-sim components
            // For now, use production engine in demo mode
            let peer_manager = TcpPeerManager::new_default();
            let tracker_manager = TrackerManager::new(config.network.clone());
            spawn_torrent_engine(config.clone(), peer_manager, tracker_manager)
        }
    };
    let search_service = MediaSearchService::from_runtime_mode(mode);

    // Initialize movie manager and piece store for demo mode
    let (movie_manager, piece_store) = if let Some(dir) =
        movies_dir.as_ref().filter(|_| mode.is_demo())
    {
        let mut manager = LocalMovieManager::new();
        let piece_store = Arc::new(riptide_sim::InMemoryPieceStore::new());

        match manager.scan_directory(dir).await {
            Ok(count) => {
                println!("Found {} movie files in {}", count, dir.display());

                // Convert local movies to BitTorrent pieces for simulation
                if let Err(e) =
                    populate_piece_store_from_movies(&mut manager, &piece_store, &torrent_engine)
                        .await
                {
                    eprintln!("Warning: Failed to populate piece store: {e}");
                }

                (
                    Some(Arc::new(RwLock::new(manager))),
                    Some(PieceStoreType::Simulation(piece_store)),
                )
            }
            Err(e) => {
                eprintln!("Warning: Failed to scan movies directory: {e}");
                (None, None)
            }
        }
    } else {
        (None, None)
    };

    // Initialize FFmpeg processor based on runtime mode
    let ffmpeg_processor: Arc<dyn FfmpegProcessor> = match mode {
        RuntimeMode::Production => Arc::new(ProductionFfmpegProcessor::new(None)),
        RuntimeMode::Demo => Arc::new(SimulationFfmpegProcessor::new()),
    };

    // Create conversion cache
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

/// Convert local movies to BitTorrent pieces and populate the piece store
///
/// This is the critical integration that enables piece-based simulation in demo mode.
/// Each local movie file is split into BitTorrent pieces and added to the simulation
/// piece store, allowing the streaming system to demonstrate real BitTorrent behavior.
async fn populate_piece_store_from_movies(
    movie_manager: &mut LocalMovieManager,
    piece_store: &Arc<riptide_sim::InMemoryPieceStore>,
    torrent_engine: &TorrentEngineHandle,
) -> Result<(), Box<dyn std::error::Error>> {
    use riptide_core::torrent::TorrentPiece;

    let torrent_creator = TorrentCreator::new();

    // Collect movies first to avoid borrowing conflicts
    let movies = movie_manager.all_movies().to_vec();
    let mut hash_updates = Vec::new();

    for movie in movies {
        println!("Converting {} to BitTorrent pieces...", movie.title);

        // Create torrent metadata from the local file
        let metadata = torrent_creator
            .create_from_file(
                &movie.file_path,
                vec!["http://demo-tracker.riptide.local/announce".to_string()],
            )
            .await?;

        // Use TorrentCreator's info hash as canonical (content-based, not path-based)
        let canonical_info_hash = metadata.info_hash;
        if canonical_info_hash != movie.info_hash {
            println!(
                "Using content-based info hash for {}: {}",
                movie.title,
                hex::encode(canonical_info_hash.as_bytes())
            );
            // Store hash update for later
            hash_updates.push((movie.info_hash, canonical_info_hash));
        }

        // Read file and split into pieces
        let file_data = tokio::fs::read(&movie.file_path).await?;
        let piece_size = metadata.piece_length as usize;
        let mut pieces = Vec::new();

        for (index, chunk) in file_data.chunks(piece_size).enumerate() {
            pieces.push(TorrentPiece {
                index: index as u32,
                hash: metadata.piece_hashes[index],
                data: chunk.to_vec(),
            });
        }

        // Add pieces to simulation store using canonical info hash
        piece_store
            .add_torrent_pieces(canonical_info_hash, pieces.clone())
            .await?;

        // Create torrent session with proper metadata (not just magnet link)
        torrent_engine
            .add_torrent_metadata(metadata.clone())
            .await?;

        // Mark all pieces as completed since they're pre-populated in demo mode
        let piece_indices: Vec<u32> = (0..pieces.len() as u32).collect();
        torrent_engine
            .mark_pieces_completed(canonical_info_hash, piece_indices)
            .await?;

        println!(
            "✓ Converted {} ({} pieces)",
            movie.title,
            metadata.piece_hashes.len()
        );
    }

    // Apply hash updates after processing all movies
    for (old_hash, new_hash) in hash_updates {
        movie_manager.update_movie_info_hash(old_hash, new_hash);
    }

    println!("Demo mode: All movies converted to piece-based simulation");
    Ok(())
}
