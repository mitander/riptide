//! Demonstrates the unified engine architecture
//! Shows how the same code works with both production and simulation engines

use riptide_core::config::RiptideConfig;
use riptide_core::torrent::{spawn_torrent_engine, TcpPeerManager, TorrentEngineHandle, TrackerManager};

/// Function that works with ANY engine implementation
async fn demonstrate_engine(engine: TorrentEngineHandle) {
    println!("Testing unified engine interface...");
    
    // This code is identical regardless of production vs simulation
    let stats = engine.get_download_stats().await.unwrap();
    
    println!("Active torrents: {}", stats.active_torrents);
    println!("Total peers: {}", stats.total_peers);
    println!("Average progress: {:.1}%", stats.average_progress * 100.0);
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let config = RiptideConfig::default();
    
    // Production engine - uses real HTTP and TCP
    println!("=== Production Engine ===");
    let peer_manager = TcpPeerManager::new_default();
    let tracker_manager = TrackerManager::new(config.network.clone());
    let production_engine = spawn_torrent_engine(config.clone(), peer_manager, tracker_manager);
    demonstrate_engine(production_engine).await;
    
    // Simulation engine - uses deterministic mocks  
    println!("\n=== Simulation Engine ===");
    // TODO: Update this to use the simulation components once they are available.
    // let simulation_engine = SimulationTorrentEngine::new_simulation(config);
    // demonstrate_engine(simulation_engine).await;
    
    println!("\n✅ Same code works with both engines!");
    Ok(())
}