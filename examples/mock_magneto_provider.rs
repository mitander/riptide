//! Mock magneto provider demonstration
//!
//! Shows how to use the custom magneto provider for completely offline
//! and deterministic torrent discovery simulation.

use std::time::Duration;

use riptide::simulation::{
    MockMagnetoProviderBuilder, MockTracker, TorrentEntryParams, create_mock_magneto_client,
    create_streaming_test_client,
};

/// Demonstrates basic mock magneto provider functionality.
async fn basic_provider_demo() -> Result<(), Box<dyn std::error::Error>> {
    println!("Basic Mock Magneto Provider Demo");
    println!("{:-<50}", "");

    // Create mock magneto client with deterministic provider
    let provider = MockMagnetoProviderBuilder::new()
        .with_seed(0x12345)
        .with_response_delay(Duration::from_millis(50))
        .build();

    let mut client = create_mock_magneto_client(provider);

    // Test various searches
    let test_queries = [
        "big buck bunny",
        "sintel",
        "creative commons",
        "linux",
        "ubuntu",
        "games",
        "blender",
    ];

    for query in &test_queries {
        println!("\nSearching for: '{query}'");

        let request = magneto::SearchRequest::new(query);
        match client.search(request).await {
            Ok(results) => {
                println!("  Found {} results:", results.len());
                for (i, torrent) in results.iter().enumerate() {
                    println!(
                        "    {}. {} ({:.1} MB, {} seeders, {} leechers)",
                        i + 1,
                        torrent.name,
                        torrent.size_bytes as f64 / 1_048_576.0,
                        torrent.seeders,
                        torrent.peers
                    );
                    println!("       Magnet: {}...", &torrent.magnet_link[..80]);
                }
            }
            Err(e) => {
                println!("  Search failed: {e}");
            }
        }
    }

    Ok(())
}

/// Demonstrates deterministic behavior across multiple runs.
async fn deterministic_behavior_demo() -> Result<(), Box<dyn std::error::Error>> {
    println!("\nDeterministic Behavior Demo");
    println!("{:-<50}", "");

    let seed = 0xDEADBEEF;

    // Run same search with same seed twice
    for run in 1..=2 {
        println!("\nRun {run}: Searching with seed 0x{seed:X}");

        let provider = MockMagnetoProviderBuilder::new().with_seed(seed).build();
        let mut client = create_mock_magneto_client(provider);

        let request = magneto::SearchRequest::new("creative commons");
        let results = client.search(request).await?;

        println!("  Results for run {run}:");
        for torrent in &results {
            println!(
                "    - {} ({} seeders, {} leechers)",
                torrent.name, torrent.seeders, torrent.peers
            );
        }
    }

    println!("\nNote: Both runs should produce identical results!");

    Ok(())
}

/// Demonstrates custom content creation for specialized testing.
async fn custom_content_demo() -> Result<(), Box<dyn std::error::Error>> {
    println!("\nCustom Content Demo");
    println!("{:-<50}", "");

    // Create provider with custom content for testing
    let provider = MockMagnetoProviderBuilder::new()
        .with_seed(0xC057044)
        .add_torrent(
            "test_movies".to_string(),
            TorrentEntryParams {
                name: "Test Movie 2024 [4K] [HDR] [5.1]".to_string(),
                size_bytes: 15_000_000_000, // 15GB
                seeders: 234,
                leechers: 67,
            },
        )
        .add_torrent(
            "test_movies".to_string(),
            TorrentEntryParams {
                name: "Sample Series Complete Season 1 [1080p]".to_string(),
                size_bytes: 25_000_000_000, // 25GB
                seeders: 456,
                leechers: 123,
            },
        )
        .add_torrent(
            "test_software".to_string(),
            TorrentEntryParams {
                name: "Custom Development Tools Suite v2.0".to_string(),
                size_bytes: 8_500_000_000, // 8.5GB
                seeders: 89,
                leechers: 23,
            },
        )
        .build();

    let mut client = create_mock_magneto_client(provider);

    // Search for custom content
    let categories = ["test_movies", "test_software"];

    for category in &categories {
        println!("\nSearching category: '{category}'");

        let request = magneto::SearchRequest::new(category);
        let results = client.search(request).await?;

        for torrent in &results {
            println!(
                "  - {} ({:.1} GB, {} seeders)",
                torrent.name,
                torrent.size_bytes as f64 / 1_073_741_824.0,
                torrent.seeders
            );
        }
    }

    Ok(())
}

/// Demonstrates failure simulation for error handling testing.
async fn failure_simulation_demo() -> Result<(), Box<dyn std::error::Error>> {
    println!("\nFailure Simulation Demo");
    println!("{:-<50}", "");

    // Create provider with high failure rate
    let provider = MockMagnetoProviderBuilder::new()
        .with_seed(0xFA11FE)
        .with_failure_rate(0.7) // 70% failure rate
        .build();

    let mut client = create_mock_magneto_client(provider);

    println!("Testing with 70% failure rate:");

    let mut successes = 0;
    let mut failures = 0;

    for i in 1..=10 {
        let request = magneto::SearchRequest::new("test");
        match client.search(request).await {
            Ok(results) => {
                successes += 1;
                println!("  Attempt {}: Success ({} results)", i, results.len());
            }
            Err(_) => {
                failures += 1;
                println!("  Attempt {}: Failed", i);
            }
        }
    }

    println!(
        "\nResults: {} successes, {} failures ({:.1}% failure rate)",
        successes,
        failures,
        failures as f64 / 10.0 * 100.0
    );

    Ok(())
}

