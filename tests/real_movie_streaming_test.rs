//! Debug test for real movie streaming issues
//!
//! This test reproduces the exact issue experienced when trying to stream
//! real MKV/AVI files from ~/Downloads/riptide-torrents through the browser.
//!
//! Run with: cargo test real_movie_streaming_debug -- --ignored --nocapture

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use riptide_core::config::RiptideConfig;
use riptide_core::streaming::{FileAssembler, PieceFileAssembler};
use riptide_web::streaming::{
    ClientCapabilities, HttpStreamingConfig, HttpStreamingService, SimpleRangeRequest,
    StreamingRequest,
};
use tracing::{error, info, warn};

/// Test that reproduces the real movie streaming issue
#[tokio::test]
#[ignore] // Use `cargo test real_movie_streaming_debug -- --ignored` to run
async fn real_movie_streaming_debug() {
    // Initialize logging to see what's happening
    let _ = tracing_subscriber::fmt().with_test_writer().try_init();

    info!("=== Starting Real Movie Streaming Debug Test ===");

    // Step 1: Set up components exactly like the real development server
    let movies_dir = PathBuf::from(std::env::var("HOME").unwrap())
        .join("Downloads")
        .join("riptide-torrents");

    if !movies_dir.exists() {
        panic!("Movies directory not found: {}", movies_dir.display());
    }

    info!("=== Creating Development Components (same as real server) ===");
    let config = RiptideConfig::default();
    let components = riptide_sim::create_fast_development_components(config, Some(movies_dir))
        .await
        .expect("Failed to create development components");

    info!("=== Waiting for Background Conversion ===");
    // Wait for background conversion to complete (same as real server)
    tokio::time::sleep(Duration::from_secs(5)).await;

    // Step 2: Get the converted movie info from active sessions
    let engine = components.engine();
    let sessions = engine.active_sessions().await.unwrap();

    if sessions.is_empty() {
        panic!("No active sessions found - background conversion may have failed");
    }

    let session = sessions.iter().next().unwrap();
    let info_hash = session.info_hash;

    info!("=== Movie Details (from converted torrent) ===");
    info!("Title: {}", session.filename);
    info!("Info Hash: {}", info_hash);
    info!(
        "Total Length: {} bytes ({:.2} MB)",
        session.total_size,
        session.total_size as f64 / 1024.0 / 1024.0
    );

    // Step 3: Set up streaming infrastructure (same as real server)
    info!("=== Setting Up Streaming Service ===");
    let piece_store = components.piece_store().unwrap();
    let file_assembler = Arc::new(PieceFileAssembler::new(piece_store.clone(), Some(100)));

    // Step 4: Create HttpStreamingService with same config as real server
    let streaming_service = Arc::new(HttpStreamingService::new(
        file_assembler.clone(),
        piece_store.clone(),
        HttpStreamingConfig::default(),
    ));

    info!("Streaming service created");

    // Step 5: Test file assembler directly first
    info!("=== Testing File Assembler ===");
    match file_assembler.file_size(info_hash).await {
        Ok(size) => info!("File assembler reports size: {} bytes", size),
        Err(e) => error!("File assembler error: {}", e),
    }

    // Test if initial data is available
    let test_range = 0..1024;
    info!(
        "Testing range availability: {}..{}",
        test_range.start, test_range.end
    );
    let is_available = file_assembler.is_range_available(info_hash, test_range.clone());
    info!("Range available: {}", is_available);

    if is_available {
        match file_assembler.read_range(info_hash, test_range).await {
            Ok(data) => info!("Successfully read {} bytes from file assembler", data.len()),
            Err(e) => error!("Failed to read from file assembler: {}", e),
        }
    } else {
        warn!("Initial data not available - this might be the issue!");
    }

    // Step 6: Make streaming request (same as browser does)
    info!("=== Making Streaming Request ===");
    let client_capabilities = ClientCapabilities {
        supports_mp4: true,
        supports_webm: false,
        supports_hls: false,
        user_agent: "Mozilla/5.0 (Test Browser)".to_string(),
    };

    let streaming_request = StreamingRequest {
        info_hash,
        range: None, // Full file request first
        client_capabilities,
        preferred_quality: None,
        time_offset: None,
    };

    info!("Sending streaming request for {}", info_hash);

    // Step 7: Handle streaming request and show detailed results
    let start_time = std::time::Instant::now();
    let result = streaming_service
        .handle_streaming_request(streaming_request)
        .await;
    let elapsed = start_time.elapsed();

    info!(
        "Streaming request completed in {:.2}s",
        elapsed.as_secs_f64()
    );

    match result {
        Ok(response) => {
            info!("=== Streaming Response ===");
            info!("Status: {}", response.status);
            info!("Content-Type: {}", response.content_type);
            info!("Headers:");
            for (key, value) in &response.headers {
                info!("  {}: {:?}", key, value);
            }

            // Check if it's a 503 error (remux in progress)
            if response.status == 503 {
                warn!("Got 503 Service Unavailable - remuxing not ready");
            } else {
                info!("Got response body");
            }
        }
        Err(e) => {
            error!("=== Streaming Request Failed ===");
            error!("Error: {}", e);
        }
    }

    // Step 8: Try range request (like browser does)
    info!("=== Testing Range Request ===");
    let range_request = StreamingRequest {
        info_hash,
        range: Some(SimpleRangeRequest {
            start: 0,
            end: Some(64 * 1024 - 1), // First 64KB
        }),
        client_capabilities: ClientCapabilities {
            supports_mp4: true,
            supports_webm: false,
            supports_hls: false,
            user_agent: "Mozilla/5.0 (Test Browser)".to_string(),
        },
        preferred_quality: None,
        time_offset: None,
    };

    let start_time = std::time::Instant::now();
    let result = streaming_service
        .handle_streaming_request(range_request)
        .await;
    let elapsed = start_time.elapsed();

    info!("Range request completed in {:.2}s", elapsed.as_secs_f64());

    match result {
        Ok(response) => {
            info!("=== Range Response ===");
            info!("Status: {}", response.status);
            info!("Content-Type: {}", response.content_type);
            info!("Headers:");
            for (key, value) in &response.headers {
                info!("  {}: {:?}", key, value);
            }

            if response.status == 206 {
                info!("Successfully got partial content response");
            } else if response.status == 503 {
                warn!("Still getting 503 - remuxing issue confirmed");
            }
        }
        Err(e) => {
            error!("Range request failed: {}", e);
        }
    }

    // Step 9: Wait and retry (simulate browser behavior)
    info!("=== Simulating Browser Retry ===");
    tokio::time::sleep(Duration::from_secs(5)).await;

    let retry_request = StreamingRequest {
        info_hash,
        range: Some(SimpleRangeRequest {
            start: 0,
            end: Some(1023), // First 1KB
        }),
        client_capabilities: ClientCapabilities {
            supports_mp4: true,
            supports_webm: false,
            supports_hls: false,
            user_agent: "Mozilla/5.0 (Test Browser)".to_string(),
        },
        preferred_quality: None,
        time_offset: None,
    };

    let start_time = std::time::Instant::now();
    let result = streaming_service
        .handle_streaming_request(retry_request)
        .await;
    let elapsed = start_time.elapsed();

    info!("Retry request completed in {:.2}s", elapsed.as_secs_f64());

    match result {
        Ok(response) => {
            info!("=== Retry Response ===");
            info!("Status: {}", response.status);
            info!("Content-Type: {}", response.content_type);

            if response.status == 206 {
                info!("SUCCESS: Got partial content on retry!");
            } else {
                warn!("ISSUE: Still not getting successful response");
            }
        }
        Err(e) => {
            error!("Retry request failed: {}", e);
        }
    }

    info!("=== Test Complete ===");
}

