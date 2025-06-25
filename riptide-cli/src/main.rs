//! Riptide CLI - Command-line interface
//!
//! Provides command-line access to Riptide functionality.

use clap::Parser;

#[derive(Parser)]
#[command(name = "riptide")]
#[command(about = "A BitTorrent media server")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(clap::Subcommand)]
enum Commands {
    /// Start the media server
    Server,
    /// Download a torrent
    Download { magnet: String },
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::init();

    let cli = Cli::parse();

    match &cli.command {
        Commands::Server => {
            println!("Starting Riptide server...");
            // TODO: Start server
        }
        Commands::Download { magnet } => {
            println!("Downloading: {}", magnet);
            // TODO: Start download
        }
    }

    Ok(())
}
