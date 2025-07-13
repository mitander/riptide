//! Riptide Simulation Framework - Deterministic testing for BitTorrent streaming.

#![warn(missing_docs)]
#![warn(clippy::missing_errors_doc)]
#![deny(clippy::missing_panics_doc)]
#![warn(clippy::too_many_lines)]
//!
//! This crate provides a comprehensive simulation environment for testing
//! BitTorrent protocol implementations, streaming algorithms, and network
//! behavior under controlled, reproducible conditions.
//!
//! # Features
//!
//! - **Deterministic Execution**: Same seed always produces identical results
//! - **Event-Based Simulation**: Precise control over timing and ordering
//! - **Network Simulation**: Configurable latency, packet loss, and bandwidth
//! - **Invariant Checking**: Validate protocol correctness during execution
//! - **Resource Governance**: Enforce memory, connection, and CPU limits
//! - **Edge Case Testing**: Pre-built scenarios for challenging conditions
//!
//! # Example
//!
//! ```rust,no_run
//! use riptide_sim::{DeterministicSimulation, SimulationConfig};
//! use riptide_core::torrent::InfoHash;
//! use std::time::Duration;
//!
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! // Configure simulation
//! let config = SimulationConfig {
//!     enabled: true,
//!     deterministic_seed: Some(12345),
//!     network_latency_ms: 50,
//!     packet_loss_rate: 0.01,
//!     max_simulated_peers: 20,
//!     simulated_download_speed: 5_242_880, // 5 MB/s
//!     use_mock_data: true,
//! };
//!
//! // Create and run simulation
//! let mut sim = DeterministicSimulation::new(config)?;
//! sim.create_streaming_scenario(100, Duration::from_secs(5))?;
//!
//! let report = sim.execute_for_duration(Duration::from_secs(30))?;
//! println!("Completed {} pieces", report.final_state.completed_pieces.len());
//! # Ok(())
//! # }
//! ```
//!
//! # Architecture
//!
//! The simulation framework consists of several key components:
//!
//! - **Deterministic Engine**: Core event scheduler with controlled time
//! - **Mock Components**: Simulated peers, trackers, and network behavior
//! - **Scenario Library**: Pre-built test cases for common situations
//! - **Invariant System**: Runtime validation of protocol correctness
//! - **Metrics Collection**: Detailed performance and behavior analysis

pub mod deterministic;
pub mod magneto_provider;
pub mod media;
pub mod network;
pub mod peer;
pub mod peer_server;
pub mod peers;
pub mod scenarios;
pub mod simulation_mode;
pub mod streaming;
pub mod tracker;

// Re-export core types for convenience
use std::collections::{HashMap, HashSet};
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

pub use deterministic::{
    BandwidthInvariant, DataIntegrityInvariant, DeterministicClock, DeterministicRng,
    DeterministicSimulation, EventPriority, EventType, Invariant, InvariantViolation,
    MinimumPeersInvariant, PeerBehavior, ResourceLimitInvariant, ResourceLimits, ResourceType,
    ResourceUsage, SimulationError, SimulationEvent, SimulationMetrics, SimulationReport,
    SimulationState, StreamingInvariant, ThrottleDirection,
};
pub use magneto_provider::{
    MockMagnetoProvider, MockMagnetoProviderBuilder, TorrentEntryParams,
    create_mock_magneto_client, create_streaming_test_client,
};
pub use media::{MediaStreamingSimulation, MovieFolder, StreamingResult};
pub use network::{NetworkSimulator, NetworkSimulatorBuilder};
pub use peer::{MockPeer, MockPeerBuilder};
pub use peer_server::{PeerServer, spawn_peer_servers_for_torrent};
pub use peers::{
    InMemoryPieceStore, PeersBuilder, PeersMode, SimulatedConfig, SimulatedPeers, SimulationSpeed,
    SimulationStats,
};
// Re-export config from core for convenience
pub use riptide_core::config::SimulationConfig;
pub use scenarios::{
    cascading_piece_failures_scenario, extreme_peer_churn_scenario, resource_exhaustion_scenario,
    severe_network_degradation_scenario, streaming_edge_cases, total_peer_failure_scenario,
};
pub use simulation_mode::{
    NetworkConditions, SimulationConfigBuilder, SimulationMode, SimulationPeersBuilder,
};
pub use tracker::{ResponseConfig, SimulatedTracker};

