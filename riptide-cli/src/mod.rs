//! Command-line interface for Riptide

pub mod commands;

use std::path::PathBuf;

use clap::{Parser, Subcommand};

/// Main CLI structure for Riptide torrent client.
///
/// Defines command-line interface with global options and subcommands
/// for torrent management and simulation.
#[derive(Parser)]
#[command(name = "riptide")]
#[command(about = "Production-grade torrent media server")]
#[command(version)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,

    /// Configuration file path
    #[arg(short, long, default_value = "~/.config/riptide/config.toml")]
    pub config: PathBuf,

    /// Verbose output
    #[arg(short, long)]
    pub verbose: bool,
}

/// Available CLI commands.
///
/// Covers torrent lifecycle management from adding torrents through
/// monitoring status, plus simulation mode for development.
#[derive(Subcommand)]
pub enum Commands {
    /// Add a torrent by magnet link or file
    Add {
        /// Magnet link or path to .torrent file
        source: String,

        /// Download directory override
        #[arg(short, long)]
        output: Option<PathBuf>,
    },

    /// Start downloading a torrent
    Start {
        /// Info hash or torrent name
        torrent: String,
    },

    /// Stop downloading a torrent
    Stop {
        /// Info hash or torrent name
        torrent: String,
    },

    /// Show status of all or specific torrents
    Status {
        /// Specific torrent to show (optional)
        torrent: Option<String>,
    },

    /// List all torrents
    List,

    /// Start the simulation environment for testing
    #[cfg(feature = "simulation")]
    Simulate {
        /// Number of peers to simulate
        #[arg(short, long, default_value = "10")]
        peers: usize,

        /// Torrent file to simulate downloading
        torrent: PathBuf,
    },

    /// Start the web server for the dashboard and API
    Server {
        /// Port to bind to
        #[arg(short, long, default_value = "3000")]
        port: u16,

        /// Host to bind to
        #[arg(long, default_value = "127.0.0.1")]
        host: String,

        /// Runtime mode (production or development)
        #[arg(long, default_value = "development")]
        mode: riptide_core::RuntimeMode,

        /// Directory containing movie files for simulation (development mode only)
        #[arg(long)]
        movies_dir: Option<PathBuf>,
    },
}

/// Main CLI entry point
///
/// # Errors
/// - Command parsing errors from clap
/// - Individual command execution errors
pub async fn run_cli() -> crate::Result<()> {
    let cli = Cli::parse();

    // Initialize tracing
    let level = if cli.verbose {
        tracing::Level::DEBUG
    } else {
        tracing::Level::INFO
    };

    tracing_subscriber::fmt().with_max_level(level).init();

    match cli.command {
        Commands::Add { source, output } => commands::add_torrent(source, output).await,
        Commands::Start { torrent } => commands::start_torrent(torrent).await,
        Commands::Stop { torrent } => commands::stop_torrent(torrent).await,
        Commands::Status { torrent } => commands::show_status(torrent).await,
        Commands::List => commands::list_torrents().await,
        #[cfg(feature = "simulation")]
        Commands::Simulate { peers, torrent } => commands::run_simulation(peers, torrent).await,
        Commands::Server { port, host, mode, movies_dir } => commands::start_server(host, port, mode, movies_dir).await,
    }
}
