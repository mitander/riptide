//! Riptide CLI - Command-line interface
//!
//! Provides command-line access to Riptide functionality.

mod commands;

use clap::Parser;

#[derive(Parser)]
#[command(name = "riptide")]
#[command(about = "A BitTorrent media server")]
struct Cli {
    #[command(subcommand)]
    command: commands::Commands,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt::init();

    let cli = Cli::parse();
    commands::handle_command(cli.command).await?;

    Ok(())
}
