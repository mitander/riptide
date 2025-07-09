//! Riptide CLI - Command-line interface
//!
//! Provides command-line access to Riptide functionality.

mod commands;

use clap::Parser;
use riptide_core::{CliLogLevel, init_tracing};

#[derive(Parser)]
#[command(name = "riptide")]
#[command(about = "A BitTorrent media server")]
struct Cli {
    /// Set the log level for console output (file logs are always full debug)
    #[arg(long, value_enum, default_value = "info")]
    log_level: CliLogLevel,

    #[command(subcommand)]
    command: commands::Commands,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();

    // Initialize tracing with dual output: console (user level) + file (full debug)
    init_tracing(cli.log_level.as_tracing_level(), None)?;

    tracing::info!("Riptide CLI starting with log level: {}", cli.log_level);

    let result = commands::handle_command(cli.command).await;

    if let Err(e) = &result {
        tracing::error!("Command failed: {}", e);
    } else {
        tracing::info!("Command completed successfully");
    }

    result.map_err(|e| Box::new(e) as Box<dyn std::error::Error>)
}
