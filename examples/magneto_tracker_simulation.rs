//! Magneto integration with tracker simulation example
//!
//! Demonstrates using magneto for magnet link discovery and mock tracker
//! interaction simulation for testing BitTorrent functionality offline.

use std::time::Duration;

use riptide::simulation::MockPeer;
use riptide::simulation::tracker::MockTracker;
use riptide::torrent::InfoHash;

/// Demonstrates magneto integration for torrent discovery simulation.
async fn magneto_discovery_simulation() -> Result<(), Box<dyn std::error::Error>> {
    println!("Magneto Integration Simulation");
    println!("{:-<50}", "");

    // Create tracker with magneto enabled
    let mut tracker = MockTracker::builder()
        .with_magneto(true)
        .with_seeders(20)
        .with_leechers(5)
        .with_seed(0x1234ABCD)
        .build();

    println!("Created mock tracker with magneto integration");

    // Search for popular content to populate tracker
    let search_queries = [
        "big buck bunny",
        "sintel",
        "tears of steel",
        "elephants dream",
    ];

    for query in &search_queries {
        println!("\nSearching for: '{}'", query);

        match tracker.search_and_add_torrents(query, 3).await {
            Ok(torrents) => {
                println!("  Found {} torrents:", torrents.len());
                for torrent in &torrents {
                    println!(
                        "    - {} ({:.1} MB, {} seeders, {} leechers)",
                        torrent.name,
                        torrent.size as f64 / 1_048_576.0,
                        torrent.seeders,
                        torrent.leechers
                    );
                }
            }
            Err(e) => {
                println!("  Search failed: {}", e);
            }
        }

        // Small delay between searches
        tokio::time::sleep(Duration::from_millis(500)).await;
    }

    // Display tracker statistics
    let tracked_torrents = tracker.tracked_torrents();
    println!("\nTracker Statistics:");
    println!("  Total tracked torrents: {}", tracked_torrents.len());

    let total_seeders: u32 = tracked_torrents.iter().map(|t| t.seeders).sum();
    let total_leechers: u32 = tracked_torrents.iter().map(|t| t.leechers).sum();
    let total_size: u64 = tracked_torrents.iter().map(|t| t.size).sum();

    println!("  Total seeders: {}", total_seeders);
    println!("  Total leechers: {}", total_leechers);
    println!(
        "  Total content size: {:.1} GB",
        total_size as f64 / 1_073_741_824.0
    );

    Ok(())
}

/// Demonstrates magnet link discovery without adding to tracker.
async fn magnet_link_discovery() -> Result<(), Box<dyn std::error::Error>> {
    println!("\nMagnet Link Discovery");
    println!("{:-<50}", "");

    let tracker = MockTracker::builder()
        .with_magneto(true)
        .with_seed(0x123456)
        .build();

    let query = "open source movies";
    println!("Discovering magnet links for: '{}'", query);

    match tracker.discover_magnet_links(query, 5).await {
        Ok(magnet_links) => {
            println!("Found {} magnet links:", magnet_links.len());
            for (i, link) in magnet_links.iter().enumerate() {
                println!("  {}: {}", i + 1, link);
            }
        }
        Err(e) => {
            println!("Discovery failed: {}", e);
        }
    }

    Ok(())
}

/// Simulates realistic tracker announces with discovered content.
async fn tracker_announce_simulation(
    tracker: &mut MockTracker,
) -> Result<(), Box<dyn std::error::Error>> {
    println!("\nTracker Announce Simulation");
    println!("{:-<50}", "");

    let torrent_hashes: Vec<(InfoHash, String)> = tracker
        .tracked_torrents()
        .iter()
        .take(3)
        .map(|t| (t.info_hash, t.name.clone()))
        .collect();

    if torrent_hashes.is_empty() {
        println!("No torrents available for announce simulation");
        return Ok(());
    }

    // Simulate announces for discovered torrents
    for (info_hash, name) in torrent_hashes {
        println!("\nAnnouncing torrent: {}", name);

        match tracker.announce(info_hash).await {
            Ok(response) => {
                println!("  Announce successful:");
                println!("    Interval: {} seconds", response.interval);
                println!(
                    "    Seeders: {} | Leechers: {}",
                    response.seeders, response.leechers
                );
                println!(
                    "    Complete: {} | Incomplete: {}",
                    response.complete, response.incomplete
                );
                println!("    Downloaded: {}", response.downloaded);
                println!("    Peers available: {}", response.peers.len());

                if let Some(name) = &response.torrent_name {
                    println!("    Torrent name: {}", name);
                }

                if let Some(size) = response.torrent_size {
                    println!("    Size: {:.1} MB", size as f64 / 1_048_576.0);
                }

                // Show some peer examples
                if !response.peers.is_empty() {
                    println!("    Sample peers:");
                    for peer in response.peers.iter().take(3) {
                        println!("      {}", peer);
                    }
                }
            }
            Err(e) => {
                println!("  Announce failed: {}", e);
            }
        }

        // Simulate realistic announce intervals
        tokio::time::sleep(Duration::from_millis(200)).await;
    }

    Ok(())
}

