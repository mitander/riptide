//! Modern HTMX + Tailwind web server for Riptide
//!
//! Provides both HTMX partial updates and JSON API endpoints.
//! All pages use server-side rendering with real-time updates.

use std::collections::HashMap;
use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4};
use std::sync::{Arc, Mutex};

use axum::Router;
use axum::routing::{get, post};
use rand::{Rng, rng};
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
    api_add_torrent, api_download_torrent, api_library, api_search, api_seek_torrent, api_settings,
    api_stats, api_torrents, stream_torrent, video_player_page,
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
    pub server_started_at: std::time::Instant,
}

pub async fn run_server(
    mut config: RiptideConfig,
    mode: RuntimeMode,
    movies_dir: Option<std::path::PathBuf>,
) -> Result<(), Box<dyn std::error::Error>> {
    // Set the runtime mode in config so the engine can access it
    config.runtime_mode = mode;

    // Create engine with appropriate managers based on runtime mode
    type ServerComponents = (
        TorrentEngineHandle,
        Option<Arc<RwLock<FileLibraryManager>>>,
        Option<Arc<dyn PieceStore>>,
    );
    let (torrent_engine, movie_manager, piece_store): ServerComponents = match mode {
        RuntimeMode::Production => {
            // Production mode uses real network components
            let peer_manager = TcpPeerManager::new_default();
            let tracker_manager = TrackerManager::new(config.network.clone());
            let engine = spawn_torrent_engine(config.clone(), peer_manager, tracker_manager);
            (engine, None, None)
        }
        RuntimeMode::Development => {
            // Development mode uses coordinated simulation components
            let piece_store_sim = Arc::new(riptide_sim::InMemoryPieceStore::new());

            // Create a shared registry of simulated peer addresses
            let peer_registry = Arc::new(Mutex::new(HashMap::<InfoHash, Vec<SocketAddr>>::new()));
            let peer_registry_clone = peer_registry.clone();

            // Create content-aware peer manager using simulation environment settings
            // (Using streaming environment's network characteristics: 10-100ms latency, 0.1% packet loss)
            let realistic_peer_config = riptide_sim::InMemoryPeerConfig {
                message_delay_ms: 50,          // 10-100ms range from streaming environment
                connection_failure_rate: 0.05, // 5% failure rate (realistic)
                message_loss_rate: 0.001,      // 0.1% from streaming environment
                max_connections: 100,          // Support for larger peer swarms
                auto_keepalive: true,
            };

            let peer_manager_sim = riptide_sim::ContentAwarePeerManager::new(
                realistic_peer_config,
                piece_store_sim.clone(),
            );

            // Create tracker that coordinates with the peer registry
            let tracker_manager_sim =
                riptide_sim::tracker::SimulatedTrackerManager::with_peer_coordinator(
                    riptide_sim::tracker::ResponseConfig::default(),
                    move |info_hash| {
                        let registry = peer_registry_clone.lock().unwrap();
                        registry.get(info_hash).cloned().unwrap_or_default()
                    },
                );

            let engine =
                spawn_torrent_engine(config.clone(), peer_manager_sim, tracker_manager_sim);

            // Populate the piece store and register simulated peers
            let mut manager_opt = None;
            if let Some(dir) = movies_dir.as_ref() {
                let mut manager = FileLibraryManager::new();
                if let Err(e) = setup_development_environment(
                    &mut manager,
                    &piece_store_sim,
                    &peer_registry,
                    &engine,
                    dir,
                )
                .await
                {
                    eprintln!("Warning: Failed to setup development environment: {e}");
                }
                manager_opt = Some(Arc::new(RwLock::new(manager)));
            }

            let piece_store_for_state = Some(piece_store_sim.clone() as Arc<dyn PieceStore>);

            (engine, manager_opt, piece_store_for_state)
        }
    };
    let search_service = MediaSearchService::from_runtime_mode(mode);

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
        server_started_at: std::time::Instant::now(),
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
        .route("/api/torrents/{info_hash}/seek", post(api_seek_torrent))
        // Static assets (minimal)
        .nest_service("/static", ServeDir::new("riptide-web/static"))
        .layer(CorsLayer::permissive())
        .with_state(state);

    println!("Riptide media server running on http://127.0.0.1:3000");
    let listener = tokio::net::TcpListener::bind("127.0.0.1:3000").await?;
    axum::serve(listener, app).await?;
    Ok(())
}