/// Test that focuses on the head data availability issue
#[tokio::test]
#[ignore]
async fn test_head_data_availability() {
    let _ = tracing_subscriber::fmt().with_test_writer().try_init();

    info!("=== Testing Head Data Availability ===");

    // This test specifically checks if the head data (first 64KB) is available
    // immediately after conversion, which is required for remuxing

    let movies_dir = PathBuf::from(std::env::var("HOME").unwrap())
        .join("Downloads")
        .join("riptide-torrents");

    if !movies_dir.exists() {
        panic!("Movies directory not found: {}", movies_dir.display());
    }

    // Use exact same setup as development server
    let config = RiptideConfig::default();
    let components = riptide_sim::create_fast_development_components(config, Some(movies_dir))
        .await
        .expect("Failed to create development components");

    // Wait for background conversion
    tokio::time::sleep(Duration::from_secs(5)).await;

    let engine = components.engine();
    let sessions = engine.active_sessions().await.unwrap();

    if sessions.is_empty() {
        panic!("No active sessions found");
    }

    let session = sessions.iter().next().unwrap();
    let info_hash = session.info_hash;
    info!("Testing with: {}", session.filename);

    let piece_store = components.piece_store().unwrap();
    let file_assembler = Arc::new(PieceFileAssembler::new(piece_store.clone(), Some(100)));

    // Test head data availability at different sizes
    let test_sizes = vec![1024, 4096, 16384, 65536, 262144]; // 1KB to 256KB

    for size in test_sizes {
        let range = 0..size;
        let available = file_assembler.is_range_available(info_hash, range.clone());
        info!("Range 0..{} available: {}", size, available);

        if available {
            match file_assembler.read_range(info_hash, range).await {
                Ok(data) => info!("Successfully read {} bytes", data.len()),
                Err(e) => error!("Failed to read {} bytes: {}", size, e),
            }
        } else {
            warn!("Range 0..{} not available - remuxing will fail", size);
        }
    }
}
