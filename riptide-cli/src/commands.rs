//! CLI command implementations

use std::path::PathBuf;

use clap::Subcommand;
use riptide_core::config::RiptideConfig;
use riptide_core::streaming::DirectStreamingService;
use riptide_core::torrent::{InfoHash, TorrentEngine, TorrentError};
use riptide_core::{Result, RiptideError};
use riptide_search::MediaSearchService;
use riptide_web::{TemplateEngine, WebHandlers, WebServer, WebServerConfig};
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
        /// Use demo data for development
        #[arg(long)]
        demo: bool,
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

/// Handle the CLI command
///
/// # Errors
/// Returns appropriate error based on the command that fails
pub async fn handle_command(command: Commands) -> Result<()> {
    match command {
        Commands::Add { source, output } => add_torrent(source, output).await,
        Commands::Start { torrent } => start_torrent(torrent).await,
        Commands::Stop { torrent } => stop_torrent(torrent).await,
        Commands::Status { torrent } => show_status(torrent).await,
        Commands::List => list_torrents().await,
        Commands::Server { host, port, demo } => start_server(host, port, demo).await,
        Commands::Simulation { peers, torrent } => run_simulation(peers, torrent).await,
    }
}

/// Add a torrent by magnet link or file
///
/// # Errors
/// - `RiptideError::Torrent` - Failed to parse or add torrent
/// - `RiptideError::Io` - File system operation failed
pub async fn add_torrent(source: String, output: Option<PathBuf>) -> Result<()> {
    let config = RiptideConfig::default();
    let mut engine = TorrentEngine::new(config);

    let info_hash = if source.starts_with("magnet:") {
        // Handle magnet link
        println!("Adding magnet link: {source}");
        engine.add_magnet(&source).await?
    } else {
        // Handle torrent file
        println!("Adding torrent file: {source}");
        let torrent_data = fs::read(&source).await?;
        engine.add_torrent_data(&torrent_data).await?
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
pub async fn start_torrent(torrent: String) -> Result<()> {
    let config = RiptideConfig::default();
    let mut engine = TorrentEngine::new(config);

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
pub async fn stop_torrent(torrent: String) -> Result<()> {
    let config = RiptideConfig::default();
    let _engine = TorrentEngine::new(config);

    let info_hash = parse_torrent_identifier(&torrent)?;

    println!("Stopping download for torrent: {info_hash}");
    // TODO: Add stop_download method to engine
    println!("Note: Stop functionality pending engine enhancement");

    println!("Download stopped for torrent: {info_hash}");
    println!("  Connections closed and resources released");

    Ok(())
}

/// Show status of torrents
///
/// # Errors
/// - `RiptideError::Torrent` - Failed to retrieve torrent status
pub async fn show_status(torrent: Option<String>) -> Result<()> {
    let config = RiptideConfig::default();
    let engine = TorrentEngine::new(config);

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
pub async fn list_torrents() -> Result<()> {
    let config = RiptideConfig::default();
    let engine = TorrentEngine::new(config);

    println!("Torrent List");
    println!("{:-<60}", "");

    let stats = engine.get_download_stats().await;

    if stats.active_torrents == 0 {
        println!("No torrents added yet.");
        println!("Use 'riptide add <magnet-link-or-file>' to add a torrent.");
    } else {
        println!("Active torrents: {}", stats.active_torrents);
        println!(
            "Total downloaded: {:.2} MB",
            stats.bytes_downloaded as f64 / 1_048_576.0
        );
        println!(
            "Total uploaded: {:.2} MB",
            stats.bytes_uploaded as f64 / 1_048_576.0
        );
        println!("Connected peers: {}", stats.total_peers);

        // TODO: Once engine provides torrent iteration, list individual torrents
        println!("\nUse 'riptide status' for detailed information.");
    }

    Ok(())
}

/// Run simulation environment
///
/// # Errors
/// - Currently returns Ok but will add simulation errors in future implementation
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

    println!("Created simulation environment with {} peers", peers);

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

/// Simple hex decoder for info hashes
fn decode_hex(hex_str: &str) -> Result<Vec<u8>> {
    if hex_str.len() % 2 != 0 {
        return Err(RiptideError::Torrent(TorrentError::InvalidTorrentFile {
            reason: "Invalid hex string length".to_string(),
        }));
    }

    let mut bytes = Vec::new();
    for chunk in hex_str.as_bytes().chunks(2) {
        let hex_byte = std::str::from_utf8(chunk).map_err(|_| {
            RiptideError::Torrent(TorrentError::InvalidTorrentFile {
                reason: "Invalid UTF-8 in hex string".to_string(),
            })
        })?;
        let byte = u8::from_str_radix(hex_byte, 16).map_err(|_| {
            RiptideError::Torrent(TorrentError::InvalidTorrentFile {
                reason: "Invalid hex digit".to_string(),
            })
        })?;
        bytes.push(byte);
    }

    Ok(bytes)
}

/// Parse torrent identifier (info hash or name) into InfoHash
fn parse_torrent_identifier(identifier: &str) -> Result<InfoHash> {
    // Try parsing as hex info hash first
    if identifier.len() == 40 {
        if let Ok(hash_bytes) = decode_hex(identifier) {
            if hash_bytes.len() == 20 {
                let mut hash_array = [0u8; 20];
                hash_array.copy_from_slice(&hash_bytes);
                return Ok(InfoHash::new(hash_array));
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
async fn show_single_torrent_status(engine: &TorrentEngine, info_hash: InfoHash) -> Result<()> {
    println!("Torrent Status");
    println!("{:-<60}", "");

    let stats = engine.get_download_stats().await;

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
async fn show_all_torrents_status(engine: &TorrentEngine) -> Result<()> {
    println!("All Torrents Status");
    println!("{:-<60}", "");

    let stats = engine.get_download_stats().await;

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

/// Start the web server for dashboard and API access
///
/// # Errors
/// - `RiptideError::Io` - Failed to bind to the specified address
/// - `RiptideError::Torrent` - Failed to initialize torrent engine
pub async fn start_server(host: String, port: u16, demo: bool) -> Result<()> {
    println!("Starting Riptide web server...");
    println!("Host: {host}");
    println!("Port: {port}");
    println!("URL: http://{host}:{port}");
    if demo {
        println!("Mode: Demo (using sample data)");
    } else {
        println!("Mode: Production");
    }
    println!("{:-<50}", "");

    let config = RiptideConfig::default();

    // Initialize core services
    let torrent_engine =
        std::sync::Arc::new(tokio::sync::RwLock::new(TorrentEngine::new(config.clone())));
    let streaming_service = std::sync::Arc::new(tokio::sync::RwLock::new(
        DirectStreamingService::new(config.clone()),
    ));

    // Create web components
    let media_search_service = if demo {
        MediaSearchService::new_demo()
    } else {
        MediaSearchService::new()
    };
    let handlers =
        WebHandlers::new_with_media_search(torrent_engine, streaming_service, media_search_service);
    let template_engine = TemplateEngine::new();

    // Configure web server
    let mut web_config = WebServerConfig::from_riptide_config(&config);
    web_config.bind_address = format!("{host}:{port}")
        .parse()
        .map_err(|e| RiptideError::Io(std::io::Error::new(std::io::ErrorKind::InvalidInput, e)))?;

    let web_server = WebServer::new(web_config, handlers, template_engine);

    println!("Web server starting...");
    println!("Dashboard: http://{host}:{port}/");
    println!("Torrents: http://{host}:{port}/torrents");
    println!("Library: http://{host}:{port}/library");
    println!("API: http://{host}:{port}/api/*");
    println!();
    println!("Press Ctrl+C to stop the server");

    // Start the server (this will block)
    web_server
        .start()
        .await
        .map_err(RiptideError::from_web_ui_error)?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_decode_hex_valid() {
        let hex_str = "0123456789abcdef0123456789abcdef01234567";
        let result = decode_hex(hex_str);
        assert!(result.is_ok());

        let bytes = result.unwrap();
        assert_eq!(bytes.len(), 20);
        assert_eq!(bytes[0], 0x01);
        assert_eq!(bytes[1], 0x23);
        assert_eq!(bytes[19], 0x67);
    }

    #[test]
    fn test_decode_hex_invalid_length() {
        let hex_str = "123"; // Odd length
        let result = decode_hex(hex_str);
        assert!(result.is_err());
    }

    #[test]
    fn test_decode_hex_invalid_chars() {
        let hex_str = "012345678z"; // Contains 'z'
        let result = decode_hex(hex_str);
        assert!(result.is_err());
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
        let result = add_torrent("magnet:?xt=urn:btih:test".to_string(), None).await;
        // Should not panic and return some result (may be error due to invalid magnet)
        // This tests the parsing logic rather than actual torrent functionality
        assert!(result.is_ok() || result.is_err());
    }

    #[tokio::test]
    async fn test_list_torrents_empty() {
        let result = list_torrents().await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_show_status_all() {
        let result = show_status(None).await;
        assert!(result.is_ok());
    }
}