/// Simulation environment for BitTorrent development.
///
/// Combines mock tracker, network simulator, and peer pool for
/// comprehensive testing of BitTorrent functionality.
pub struct SimulationEnvironment {
    /// Mock tracker for announce/scrape operations
    pub tracker: SimulatedTracker,
    /// Network simulator for latency and packet loss
    pub network: NetworkSimulator,
    /// Pool of mock peers for testing
    pub peers: Vec<MockPeer>,
}

impl Default for SimulationEnvironment {
    fn default() -> Self {
        Self::new()
    }
}

impl SimulationEnvironment {
    /// Creates a new simulation environment with sensible defaults.
    pub fn new() -> Self {
        Self {
            tracker: SimulatedTracker::new(),
            network: NetworkSimulator::new(),
            peers: Vec::new(),
        }
    }

    /// Creates environment optimized for streaming development.
    pub fn for_streaming() -> Self {
        let mut env = Self::new();

        // Configure network for realistic streaming conditions
        env.network = NetworkSimulator::builder()
            .latency(10..100) // 10-100ms typical home internet
            .packet_loss(0.001) // 0.1% loss
            .bandwidth_limit(50_000_000) // 50 Mbps
            .build();

        env.add_fast_peers(10);
        env.add_slow_peers(5);
        env.add_unreliable_peers(2);

        env
    }

    /// Adds fast, reliable peers to the simulation.
    pub fn add_fast_peers(&mut self, count: usize) {
        for i in 0..count {
            let peer = MockPeer::builder()
                .upload_speed(5_000_000) // 5 MB/s
                .reliability(0.99)
                .latency(Duration::from_millis(20))
                .peer_id(format!("FAST{i:04}"))
                .build();
            self.peers.push(peer);
        }
    }

    /// Adds slow peers that simulate real-world conditions.
    pub fn add_slow_peers(&mut self, count: usize) {
        for i in 0..count {
            let peer = MockPeer::builder()
                .upload_speed(500_000) // 500 KB/s
                .reliability(0.95)
                .latency(Duration::from_millis(100))
                .peer_id(format!("SLOW{i:04}"))
                .build();
            self.peers.push(peer);
        }
    }

    /// Adds unreliable peers for testing error handling.
    pub fn add_unreliable_peers(&mut self, count: usize) {
        for i in 0..count {
            let peer = MockPeer::builder()
                .upload_speed(1_000_000) // 1 MB/s
                .reliability(0.70) // Drops connections frequently
                .latency(Duration::from_millis(200))
                .peer_id(format!("UNREL{i:03}"))
                .build();
            self.peers.push(peer);
        }
    }
}

/// Invariant that prevents duplicate piece downloads.
struct NoDuplicateDownloadsInvariant;

impl Invariant for NoDuplicateDownloadsInvariant {
    fn check(&self, state: &SimulationState) -> std::result::Result<(), InvariantViolation> {
        let mut seen_pieces = HashSet::new();

        for piece_index in state.downloading_pieces.keys() {
            if !seen_pieces.insert(*piece_index) {
                return Err(InvariantViolation {
                    invariant: "NoDuplicateDownloads".to_string(),
                    description: format!("Piece {piece_index} is being downloaded multiple times"),
                    timestamp: Instant::now(),
                });
            }
        }

        Ok(())
    }

    fn name(&self) -> &str {
        "NoDuplicateDownloads"
    }
}

/// Invariant that enforces maximum peer count.
struct MaxPeersInvariant {
    max_peers: usize,
}

impl MaxPeersInvariant {
    fn new(max_peers: usize) -> Self {
        Self { max_peers }
    }
}

