//! Modern HTMX + Tailwind web server for Riptide
//!
//! Provides both HTMX partial updates and JSON API endpoints.
//! All pages use server-side rendering with real-time updates.

use std::collections::HashMap;
use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4};
use std::sync::{Arc, Mutex};

use axum::Router;
use axum::extract::State;
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

/// Progress tracking for background movie conversions
#[derive(Debug, Clone, serde::Serialize)]
pub struct ConversionProgress {
    pub movie_title: String,
    pub status: ConversionStatus,
    #[serde(serialize_with = "serialize_instant")]
    pub started_at: std::time::Instant,
    #[serde(serialize_with = "serialize_optional_instant")]
    pub completed_at: Option<std::time::Instant>,
    pub error_message: Option<String>,
}

/// Status of background movie conversion to torrent format.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub enum ConversionStatus {
    Pending,
    Converting,
    Completed,
    Failed,
}

fn serialize_instant<S>(instant: &std::time::Instant, serializer: S) -> Result<S::Ok, S::Error>
where
    S: serde::Serializer,
{
    let elapsed = instant.elapsed().as_secs();
    serializer.serialize_u64(elapsed)
}

fn serialize_optional_instant<S>(
    instant: &Option<std::time::Instant>,
    serializer: S,
) -> Result<S::Ok, S::Error>
where
    S: serde::Serializer,
{
    match instant {
        Some(i) => {
            let elapsed = i.elapsed().as_secs();
            serializer.serialize_some(&elapsed)
        }
        None => serializer.serialize_none(),
    }
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
    pub conversion_progress: Arc<RwLock<HashMap<String, ConversionProgress>>>,
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
        Option<Arc<RwLock<HashMap<String, ConversionProgress>>>>,
    );
    let (torrent_engine, movie_manager, piece_store, conversion_progress): ServerComponents =
        match mode {
            RuntimeMode::Production => {
                // Production mode uses real network components
                let peer_manager = TcpPeerManager::new_default();
                let tracker_manager = TrackerManager::new(config.network.clone());
                let engine = spawn_torrent_engine(config.clone(), peer_manager, tracker_manager);
                (engine, None, None, None)
            }
            RuntimeMode::Development => {
                // Development mode uses coordinated simulation components
                let piece_store_sim = Arc::new(riptide_sim::InMemoryPieceStore::new());

                // Create a shared registry of simulated peer addresses
                let peer_registry =
                    Arc::new(Mutex::new(HashMap::<InfoHash, Vec<SocketAddr>>::new()));
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

                let conversion_progress = Arc::new(RwLock::new(HashMap::new()));

                // Initialize file library manager
                let manager_opt = if let Some(dir) = movies_dir.as_ref() {
                    let mut manager = FileLibraryManager::new();

                    // Quick scan to initialize the manager
                    match manager.scan_directory(dir).await {
                        Ok(count) => {
                            println!("Found {} movie files in {}", count, dir.display());

                            let movies: Vec<_> = manager.all_files().into_iter().cloned().collect();

                            {
                                let mut progress = conversion_progress.write().await;
                                for movie in &movies {
                                    progress.insert(
                                        movie.title.clone(),
                                        ConversionProgress {
                                            movie_title: movie.title.clone(),
                                            status: ConversionStatus::Pending,
                                            started_at: std::time::Instant::now(),
                                            completed_at: None,
                                            error_message: None,
                                        },
                                    );
                                }
                            }

                            if !movies.is_empty() {
                                println!("Starting background conversion service...");
                                let piece_store_bg = piece_store_sim.clone();
                                let peer_registry_bg = peer_registry.clone();
                                let engine_bg = engine.clone();
                                let progress_tracker = conversion_progress.clone();

                                tokio::spawn(async move {
                                    start_background_conversions(
                                        movies,
                                        piece_store_bg,
                                        peer_registry_bg,
                                        engine_bg,
                                        progress_tracker,
                                    )
                                    .await;
                                });
                            } else {
                                println!("No movies found to convert.");
                            }

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

                let piece_store_for_state = Some(piece_store_sim.clone() as Arc<dyn PieceStore>);

                (
                    engine,
                    manager_opt,
                    piece_store_for_state,
                    Some(conversion_progress),
                )
            }
        };
    let search_service = MediaSearchService::from_runtime_mode(mode);

    // Initialize FFmpeg processor based on runtime mode.
    let ffmpeg_processor: Arc<dyn FfmpegProcessor> = match mode {
        RuntimeMode::Production => Arc::new(ProductionFfmpegProcessor::new(None)),
        RuntimeMode::Development => Arc::new(SimulationFfmpegProcessor::new()),
    };

    let conversion_cache = Arc::new(RwLock::new(HashMap::new()));
    let conversion_progress =
        conversion_progress.unwrap_or_else(|| Arc::new(RwLock::new(HashMap::new())));

    let state = AppState {
        torrent_engine,
        search_service,
        movie_manager,
        piece_store,
        ffmpeg_processor,
        conversion_cache,
        conversion_progress,
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
        .route("/api/conversions/progress", get(api_conversion_progress))
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
    let progress = state.conversion_progress.read().await;
    axum::Json(progress.clone())
}

/// Generate mock peer addresses for simulation
fn generate_mock_peer_addresses(min_count: u32, max_count: u32) -> Vec<SocketAddr> {
    let peer_count = min_count + (rand::random::<u32>() % (max_count - min_count + 1));
    let mut addresses = Vec::new();

    for i in 0..peer_count {
        let addr = SocketAddr::V4(SocketAddrV4::new(
            Ipv4Addr::new(192, 168, (i / 256) as u8, (i % 256) as u8),
            6881 + (i as u16 % 1000),
        ));
        addresses.push(addr);
    }

    addresses
}

/// Background conversion service that processes all movies without blocking server startup
async fn start_background_conversions(
    movies: Vec<riptide_core::storage::LibraryFile>,
    piece_store: Arc<riptide_sim::InMemoryPieceStore>,
    peer_registry: Arc<Mutex<HashMap<InfoHash, Vec<SocketAddr>>>>,
    torrent_engine: TorrentEngineHandle,
    progress_tracker: Arc<RwLock<HashMap<String, ConversionProgress>>>,
) {
    println!(
        "Converting {} movies to torrents in background...",
        movies.len()
    );

    const MAX_CONCURRENT_CONVERSIONS: usize = 3;
    let semaphore = Arc::new(tokio::sync::Semaphore::new(MAX_CONCURRENT_CONVERSIONS));

    let mut tasks = Vec::new();
    for movie in movies {
        let piece_store_task = piece_store.clone();
        let peer_registry_task = peer_registry.clone();
        let engine_task = torrent_engine.clone();
        let progress_task = progress_tracker.clone();
        let semaphore_permit = semaphore.clone();
        let movie_title = movie.title.clone();

        let task = tokio::spawn(async move {
            let _permit = semaphore_permit.acquire().await.unwrap();

            {
                let mut progress = progress_task.write().await;
                if let Some(p) = progress.get_mut(&movie_title) {
                    p.status = ConversionStatus::Converting;
                }
            }

            let result =
                convert_single_movie(movie, piece_store_task, peer_registry_task, engine_task)
                    .await;

            {
                let mut progress = progress_task.write().await;
                if let Some(p) = progress.get_mut(&movie_title) {
                    match result {
                        Ok(()) => {
                            p.status = ConversionStatus::Completed;
                            p.completed_at = Some(std::time::Instant::now());
                        }
                        Err(ref e) => {
                            p.status = ConversionStatus::Failed;
                            p.error_message = Some(e.to_string());
                            p.completed_at = Some(std::time::Instant::now());
                            eprintln!("Failed to convert movie {movie_title}: {e}");
                        }
                    }
                }
            }

            result
        });
        tasks.push(task);
    }

    let mut completed = 0;
    let mut failed = 0;
    for task in tasks {
        match task.await {
            Ok(Ok(())) => completed += 1,
            Ok(Err(_)) => failed += 1,
            Err(e) => {
                eprintln!("Background conversion task panicked: {e}");
                failed += 1;
            }
        }
    }

    println!("Background conversions completed: {completed} successful, {failed} failed");
}

/// Convert a single movie file to torrent in background (thread-safe)
async fn convert_single_movie(
    movie: riptide_core::storage::LibraryFile,
    piece_store: Arc<riptide_sim::InMemoryPieceStore>,
    peer_registry: Arc<Mutex<HashMap<InfoHash, Vec<SocketAddr>>>>,
    torrent_engine: TorrentEngineHandle,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    println!("Converting {} to BitTorrent pieces...", movie.title);

    let mut sim_creator = riptide_core::torrent::SimulationTorrentCreator::new();

    let (metadata, pieces) = sim_creator
        .create_with_pieces(
            &movie.file_path,
            vec!["http://development-tracker.riptide.local/announce".to_string()],
        )
        .await?;

    let canonical_info_hash = metadata.info_hash;

    torrent_engine
        .add_torrent_metadata(metadata.clone())
        .await?;
    piece_store
        .add_torrent_pieces(canonical_info_hash, pieces)
        .await?;

    let peer_addrs = generate_mock_peer_addresses(35, 45);

    {
        let mut registry = peer_registry.lock().unwrap();
        registry.insert(canonical_info_hash, peer_addrs.clone());
    }

    torrent_engine.start_download(canonical_info_hash).await?;

    println!(
        "✓ Converted {} ({} pieces, {} simulated peers)",
        movie.title,
        metadata.piece_hashes.len(),
        peer_addrs.len()
    );

    Ok(())
}