/// Sets up the development environment with simulated peers and torrents.
///
/// This function:
/// 1. Converts local movie files into torrent pieces
/// 2. Stores pieces in the InMemoryPieceStore
/// 3. Creates simulated peer addresses and registers them with the tracker
/// 4. Starts downloads so the engine can connect to simulated peers
async fn setup_development_environment(
    library_manager: &mut FileLibraryManager,
    piece_store: &Arc<riptide_sim::InMemoryPieceStore>,
    peer_registry: &Arc<Mutex<HashMap<InfoHash, Vec<SocketAddr>>>>,
    torrent_engine: &TorrentEngineHandle,
    movies_dir: &std::path::Path,
) -> Result<(), Box<dyn std::error::Error>> {
    match library_manager.scan_directory(movies_dir).await {
        Ok(count) => {
            println!("Found {} movie files in {}", count, movies_dir.display());
        }
        Err(e) => {
            return Err(format!("Failed to scan movies directory: {e}").into());
        }
    }
    let movies = library_manager.all_files().to_vec();
    let mut info_hash_updates = Vec::new();

    println!(
        "Setting up development environment for {} movies...",
        movies.len()
    );

    for (index, movie) in movies.iter().enumerate() {
        println!("Converting {} to BitTorrent pieces...", movie.title);

        // Create SimulationTorrentCreator
        let mut sim_creator = riptide_core::torrent::SimulationTorrentCreator::new();

        match sim_creator
            .create_with_pieces(
                &movie.file_path,
                vec!["http://development-tracker.riptide.local/announce".to_string()],
            )
            .await
        {
            Ok((metadata, pieces)) => {
                let canonical_info_hash = metadata.info_hash;

                // Add torrent metadata to the engine
                torrent_engine
                    .add_torrent_metadata(metadata.clone())
                    .await?;

                // Populate the piece store with actual torrent pieces
                piece_store
                    .add_torrent_pieces(canonical_info_hash, pieces)
                    .await?;

                // Create realistic multi-peer simulation (40-50 peers with varying characteristics)
                let mut rng = rng();
                let peer_count = 40 + rng.random_range(0..11); // 40-50 peers
                let mut peer_addrs = Vec::new();

                for peer_num in 0..peer_count {
                    // Distribute peers across different subnets for realism
                    let subnet_base = 192;
                    let subnet_middle = 168 + ((peer_num / 50) as u8); // Different /16 subnets
                    let subnet_third = (index as u8).wrapping_add((peer_num / 10) as u8);
                    let host = 1 + (peer_num % 10) as u8;

                    let addr = SocketAddr::V4(SocketAddrV4::new(
                        Ipv4Addr::new(subnet_base, subnet_middle, subnet_third, host),
                        6881 + (peer_num as u16),
                    ));
                    peer_addrs.push(addr);
                }

                // Register these peer addresses with the tracker
                {
                    let mut registry = peer_registry.lock().unwrap();
                    registry.insert(canonical_info_hash, peer_addrs.clone());
                }

                // Start the download to initiate the simulation
                torrent_engine.start_download(canonical_info_hash).await?;

                // Store the info hash update for later
                info_hash_updates.push((movie.info_hash, canonical_info_hash));

                println!(
                    "✓ Converted {} ({} pieces, {} simulated peers)",
                    movie.title,
                    metadata.piece_hashes.len(),
                    peer_addrs.len()
                );
            }
            Err(e) => {
                return Err(format!("Failed to convert {} to torrent: {}", movie.title, e).into());
            }
        }
    }

    // Apply all info hash updates after the loop
    for (old_hash, new_hash) in info_hash_updates {
        library_manager.update_file_info_hash(old_hash, new_hash);
    }

    println!("Development environment ready!");
    Ok(())
}