impl Invariant for MaxPeersInvariant {
    fn check(&self, state: &SimulationState) -> std::result::Result<(), InvariantViolation> {
        if state.connected_peers.len() > self.max_peers {
            return Err(InvariantViolation {
                invariant: "MaxPeers".to_string(),
                description: format!(
                    "Too many peers connected: {} > {}",
                    state.connected_peers.len(),
                    self.max_peers
                ),
                timestamp: Instant::now(),
            });
        }
        Ok(())
    }

    fn name(&self) -> &str {
        "MaxPeers"
    }
}

/// Creates a standard streaming simulation with common invariants.
///
/// This is a convenience function for quick testing that sets up
/// a simulation with reasonable defaults and common invariants.
///
/// # Errors
///
/// - `SimulationError::InvalidEventScheduling` - Invalid configuration
pub fn create_standard_streaming_simulation(
    seed: u64,
    piece_count: u32,
) -> Result<DeterministicSimulation> {
    let config = SimulationConfig::bandwidth_limited(seed);

    let mut sim = DeterministicSimulation::new(config)?;

    sim.add_invariant(Arc::new(MaxPeersInvariant::new(25)));
    sim.add_invariant(Arc::new(NoDuplicateDownloadsInvariant));
    sim.add_invariant(Arc::new(ResourceLimitInvariant::new(
        ResourceLimits::default(),
    )));

    sim.create_streaming_scenario(piece_count, std::time::Duration::from_secs(1))?;

    Ok(sim)
}

