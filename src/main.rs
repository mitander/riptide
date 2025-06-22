use riptide::cli;

#[tokio::main]
async fn main() -> riptide::Result<()> {
    cli::run_cli().await
}