/// Demonstrates offline peer simulation with discovered content.
async fn peer_simulation_with_content(
    tracker: &MockTracker,
) -> Result<(), Box<dyn std::error::Error>> {
    println!("\nPeer Simulation with Discovered Content");
    println!("{:-<50}", "");

    let tracked_torrents = tracker.tracked_torrents();

    if let Some(torrent) = tracked_torrents.first() {
        println!("Simulating peers for: {}", torrent.name);

        // Create realistic peer distribution
        let mut peers = Vec::new();

        // Fast seeders (full content)
        for i in 0..5 {
            let peer = MockPeer::builder()
                .upload_speed(10_000_000) // 10 MB/s
                .reliability(0.98)
                .latency(Duration::from_millis(30))
                .peer_id(format!("SEEDER{:02}", i))
                .build();
            peers.push(peer);
        }

        // Medium leechers (partial content)
        for i in 0..8 {
            let peer = MockPeer::builder()
                .upload_speed(2_000_000) // 2 MB/s
                .reliability(0.85)
                .latency(Duration::from_millis(80))
                .peer_id(format!("LEECHER{:02}", i))
                .build();
            peers.push(peer);
        }

        // Slow peers (dialup simulation)
        for i in 0..2 {
            let peer = MockPeer::builder()
                .upload_speed(100_000) // 100 KB/s
                .reliability(0.70)
                .latency(Duration::from_millis(300))
                .peer_id(format!("SLOW{:02}", i))
                .build();
            peers.push(peer);
        }

        println!("Created peer swarm:");
        println!("  Fast seeders: 5 (10 MB/s each)");
        println!("  Medium leechers: 8 (2 MB/s each)");
        println!("  Slow peers: 2 (100 KB/s each)");

        // Calculate theoretical bandwidth
        let total_upload = 5 * 10_000_000 + 8 * 2_000_000 + 2 * 100_000;
        println!(
            "  Total swarm upload capacity: {:.1} MB/s",
            total_upload as f64 / 1_000_000.0
        );

        // Estimate download time
        let download_time_seconds = torrent.size as f64 / total_upload as f64;
        println!(
            "  Theoretical download time: {:.1} seconds",
            download_time_seconds
        );

        if download_time_seconds < 60.0 {
            println!("  Streaming viability: Excellent (sub-minute download)");
        } else if download_time_seconds < 300.0 {
            println!("  Streaming viability: Good (< 5 minutes)");
        } else {
            println!("  Streaming viability: Poor (> 5 minutes)");
        }
    }

    Ok(())
}

/// Demonstrates failure scenarios and error handling.
async fn failure_simulation() -> Result<(), Box<dyn std::error::Error>> {
    println!("\nFailure Scenario Simulation");
    println!("{:-<50}", "");

    // Test tracker without magneto
    let mut tracker_no_magneto = MockTracker::builder().with_seed(0xFA11ED).build();

    println!("Testing tracker without magneto integration:");
    match tracker_no_magneto
        .search_and_add_torrents("test query", 1)
        .await
    {
        Ok(_) => println!("  Unexpected success"),
        Err(e) => println!("  Expected error: {}", e),
    }

    // Test high failure rate tracker
    let mut unreliable_tracker = MockTracker::builder()
        .with_magneto(true)
        .with_failure_rate(0.8) // 80% failure rate
        .with_seed(0xBAD1234)
        .build();

    println!("\nTesting unreliable tracker (80% failure rate):");
    let info_hash = InfoHash::new([42u8; 20]);

    let mut successes = 0;
    let mut failures = 0;

    for i in 0..10 {
        match unreliable_tracker.announce(info_hash).await {
            Ok(_) => {
                successes += 1;
                println!("  Attempt {}: Success", i + 1);
            }
            Err(_) => {
                failures += 1;
                println!("  Attempt {}: Failed", i + 1);
            }
        }
    }

    println!("  Results: {} successes, {} failures", successes, failures);
    println!(
        "  Actual failure rate: {:.1}%",
        failures as f64 / 10.0 * 100.0
    );

    Ok(())
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("Magneto + Mock Tracker Integration Example");
    println!("=========================================\n");

    // Example 1: Magneto discovery and tracker population
    magneto_discovery_simulation().await?;

    // Example 2: Direct magnet link discovery
    magnet_link_discovery().await?;

    // Example 3: Tracker announce simulation
    let mut populated_tracker = MockTracker::builder()
        .with_magneto(true)
        .with_seeders(15)
        .with_leechers(8)
        .with_seed(0x7EACCE5)
        .build();

    // Populate with some content first
    let _ = populated_tracker
        .search_and_add_torrents("creative commons", 3)
        .await;

    tracker_announce_simulation(&mut populated_tracker).await?;

    // Example 4: Peer simulation with discovered content
    peer_simulation_with_content(&populated_tracker).await?;

    // Example 5: Failure scenarios
    failure_simulation().await?;

    println!("\n{}", "=".repeat(60));
    println!("Magneto Integration Complete");
    println!("\nThis simulation demonstrates:");
    println!("• Integration with magneto for torrent discovery");
    println!("• Mock tracker with realistic peer responses");
    println!("• Offline testing of torrent functionality");
    println!("• Failure scenario simulation and error handling");
    println!("• Peer swarm composition and bandwidth estimation");
    println!("\nBenefits for development:");
    println!("• Test without real trackers or peers");
    println!("• Reproducible scenarios with deterministic seeds");
    println!("• Realistic peer discovery and announce simulation");
    println!("• Integration testing with actual magnet link data");

    Ok(())
}
