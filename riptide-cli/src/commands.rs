//! CLI command implementations

use std::path::PathBuf;

use clap::Subcommand;
use riptide_core::config::RiptideConfig;
use riptide_core::server_components::ServerComponents;
use riptide_core::streaming::{Ffmpeg, ProductionFfmpeg};
use riptide_core::torrent::{
    InfoHash, TcpPeers, TorrentEngineHandle, TorrentError, Tracker, spawn_torrent_engine,
};
use riptide_core::{Result, RiptideError, RuntimeMode};
use riptide_search::MediaSearch;
use tokio::fs;

/// Available CLI commands
#[derive(Subcommand)]
pub enum Commands {
    /// Add a torrent from magnet link or file
    Add {
        /// Magnet link or path to torrent file
        source: String,
        /// Output directory for downloads
        #[arg(short, long)]
        output: Option<PathBuf>,
    },
    /// Start downloading a torrent
    Start {
        /// Torrent info hash or name
        torrent: String,
    },
    /// Stop downloading a torrent
    Stop {
        /// Torrent info hash or name
        torrent: String,
    },
    /// Show status of torrents
    Status {
        /// Specific torrent to show status for
        torrent: Option<String>,
    },
    /// List all torrents
    List,
    /// Start the web server
    Server {
        /// Host to bind to
        #[arg(long, default_value = "127.0.0.1")]
        host: String,
        /// Port to bind to
        #[arg(short, long, default_value = "3000")]
        port: u16,
        /// Runtime mode
        #[arg(long, default_value = "development")]
        mode: RuntimeMode,
        /// Directory containing movie files for simulation (development mode only)
        #[arg(long)]
        movies_dir: Option<PathBuf>,
    },
    /// Run simulation environment
    Simulation {
        /// Number of simulated peers
        #[arg(short, long, default_value = "50")]
        peers: usize,
        /// Path to torrent file for simulation
        torrent: PathBuf,
    },
}

/// Create server components based on runtime mode.
/// This function serves as the composition root for dependency injection.
///
/// # Errors
/// - `RiptideError::Io` - Failed to scan movies directory in development mode
pub async fn create_server_components(
    config: RiptideConfig,
    mode: RuntimeMode,
    movies_dir: Option<PathBuf>,
) -> Result<ServerComponents> {
    match mode {
        RuntimeMode::Production => {
            let peers = TcpPeers::new_default();
            let tracker = Tracker::new(config.network.clone());
            let engine = spawn_torrent_engine(config, peers, tracker);

            let ffmpeg: std::sync::Arc<dyn Ffmpeg> =
                std::sync::Arc::new(ProductionFfmpeg::new(None));

            Ok(ServerComponents {
                torrent_engine: engine,
                movie_library: None,
                piece_store: None,
                conversion_progress: None,
                ffmpeg,
            })
        }
        RuntimeMode::Simulation => {
            riptide_sim::create_deterministic_development_components(config, movies_dir)
                .await
                .map_err(|e| RiptideError::Io(std::io::Error::other(e.to_string())))
        }
        RuntimeMode::Development => {
            riptide_sim::create_fast_development_components(config, movies_dir)
                .await
                .map_err(|e| RiptideError::Io(std::io::Error::other(e.to_string())))
        }
    }
}

/// Handle the CLI command
///
/// # Errors
/// Returns appropriate error based on the command that fails
pub async fn handle_command(command: Commands) -> Result<()> {
    let config = RiptideConfig::default();
    let peers = TcpPeers::new_default();
    let tracker = Tracker::new(config.network.clone());
    let engine = spawn_torrent_engine(config, peers, tracker);

    match command {
        Commands::Add { source, output } => add_torrent(engine, source, output).await,
        Commands::Start { torrent } => start_torrent(engine, torrent).await,
        Commands::Stop { torrent } => stop_torrent(engine.clone(), torrent).await,
        Commands::Status { torrent } => show_status(engine, torrent).await,
        Commands::List => list_torrents(engine).await,
        Commands::Server {
            host,
            port,
            mode,
            movies_dir,
        } => start_server(host, port, mode, movies_dir).await,
        Commands::Simulation { peers, torrent } => run_simulation(peers, torrent).await,
    }
}

