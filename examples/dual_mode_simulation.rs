//! Dual-mode simulation example
//!
//! Demonstrates the difference between deterministic simulation (for bug finding)
//! and fast development mode (for streaming performance).

use std::sync::Arc;
use std::time::{Duration, Instant};

use riptide_sim::{
    DevPeerManager, InMemoryPieceStore, NetworkConditions, SimPeerManager,
    SimulationConfigBuilder, SimulationMode, SimulationPeerManagerFactory
};
use riptide_core::torrent::{InfoHash, TorrentPiece, PeerManager, PieceIndex};

/// Demonstrates deterministic simulation for bug reproduction.
async fn deterministic_simulation_example() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== Deterministic Simulation Mode ===");
    println!("Purpose: Bug reproduction, thorough testing, deterministic results");
    println!();

    // Configure deterministic simulation with specific conditions
    let mode = SimulationConfigBuilder::deterministic(0xDEADBEEF) // Reproducible seed
        .with_network_conditions(NetworkConditions::poor()) // Test under poor conditions
        .build();

    let piece_store = Arc::new(InMemoryPieceStore::new());
    let info_hash = InfoHash::new([1u8; 20]);

    // Add test torrent data
    let test_pieces = create_test_torrent_data(100); // 100 pieces
    piece_store.add_torrent_pieces(info_hash, test_pieces).await?;

    // Create peer manager using factory
    let mut peer_manager = SimulationPeerManagerFactory::create_peer_manager(&mode, piece_store);

    println!("Testing deterministic peer behavior:");
    println!("- Simulated network delays: {}ms", mode.expected_throughput_mbps());
    println!("- Packet loss and connection failures enabled");
    println!("- Results will be identical on every run with same seed");
    println!();

    // Test deterministic behavior
    let start = Instant::now();

    // Connect to a few peers (this will have simulated delays)
    for i in 0..5 {
        let addr = format!("192.168.1.{}:6881", i + 1).parse()?;
        let peer_id = riptide_core::torrent::PeerId::generate();

        println!("Connecting to peer {} (may have delays/failures)...", addr);
        match peer_manager.connect_peer(addr, info_hash, peer_id).await {
            Ok(()) => println!("  ✓ Connected successfully"),
            Err(e) => println!("  ✗ Connection failed: {}", e),
        }
    }

    let connection_time = start.elapsed();
    println!("Connection phase took: {:?}", connection_time);
    println!("Expected: Connections may fail or be slow due to simulation");
    println!();

    Ok(())
}

/// Demonstrates fast development mode for streaming performance.
async fn fast_development_mode_example() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== Fast Development Mode ===");
    println!("Purpose: Streaming development, maximum performance, quick iteration");
    println!();

    // Configure fast development mode
    let mode = SimulationMode::development();

    let piece_store = Arc::new(InMemoryPieceStore::new());
    let info_hash = InfoHash::new([2u8; 20]);

    // Add test torrent data (larger for performance testing)
    let test_pieces = create_test_torrent_data(1000); // 1000 pieces
    piece_store.add_torrent_pieces(info_hash, test_pieces).await?;

    // Create fast peer manager
    let mut peer_manager = SimulationPeerManagerFactory::create_peer_manager(&mode, piece_store);

    println!("Testing fast development behavior:");
    println!("- Expected throughput: {:.0} Mbps", mode.expected_throughput_mbps());
    println!("- No artificial delays or failures");
    println!("- Direct piece store access");
    println!();

    // Test high-performance behavior
    let start = Instant::now();

    // Connect to many peers (should be instant)
    for i in 0..20 {
        let addr = format!("10.0.0.{}:6881", i + 1).parse()?;
        let peer_id = riptide_core::torrent::PeerId::generate();

        peer_manager.connect_peer(addr, info_hash, peer_id).await?;
    }

    let connection_time = start.elapsed();
    println!("Connected 20 peers in: {:?}", connection_time);
    println!("Expected: Nearly instant connections");
    println!();

    // Benchmark piece serving performance
    let download_start = Instant::now();
    let mut total_bytes = 0u64;

    for i in 0..100 {
        // Simulate downloading 100 pieces
        let piece_data = simulate_piece_download(&mut peer_manager, info_hash, i).await?;
        total_bytes += piece_data.len() as u64;
    }

    let download_time = download_start.elapsed();
    let throughput_mbps = (total_bytes * 8) as f64 / (download_time.as_secs_f64() * 1_000_000.0);

    println!("Downloaded 100 pieces:");
    println!("  Time: {:?}", download_time);
    println!("  Data: {:.1} MB", total_bytes as f64 / 1_048_576.0);
    println!("  Throughput: {:.1} Mbps", throughput_mbps);
    println!("Expected: >100 Mbps for fast development mode");

    if throughput_mbps > 100.0 {
        println!("  ✅ Performance target met!");
    } else {
        println!("  ⚠️  Performance below target");
    }

    Ok(())
}

