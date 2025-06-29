//! Demonstrates the unified engine architecture
//!
//! Shows how the same code works with both production and simulation engines

use riptide_core::{ProductionTorrentEngine, RiptideConfig, RuntimeMode, SimulationTorrentEngine, TorrentEngineOps};
use std::sync::Arc;
use tokio::sync::RwLock;

/// Function that works with ANY engine implementation
async fn demonstrate_engine(engine: Arc<RwLock<dyn TorrentEngineOps>>) {
    println!("Testing unified engine interface...");
    
    // This code is identical regardless of production vs simulation
    let stats = {
        let engine = engine.read().await;
        engine.get_download_stats().await
    };
    
    println!("Active torrents: {}", stats.active_torrents);
    println!("Total peers: {}", stats.total_peers);
    println!("Average progress: {:.1}%", stats.average_progress * 100.0);
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let config = RiptideConfig::default();
    
    // Production engine - uses real HTTP and TCP
    println!("=== Production Engine ===");
    let production_engine = ProductionTorrentEngine::new_production(config.clone());
    let production_boxed: Arc<RwLock<dyn TorrentEngineOps>> = Arc::new(RwLock::new(production_engine));
    demonstrate_engine(production_boxed).await;
    
    // Simulation engine - uses deterministic mocks  
    println!("\n=== Simulation Engine ===");
    let simulation_engine = SimulationTorrentEngine::new_simulation(config);
    let simulation_boxed: Arc<RwLock<dyn TorrentEngineOps>> = Arc::new(RwLock::new(simulation_engine));
    demonstrate_engine(simulation_boxed).await;
    
    println!("\n✅ Same code works with both engines!");
    Ok(())
}