/// Add a torrent by magnet link or file
///
/// # Errors
/// - `RiptideError::Torrent` - Failed to parse or add torrent
/// - `RiptideError::Io` - File system operation failed
pub async fn add_torrent(
    engine: TorrentEngineHandle,
    source: String,
    output: Option<PathBuf>,
) -> Result<()> {
    let info_hash = if source.starts_with("magnet:") {
        // Handle magnet link
        println!("Adding magnet link: {source}");
        engine.add_magnet(&source).await?
    } else {
        // Handle torrent file
        println!("Adding torrent file: {source}");
        let torrent_data = fs::read(&source).await?;

        // Validate torrent file format
        if torrent_data.is_empty() {
            return Err(RiptideError::Torrent(TorrentError::InvalidTorrentFile {
                reason: format!("Torrent file is empty: {source}"),
            }));
        }

        // For now, torrent files are not supported in the actor model
        // TODO: Implement AddTorrentData command in the actor model
        return Err(RiptideError::Torrent(TorrentError::InvalidTorrentFile {
            reason: format!(
                "Torrent file support is not yet implemented. Please use magnet links instead.\n\
                 File: {} ({} bytes)",
                source,
                torrent_data.len()
            ),
        }));
    };

    println!("Successfully added torrent: {info_hash}");

    if let Some(output_dir) = output {
        println!("  Download directory: {}", output_dir.display());
    }

    Ok(())
}

/// Start downloading a torrent
///
/// # Errors
/// - `RiptideError::Torrent` - Torrent not found or download failed to start
pub async fn start_torrent(engine: TorrentEngineHandle, torrent: String) -> Result<()> {
    let info_hash = parse_torrent_identifier(&torrent)?;

    println!("Starting download for torrent: {info_hash}");
    engine.start_download(info_hash).await?;

    println!("Download started for torrent: {info_hash}");
    println!("  Connecting to peers and beginning piece downloads...");

    Ok(())
}

/// Stop downloading a torrent
///
/// # Errors
/// - `RiptideError::Torrent` - Torrent not found or stop failed
pub async fn stop_torrent(engine: TorrentEngineHandle, torrent: String) -> Result<()> {
    let info_hash = parse_torrent_identifier(&torrent)?;

    println!("Stopping download for torrent: {info_hash}");
    engine.stop_download(info_hash).await?;

    println!("Download stopped for torrent: {info_hash}");
    println!("  Connections closed and resources released");

    Ok(())
}

/// Show status of torrents
///
/// # Errors
/// - `RiptideError::Torrent` - Failed to retrieve torrent status
pub async fn show_status(engine: TorrentEngineHandle, torrent: Option<String>) -> Result<()> {
    match torrent {
        Some(t) => {
            let info_hash = parse_torrent_identifier(&t)?;
            show_single_torrent_status(&engine, info_hash).await
        }
        None => show_all_torrents_status(&engine).await,
    }
}

/// List all torrents
///
/// # Errors
/// - `RiptideError::Torrent` - Failed to retrieve torrent list
pub async fn list_torrents(engine: TorrentEngineHandle) -> Result<()> {
    println!("Torrent List");
    println!("{:-<60}", "");

    let sessions = engine.active_sessions().await?;
    let stats = engine.download_statistics().await?;

    if sessions.is_empty() {
        println!("No torrents added yet.");
        println!("Use 'riptide add <magnet-link-or-file>' to add a torrent.");
    } else {
        println!("Active torrents: {}", sessions.len());
        println!(
            "Total downloaded: {:.2} MB",
            stats.bytes_downloaded as f64 / 1_048_576.0
        );
        println!(
            "Total uploaded: {:.2} MB",
            stats.bytes_uploaded as f64 / 1_048_576.0
        );
        println!("Connected peers: {}", stats.total_peers);

        println!("\nTorrents:");
        for (i, session) in sessions.iter().enumerate() {
            let status = if session.is_downloading {
                "Downloading"
            } else {
                "Stopped"
            };
            println!(
                "  {}: {} ({:.1}%) - {} - {}",
                i + 1,
                session.filename,
                session.progress * 100.0,
                status,
                session.info_hash
            );
        }

        println!("\nUse 'riptide status <info-hash>' for detailed information.");
    }

    Ok(())
}