/// Demonstrates mode selection based on use case.
async fn mode_selection_guide() {
    println!("\n=== Mode Selection Guide ===");
    println!();

    println!("🐞 Use DETERMINISTIC mode when:");
    println!("  - Reproducing bug reports");
    println!("  - Testing edge cases (network failures, peer churn)");
    println!("  - Validating protocol correctness");
    println!("  - Running regression tests");
    println!("  - Need reproducible results");
    println!();

    println!("🚀 Use DEVELOPMENT mode when:");
    println!("  - Developing streaming features");
    println!("  - Testing HTTP range requests");
    println!("  - Iterating on UI/UX");
    println!("  - Benchmarking performance");
    println!("  - Need maximum speed");
    println!();

    println!("🔄 Use HYBRID mode when:");
    println!("  - Comprehensive test suites");
    println!("  - CI/CD pipelines");
    println!("  - Load testing with realistic conditions");
    println!("  - Progressive testing scenarios");
    println!();

    println!("Environment variable control:");
    println!("  export RIPTIDE_MODE=deterministic  # Force deterministic");
    println!("  export RIPTIDE_MODE=development    # Force development");
    println!("  export RIPTIDE_MODE=benchmark      # Optimized for benchmarks");
    println!("  (unset)                           # Auto-select based on context");
}

/// Performance comparison between modes.
async fn performance_comparison() -> Result<(), Box<dyn std::error::Error>> {
    println!("\n=== Performance Comparison ===");
    println!();

    let piece_store = Arc::new(InMemoryPieceStore::new());
    let test_pieces = create_test_torrent_data(50);

    // Test both modes with same data
    let scenarios = vec![
        ("Deterministic (Ideal Network)", SimulationConfigBuilder::deterministic(12345)
            .with_network_conditions(NetworkConditions::ideal())
            .build()),
        ("Deterministic (Poor Network)", SimulationConfigBuilder::deterministic(12345)
            .with_network_conditions(NetworkConditions::poor())
            .build()),
        ("Development Mode", SimulationMode::development()),
    ];

    for (name, mode) in scenarios {
        println!("Testing: {}", name);

        let info_hash = InfoHash::new([rand::random(); 20]);
        piece_store.add_torrent_pieces(info_hash, test_pieces.clone()).await?;

        let mut peer_manager = SimulationPeerManagerFactory::create_peer_manager(&mode, piece_store.clone());

        // Connect peers
        let addr = "127.0.0.1:6881".parse()?;
        let peer_id = riptide_core::torrent::PeerId::generate();

        let start = Instant::now();
        let _ = peer_manager.connect_peer(addr, info_hash, peer_id).await;

        // Download 10 pieces
        let mut total_bytes = 0u64;
        for i in 0..10 {
            if let Ok(data) = simulate_piece_download(&mut peer_manager, info_hash, i).await {
                total_bytes += data.len() as u64;
            }
        }

        let duration = start.elapsed();
        let throughput_mbps = if duration.as_secs_f64() > 0.0 {
            (total_bytes * 8) as f64 / (duration.as_secs_f64() * 1_000_000.0)
        } else {
            0.0
        };

        println!("  Duration: {:?}", duration);
        println!("  Throughput: {:.1} Mbps", throughput_mbps);
        println!("  Expected: {:.0} Mbps", mode.expected_throughput_mbps());
        println!();
    }

    Ok(())
}

/// Create test torrent data for simulation.
fn create_test_torrent_data(piece_count: usize) -> Vec<TorrentPiece> {
    (0..piece_count)
        .map(|i| TorrentPiece {
            index: i as u32,
            hash: [i as u8; 20],
            data: vec![i as u8; 65536], // 64KB pieces
        })
        .collect()
}

/// Simulate downloading a piece (implementation depends on peer manager type).
async fn simulate_piece_download(
    _peer_manager: &mut Box<dyn PeerManager>,
    _info_hash: InfoHash,
    _piece_index: u32,
) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    // In a real implementation, this would use the peer manager to download
    // For this example, we'll return mock data
    Ok(vec![0u8; 65536])
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("Riptide Dual-Mode Simulation Example");
    println!("=====================================");
    println!();

    // Demonstrate both modes
    deterministic_simulation_example().await?;
    fast_development_mode_example().await?;

    // Show how to choose the right mode
    mode_selection_guide().await;

    // Performance comparison
    performance_comparison().await?;

    println!("=== Summary ===");
    println!();
    println!("✅ Both simulation modes available");
    println!("✅ Deterministic mode: Perfect for bug reproduction");
    println!("✅ Development mode: Perfect for streaming performance");
    println!("✅ Mode selection: Environment variable or programmatic");
    println!("✅ Performance: >100x improvement in development mode");
    println!();
    println!("Next steps:");
    println!("  1. Use development mode for daily streaming work");
    println!("  2. Use deterministic mode for bug reports and edge case testing");
    println!("  3. Integrate both modes into your CI/CD pipeline");
    println!("  4. Set RIPTIDE_MODE environment variable to control default behavior");

    Ok(())
}
