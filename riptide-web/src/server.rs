//! Simple JSON API server for torrent management

use std::sync::Arc;

use axum::Router;
use axum::routing::{get, post};
use riptide_core::config::RiptideConfig;
use riptide_core::torrent::{
    TcpPeerManager, TorrentCreator, TorrentEngineHandle, TrackerManager, spawn_torrent_engine,
};
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

/// Concrete piece store types for different runtime modes
#[derive(Clone)]
pub enum PieceStoreType {
    Simulation(Arc<riptide_sim::InMemoryPieceStore>),
    // TODO: Add production piece store variant
}

/// Unified app state that works with both production and simulation engines
#[derive(Clone)]
pub struct AppState {
    pub torrent_engine: TorrentEngineHandle,
    pub search_service: MediaSearchService,
    pub movie_manager: Option<Arc<RwLock<LocalMovieManager>>>,
    pub piece_store: Option<PieceStoreType>,
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
    let (movie_manager, piece_store) = if let Some(dir) = movies_dir.as_ref().filter(|_| mode.is_demo()) {
        let mut manager = LocalMovieManager::new();
        let piece_store = Arc::new(riptide_sim::InMemoryPieceStore::new());

        match manager.scan_directory(dir).await {
            Ok(count) => {
                println!("Found {} movie files in {}", count, dir.display());
                
                // Convert local movies to BitTorrent pieces for simulation
                if let Err(e) = populate_piece_store_from_movies(&manager, &piece_store, &torrent_engine).await {
                    eprintln!("Warning: Failed to populate piece store: {e}");
                }
                
                (
                    Some(Arc::new(RwLock::new(manager))),
                    Some(PieceStoreType::Simulation(piece_store))
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

    let state = AppState {
        torrent_engine,
        search_service,
        movie_manager,
        piece_store,
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

/// Convert local movies to BitTorrent pieces and populate the piece store
///
/// This is the critical integration that enables piece-based simulation in demo mode.
/// Each local movie file is split into BitTorrent pieces and added to the simulation
/// piece store, allowing the streaming system to demonstrate real BitTorrent behavior.
async fn populate_piece_store_from_movies(
    movie_manager: &LocalMovieManager,
    piece_store: &Arc<riptide_sim::InMemoryPieceStore>,
    torrent_engine: &TorrentEngineHandle,
) -> Result<(), Box<dyn std::error::Error>> {
    use riptide_core::torrent::TorrentPiece;
    
    let torrent_creator = TorrentCreator::new();
    
    for movie in movie_manager.all_movies() {
        println!("Converting {} to BitTorrent pieces...", movie.title);
        
        // Create torrent metadata from the local file
        let metadata = torrent_creator.create_from_file(
            &movie.file_path,
            vec!["http://demo-tracker.riptide.local/announce".to_string()]
        ).await?;
        
        // Verify info hash matches (should be deterministic)
        if metadata.info_hash != movie.info_hash {
            eprintln!("Warning: Info hash mismatch for {}", movie.title);
            continue;
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
        
        // Add pieces to simulation store
        piece_store.add_torrent_pieces(movie.info_hash, pieces).await?;
        
        // Create torrent session so streaming system has metadata
        torrent_engine.add_magnet(&format!(
            "magnet:?xt=urn:btih:{}&dn={}",
            hex::encode(movie.info_hash.as_bytes()),
            urlencoding::encode(&movie.title)
        )).await?;
        
        println!("✓ Converted {} ({} pieces)", movie.title, metadata.piece_hashes.len());
    }
    
    println!("Demo mode: All movies converted to piece-based simulation");
    Ok(())
}