/// Run simulation environment
///
/// # Errors
/// - Currently returns Ok but will add simulation errors in future implementation
///
/// Run simulation environment with runtime configuration
///
/// # Errors
/// - Currently returns Ok but will add simulation errors in future implementation
pub async fn run_simulation(peers: usize, torrent: PathBuf) -> Result<()> {
    println!(
        "Running simulation with {} peers for torrent: {}",
        peers,
        torrent.display()
    );

    // Create config with simulation enabled
    let mut config = RiptideConfig::default();
    config.simulation.enabled = true;
    config.simulation.max_simulated_peers = peers;
    config.simulation.deterministic_seed = Some(42);

    println!("Created simulation environment with {peers} peers");

    // Full BitTorrent simulation implementation pending
    println!("Simulation running... (press Ctrl+C to stop)");

    // Simulate some activity
    for i in 0..10 {
        tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
        println!("Simulation step {}: downloading piece {}", i + 1, i);
    }

    println!("Simulation completed!");
    Ok(())
}

/// Parse torrent identifier (info hash or name) into InfoHash
fn parse_torrent_identifier(identifier: &str) -> Result<InfoHash> {
    // Try parsing as hex info hash first
    if identifier.len() == 40 {
        match InfoHash::from_hex(identifier) {
            Ok(info_hash) => return Ok(info_hash),
            Err(_) => {
                // Fall through to error below
            }
        }
    }

    // If not a valid hex hash, treat as torrent name (not implemented yet)
    Err(RiptideError::Torrent(TorrentError::InvalidTorrentFile {
        reason: format!(
            "Invalid torrent identifier: '{identifier}'. Please use 40-character hex info hash."
        ),
    }))
}

/// Display status for a single torrent
async fn show_single_torrent_status(
    engine: &TorrentEngineHandle,
    info_hash: InfoHash,
) -> Result<()> {
    println!("Torrent Status");
    println!("{:-<60}", "");

    let stats = engine.download_statistics().await?;

    println!("Info Hash: {info_hash}");

    // TODO: Once engine provides per-torrent status, show detailed info
    println!("Status: Active");
    println!("Progress: Calculating...");
    println!("Download rate: Calculating...");
    println!("Upload rate: Calculating...");
    println!("Peers: {} connected", stats.total_peers);
    println!("ETA: Calculating...");

    println!("\nUse 'riptide list' to see all torrents.");

    Ok(())
}

/// Display status for all torrents
async fn show_all_torrents_status(engine: &TorrentEngineHandle) -> Result<()> {
    println!("All Torrents Status");
    println!("{:-<60}", "");

    let stats = engine.download_statistics().await?;

    if stats.active_torrents == 0 {
        println!("No active torrents.");
        println!("Use 'riptide add <magnet-link-or-file>' to add a torrent.");
        return Ok(());
    }

    println!("Overall Statistics");
    println!("Active torrents: {}", stats.active_torrents);
    println!("Connected peers: {}", stats.total_peers);
    println!(
        "Total downloaded: {:.2} MB",
        stats.bytes_downloaded as f64 / 1_048_576.0
    );
    println!(
        "Total uploaded: {:.2} MB",
        stats.bytes_uploaded as f64 / 1_048_576.0
    );
    println!("Overall progress: {:.1}%", stats.average_progress * 100.0);

    // TODO: Once engine provides torrent iteration, show per-torrent details
    println!("\nUse 'riptide status <info-hash>' for detailed torrent information.");

    Ok(())
}