/// Create fast server components using DevPeers.
async fn create_fast_server_components(
    engine: riptide_core::torrent::TorrentEngineHandle,
    piece_store_sim: Arc<InMemoryPieceStore>,
    movies_dir: Option<std::path::PathBuf>,
) -> Result<riptide_core::server_components::ServerComponents> {
    use std::collections::HashMap;
    use std::net::SocketAddr;
    use std::sync::{Arc, Mutex};
    use std::time::Instant;

    use riptide_core::server_components::{ConversionProgress, ConversionStatus, ServerComponents};
    use riptide_core::storage::FileLibrary;
    use riptide_core::torrent::{InfoHash, PieceStore};
    use tokio::sync::RwLock;

    let conversion_progress = Arc::new(RwLock::new(HashMap::new()));
    let peer_registry = Arc::new(Mutex::new(HashMap::<InfoHash, Vec<SocketAddr>>::new()));

    let library_opt = if let Some(dir) = movies_dir.as_ref() {
        let mut library = FileLibrary::new();

        match library.scan_directory(dir).await {
            Ok(count) => {
                println!("Found {} movie files in {}", count, dir.display());

                let movies: Vec<_> = library.all_files().into_iter().cloned().collect();

                {
                    let mut progress = conversion_progress.write().await;
                    for movie in &movies {
                        progress.insert(
                            movie.title.clone(),
                            ConversionProgress {
                                movie_title: movie.title.clone(),
                                status: ConversionStatus::Pending,
                                started_at: Instant::now(),
                                completed_at: None,
                                error_message: None,
                            },
                        );
                    }
                }

                if !movies.is_empty() {
                    println!("Starting background conversion service with DevPeers...");
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

                Some(Arc::new(RwLock::new(library)))
            }
            Err(e) => {
                eprintln!("Warning: Failed to scan movies directory: {e}");
                None
            }
        }
    } else {
        None
    };

    Ok(ServerComponents {
        torrent_engine: engine,
        movie_library: library_opt,
        piece_store: Some(piece_store_sim as Arc<dyn PieceStore>),
        conversion_progress: Some(conversion_progress),
        ffmpeg: std::sync::Arc::new(riptide_core::streaming::SimulationFfmpeg::new()),
    })
}

/// Create deterministic server components using ContentAwarePeers.
async fn create_deterministic_server_components(
    engine: riptide_core::torrent::TorrentEngineHandle,
    piece_store_sim: Arc<InMemoryPieceStore>,
    movies_dir: Option<std::path::PathBuf>,
    peer_registry: Arc<Mutex<HashMap<riptide_core::torrent::InfoHash, Vec<std::net::SocketAddr>>>>,
) -> Result<riptide_core::server_components::ServerComponents> {
    use std::collections::HashMap;
    use std::time::Instant;

    use riptide_core::server_components::{ConversionProgress, ConversionStatus, ServerComponents};
    use riptide_core::storage::FileLibrary;
    use riptide_core::torrent::PieceStore;
    use tokio::sync::RwLock;

    let conversion_progress = Arc::new(RwLock::new(HashMap::new()));

    let library_opt = if let Some(dir) = movies_dir.as_ref() {
        let mut library = FileLibrary::new();

        match library.scan_directory(dir).await {
            Ok(count) => {
                println!("Found {} movie files in {}", count, dir.display());

                let movies: Vec<_> = library.all_files().into_iter().cloned().collect();

                {
                    let mut progress = conversion_progress.write().await;
                    for movie in &movies {
                        progress.insert(
                            movie.title.clone(),
                            ConversionProgress {
                                movie_title: movie.title.clone(),
                                status: ConversionStatus::Pending,
                                started_at: Instant::now(),
                                completed_at: None,
                                error_message: None,
                            },
                        );
                    }
                }

                if !movies.is_empty() {
                    println!("Starting background conversion service with ContentAwarePeers...");
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

                Some(Arc::new(RwLock::new(library)))
            }
            Err(e) => {
                eprintln!("Warning: Failed to scan movies directory: {e}");
                None
            }
        }
    } else {
        None
    };

    Ok(ServerComponents {
        torrent_engine: engine,
        movie_library: library_opt,
        piece_store: Some(piece_store_sim as Arc<dyn PieceStore>),
        conversion_progress: Some(conversion_progress),
        ffmpeg: std::sync::Arc::new(riptide_core::streaming::SimulationFfmpeg::new()),
    })
}

/// Create fast development server components using DevPeers.
/// This provides realistic streaming performance (5-10 MB/s) for development.
///
/// # Errors
///
/// - `SimulationError::Setup` - If failed to initialize simulation components
/// - `SimulationError::FileSystem` - If failed to scan movies directory
pub async fn create_fast_development_components(
    config: riptide_core::config::RiptideConfig,
    movies_dir: Option<std::path::PathBuf>,
) -> Result<riptide_core::server_components::ServerComponents> {
    use std::collections::HashMap;
    use std::net::SocketAddr;
    use std::sync::{Arc, Mutex};

    use riptide_core::torrent::{InfoHash, spawn_torrent_engine};

    let piece_store_sim = Arc::new(InMemoryPieceStore::new());
    let peer_registry = Arc::new(Mutex::new(HashMap::<InfoHash, Vec<SocketAddr>>::new()));

    tracing::info!(
        "Fast development components: Using DevPeers for realistic streaming performance"
    );
    let peers_sim = SimulatedPeers::new_instant(piece_store_sim.clone());

    let tracker_sim = tracker::SimulatedTracker::with_peer_registry(
        tracker::ResponseConfig::default(),
        peer_registry.clone(),
    );

    let engine = spawn_torrent_engine(config, peers_sim, tracker_sim);
    create_fast_server_components(engine, piece_store_sim, movies_dir).await
}

/// Create deterministic development server components using ContentAwarePeers.
/// This provides deterministic, reproducible behavior for testing and bug reproduction.
///
/// # Errors
///
/// - `SimulationError::Setup` - If failed to initialize simulation components
/// - `SimulationError::FileSystem` - If failed to scan movies directory
pub async fn create_deterministic_development_components(
    config: riptide_core::config::RiptideConfig,
    movies_dir: Option<std::path::PathBuf>,
) -> Result<riptide_core::server_components::ServerComponents> {
    use std::collections::HashMap;
    use std::net::SocketAddr;
    use std::sync::{Arc, Mutex};

    use riptide_core::torrent::{InfoHash, spawn_torrent_engine};

    let piece_store_sim = Arc::new(InMemoryPieceStore::new());
    let peer_registry = Arc::new(Mutex::new(HashMap::<InfoHash, Vec<SocketAddr>>::new()));

    tracing::info!("Deterministic development components: Using SimPeers for testing");
    let realistic_peer_config = SimulatedConfig {
        simulation_speed: SimulationSpeed::Realistic,
        message_delay_ms: 1,          // Minimal delay for development
        connection_failure_rate: 0.0, // No failures for development
        message_loss_rate: 0.0,       // No loss for development
        max_connections: 100,
        upload_rate_bps: 10 * 1024 * 1024,   // 10 MB/s
        streaming_rate_bps: 8 * 1024 * 1024, // 8 MB/s for development
        seed: 12345,
    };

    let peers_sim = SimulatedPeers::new(realistic_peer_config, piece_store_sim.clone());

    let tracker_sim = tracker::SimulatedTracker::with_peer_registry(
        tracker::ResponseConfig::default(),
        peer_registry.clone(),
    );

    let engine = spawn_torrent_engine(config, peers_sim, tracker_sim);
    create_deterministic_server_components(engine, piece_store_sim, movies_dir, peer_registry).await
}

/// Legacy function - now routes to fast development mode for backward compatibility.
/// Use create_fast_development_components() or create_deterministic_development_components() directly.
///
/// # Errors
///
/// - `SimulationError::Setup` - If failed to initialize simulation components
/// - `SimulationError::FileSystem` - If failed to scan movies directory
pub async fn create_development_components(
    config: riptide_core::config::RiptideConfig,
    movies_dir: Option<std::path::PathBuf>,
) -> Result<riptide_core::server_components::ServerComponents> {
    // Default to fast development for backward compatibility
    create_fast_development_components(config, movies_dir).await
}

/// Background conversion service that processes all movies without blocking server startup
async fn start_background_conversions(
    movies: Vec<riptide_core::storage::LibraryFile>,
    piece_store: Arc<InMemoryPieceStore>,
    peer_registry: Arc<
        Mutex<std::collections::HashMap<riptide_core::torrent::InfoHash, Vec<SocketAddr>>>,
    >,
    torrent_engine: riptide_core::torrent::TorrentEngineHandle,
    progress_tracker: Arc<
        tokio::sync::RwLock<
            std::collections::HashMap<String, riptide_core::server_components::ConversionProgress>,
        >,
    >,
) {
    use riptide_core::server_components::ConversionStatus;

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
                            p.completed_at = Some(Instant::now());
                        }
                        Err(ref e) => {
                            p.status = ConversionStatus::Failed;
                            p.error_message = Some(e.to_string());
                            p.completed_at = Some(Instant::now());
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
    piece_store: Arc<InMemoryPieceStore>,
    peer_registry: Arc<
        Mutex<std::collections::HashMap<riptide_core::torrent::InfoHash, Vec<SocketAddr>>>,
    >,
    torrent_engine: riptide_core::torrent::TorrentEngineHandle,
) -> std::result::Result<(), Box<dyn std::error::Error + Send + Sync>> {
    use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4};

    use riptide_core::torrent::SimulationTorrentCreator;

    println!("Converting {} to BitTorrent pieces...", movie.title);

    let mut sim_creator = SimulationTorrentCreator::new();

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
        .await;

    // Generate mock peer addresses for simulation
    let peer_count = 35 + (rand::random::<u32>() % 11); // 35-45 peers
    let mut peer_addrs = Vec::new();
    for i in 0..peer_count {
        let addr = SocketAddr::V4(SocketAddrV4::new(
            Ipv4Addr::new(192, 168, (i / 256) as u8, (i % 256) as u8),
            6881 + (i as u16 % 1000),
        ));
        peer_addrs.push(addr);
    }

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

/// Common simulation error type for convenience.
pub type Result<T> = std::result::Result<T, SimulationError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_simulation_environment_creation() {
        let env = SimulationEnvironment::new();
        assert_eq!(env.peers.len(), 0);

        let streaming_env = SimulationEnvironment::for_streaming();
        assert_eq!(streaming_env.peers.len(), 17); // 10 fast + 5 slow + 2 unreliable
    }

    #[test]
    fn test_standard_streaming_simulation() {
        let sim = create_standard_streaming_simulation(12345, 100).unwrap();
        assert_eq!(sim.simulation_seed(), 12345);
    }
}