/// Demonstrates integration with mock tracker for complete offline testing.
async fn integrated_tracker_demo() -> Result<(), Box<dyn std::error::Error>> {
    println!("\nIntegrated Tracker Demo");
    println!("{:-<50}", "");

    // Create tracker with deterministic magneto provider
    let mut tracker = MockTracker::builder()
        .with_magneto(true) // Uses our custom provider
        .with_seeders(50)
        .with_leechers(15)
        .build();

    println!("Created tracker with mock magneto provider");

    // Populate tracker using magneto searches
    let search_queries = ["big buck bunny", "creative commons", "linux"];

    for query in &search_queries {
        println!("\nPopulating tracker with: '{}'", query);

        match tracker.search_and_add_torrents(query, 2).await {
            Ok(torrents) => {
                println!("  Added {} torrents to tracker:", torrents.len());
                for torrent in &torrents {
                    println!(
                        "    - {} ({:.1} MB)",
                        torrent.name,
                        torrent.size as f64 / 1_048_576.0
                    );
                }
            }
            Err(e) => {
                println!("  Failed to populate: {}", e);
            }
        }
    }

    // Show tracker statistics
    let tracked_torrents = tracker.tracked_torrents();
    println!("\nTracker Statistics:");
    println!("  Total torrents: {}", tracked_torrents.len());

    let total_size: u64 = tracked_torrents.iter().map(|t| t.size).sum();
    println!(
        "  Total content: {:.1} GB",
        total_size as f64 / 1_073_741_824.0
    );

    // Test announces
    if let Some(torrent) = tracked_torrents.first() {
        println!("\nTesting announce for: {}", torrent.name);

        match tracker.announce(torrent.info_hash).await {
            Ok(response) => {
                println!("  Announce successful:");
                println!(
                    "    Seeders: {}, Leechers: {}",
                    response.seeders, response.leechers
                );
                println!("    Peers available: {}", response.peers.len());
            }
            Err(e) => {
                println!("  Announce failed: {}", e);
            }
        }
    }

    Ok(())
}

/// Demonstrates streaming-optimized client for media testing.
async fn streaming_client_demo() -> Result<(), Box<dyn std::error::Error>> {
    println!("\nStreaming Client Demo");
    println!("{:-<50}", "");

    // Create client optimized for streaming content testing
    let mut client = create_streaming_test_client(0x57EAAAAA);

    println!("Created streaming-optimized magneto client");

    let request = magneto::SearchRequest::new("streaming_test");
    let results = client.search(request).await?;

    println!("\nStreaming Test Content:");
    for (i, torrent) in results.iter().enumerate() {
        println!(
            "  {}. {} ({:.1} GB, {} seeders)",
            i + 1,
            torrent.name,
            torrent.size_bytes as f64 / 1_073_741_824.0,
            torrent.seeders
        );

        // Analyze streaming viability
        let bitrate_estimate = (torrent.size_bytes * 8) as f64 / (2.0 * 3600.0 * 1_000_000.0); // Assume 2hr, convert to Mbps
        println!("     Estimated bitrate: {:.1} Mbps", bitrate_estimate);

        if bitrate_estimate < 5.0 {
            println!("     Streaming: Excellent quality");
        } else if bitrate_estimate < 15.0 {
            println!("     Streaming: Good quality");
        } else {
            println!("     Streaming: High quality (requires fast connection)");
        }
    }

    Ok(())
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("Mock Magneto Provider Demonstration");
    println!("==================================\n");

    // Example 1: Basic functionality
    basic_provider_demo().await?;

    // Example 2: Deterministic behavior
    deterministic_behavior_demo().await?;

    // Example 3: Custom content
    custom_content_demo().await?;

    // Example 4: Failure simulation
    failure_simulation_demo().await?;

    // Example 5: Integration with tracker
    integrated_tracker_demo().await?;

    // Example 6: Streaming-optimized client
    streaming_client_demo().await?;

    println!("\n{}", "=".repeat(60));
    println!("Mock Magneto Provider Demo Complete");
    println!("\nKey Benefits:");
    println!("• Complete offline operation - no network dependencies");
    println!("• Deterministic results - same seed produces same results");
    println!("• Customizable content - add specific torrents for testing");
    println!("• Realistic data - proper torrent metadata and magnet links");
    println!("• Failure simulation - test error handling and resilience");
    println!("• Streaming optimization - specialized content for media testing");
    println!("\nUse cases:");
    println!("• Unit and integration testing");
    println!("• CI/CD pipelines without network access");
    println!("• Debugging and development");
    println!("• Performance testing with consistent data");
    println!("• Educational and demonstration purposes");

    Ok(())
}