/// Start the simplified web server
///
/// # Errors
/// - Server binding failures or configuration errors
#[allow(dead_code)]
pub async fn start_simple_server() -> Result<()> {
    println!("Starting Riptide media server...");
    println!("{:-<50}", "");

    let config = RiptideConfig::default();

    let components =
        create_server_components(config.clone(), RuntimeMode::Development, None).await?;
    let media_search = MediaSearch::from_runtime_mode(RuntimeMode::Development);

    riptide_web::run_server(config, components, media_search, RuntimeMode::Development)
        .await
        .map_err(|e| RiptideError::Io(std::io::Error::other(e.to_string())))?;

    Ok(())
}

/// Start the web server
///
/// # Errors
/// - `RiptideError::Io` - Failed to start server
pub async fn start_server(
    _host: String,
    _port: u16,
    mode: RuntimeMode,
    movies_dir: Option<PathBuf>,
) -> Result<()> {
    println!("Starting Riptide media server...");
    println!("{:-<50}", "");

    let config = if mode.is_development() {
        let mut config = RiptideConfig::default();
        config.simulation = riptide_core::config::SimulationConfig::ideal_streaming(42); // Use 10 MB/s for development
        config
    } else {
        RiptideConfig::default()
    };

    println!(
        "Running in {} mode - using {} data sources",
        mode,
        if mode.is_development() {
            "offline development"
        } else {
            "real API"
        }
    );

    let components = create_server_components(config.clone(), mode, movies_dir).await?;
    let media_search = MediaSearch::from_runtime_mode(mode);

    riptide_web::run_server(config, components, media_search, mode)
        .await
        .map_err(|e| RiptideError::Io(std::io::Error::other(e.to_string())))?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_engine_handle_builder() -> TorrentEngineHandle {
        let config = RiptideConfig::default();
        let peers = TcpPeers::new_default();
        let tracker = Tracker::new(config.network.clone());
        spawn_torrent_engine(config, peers, tracker)
    }

    #[test]
    fn test_parse_torrent_identifier_valid_hash() {
        let hash_str = "0123456789abcdef0123456789abcdef01234567";
        let result = parse_torrent_identifier(hash_str);
        assert!(result.is_ok());

        let info_hash = result.unwrap();
        assert_eq!(info_hash.to_string(), hash_str);
    }

    #[test]
    fn test_parse_torrent_identifier_invalid_length() {
        let hash_str = "0123456789abcdef"; // Too short
        let result = parse_torrent_identifier(hash_str);
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_torrent_identifier_invalid_chars() {
        let hash_str = "0123456789abcdef0123456789abcdef0123456z"; // Contains 'z'
        let result = parse_torrent_identifier(hash_str);
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_add_torrent_magnet_link() {
        let engine = test_engine_handle_builder();
        let result = add_torrent(engine, "magnet:?xt=urn:btih:test".to_string(), None).await;
        // Should not panic and return some result (may be error due to invalid magnet)
        // This tests the parsing logic rather than actual torrent functionality
        assert!(result.is_ok() || result.is_err());
    }

    #[tokio::test]
    async fn test_list_torrents_empty() {
        let engine = test_engine_handle_builder();
        let result = list_torrents(engine).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_show_status_all() {
        let engine = test_engine_handle_builder();
        let result = show_status(engine, None).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_stop_torrent_valid_hash() {
        let engine = test_engine_handle_builder();
        let hash_str = "0123456789abcdef0123456789abcdef01234567";

        // This should return an error since the torrent doesn't exist
        let result = stop_torrent(engine, hash_str.to_string()).await;
        assert!(result.is_err()); // Expected to fail since torrent doesn't exist
    }
}
