//! Debug tests for development mode streaming issues.
//!
//! This module contains tests to debug the integration between the new
//! streaming architecture and the development mode simulation.

use std::sync::Arc;
use std::time::Duration;

use axum::http::Request;
use riptide_core::storage::PieceDataSource;
use riptide_core::streaming::{HttpStreaming, PieceProvider, PieceProviderAdapter};
use riptide_core::torrent::{InfoHash, TorrentPiece};
use riptide_sim::peers::piece_store::InMemoryPieceStore;
use tokio::time::timeout;

/// Calculate SHA-1 hash of data
fn sha1_hash(data: &[u8]) -> [u8; 20] {
    use sha1::{Digest, Sha1};
    let mut hasher = Sha1::new();
    hasher.update(data);
    hasher.finalize().into()
}

/// Test to debug streaming with simulated piece store (like development mode).
#[tokio::test]
async fn test_streaming_with_simulated_piece_store() {
    // Create a simulated piece store like in development mode
    let piece_store = Arc::new(InMemoryPieceStore::new());
    let info_hash = InfoHash::new([1u8; 20]);

    // Load test file data
    let test_file_path = concat!(env!("CARGO_MANIFEST_DIR"), "/../test_movies/test_mkv.mkv");

    let file_bytes = match tokio::fs::read(test_file_path).await {
        Ok(bytes) => bytes,
        Err(_) => {
            eprintln!("Skipping test: Test file not found at {test_file_path}");
            return;
        }
    };

    println!("Test file size: {} bytes", file_bytes.len());

    // Simulate the file being completely downloaded (like in development mode)
    let piece_size = 262144; // 256KB pieces
    let num_pieces = file_bytes.len().div_ceil(piece_size);

    for piece_index in 0..num_pieces {
        let start = piece_index * piece_size;
        let end = std::cmp::min(start + piece_size, file_bytes.len());
        let piece_data = file_bytes[start..end].to_vec();

        // Store the piece
        let hash = sha1_hash(&piece_data);
        let piece = TorrentPiece {
            index: piece_index as u32,
            hash,
            data: piece_data,
        };
        piece_store.add_piece(info_hash, piece).await;
    }

    println!("Stored {num_pieces} pieces in piece store");

    // Create data source and adapter like in production
    let data_source = Arc::new(PieceDataSource::new(piece_store, Some(100)));
    let adapter = Arc::new(PieceProviderAdapter::new(data_source, info_hash));

    // Test basic reads from the adapter
    println!("Testing basic reads from adapter...");
    let total_size = adapter.size().await;
    println!("Adapter reports size: {total_size} bytes");

    // Test reading first chunk
    let first_chunk = adapter.read_at(0, 1024).await.unwrap();
    println!("First chunk size: {} bytes", first_chunk.len());

    // Test reading larger chunk
    let large_chunk = adapter.read_at(0, 100 * 1024).await.unwrap();
    println!("Large chunk size: {} bytes", large_chunk.len());

    // Create streaming session
    let streaming = HttpStreaming::new(adapter).await.unwrap();

    // Test streaming
    println!("Starting streaming test...");
    let request = Request::builder().body(()).unwrap();
    let response = streaming.serve_http_stream(request).await;

    println!("Response status: {}", response.status());
    println!("Response headers: {:?}", response.headers());

    // Try to read some data from the response
    let result = timeout(Duration::from_secs(10), async {
        axum::body::to_bytes(response.into_body(), 10 * 1024 * 1024).await
    })
    .await;

    match result {
        Ok(Ok(bytes)) => {
            println!("Successfully read {} bytes from response", bytes.len());

            // Check if it's valid MP4
            if bytes.len() >= 8 && &bytes[4..8] == b"ftyp" {
                println!("Output appears to be valid MP4");
            } else {
                println!("Output does not appear to be valid MP4");
                println!(
                    "First 32 bytes: {:?}",
                    &bytes[..std::cmp::min(32, bytes.len())]
                );
            }
        }
        Ok(Err(e)) => {
            println!("Error reading response body: {e}");
        }
        Err(_) => {
            println!("Timeout reading response body");
        }
    }
}

/// Test to simulate progressive data availability like in real torrenting.
#[tokio::test]
async fn test_streaming_with_progressive_data() {
    // Create a simulated piece store
    let piece_store = Arc::new(InMemoryPieceStore::new());
    let info_hash = InfoHash::new([2u8; 20]);

    // Load test file data
    let test_file_path = concat!(env!("CARGO_MANIFEST_DIR"), "/../test_movies/test_mkv.mkv");

    let file_bytes = match tokio::fs::read(test_file_path).await {
        Ok(bytes) => bytes,
        Err(_) => {
            eprintln!("Skipping test: Test file not found at {test_file_path}");
            return;
        }
    };

    println!("Test file size: {} bytes", file_bytes.len());

    // Initially store only the first few pieces
    let piece_size = 262144; // 256KB pieces
    let initial_pieces = 5; // Start with 5 pieces (~1.25MB)

    for piece_index in 0..initial_pieces {
        let start = piece_index * piece_size;
        let end = std::cmp::min(start + piece_size, file_bytes.len());
        let piece_data = file_bytes[start..end].to_vec();

        let hash = sha1_hash(&piece_data);
        let piece = TorrentPiece {
            index: piece_index as u32,
            hash,
            data: piece_data,
        };
        piece_store.add_piece(info_hash, piece).await;
    }

    println!("Initially stored {initial_pieces} pieces");

    // Create data source and adapter
    let data_source = Arc::new(PieceDataSource::new(piece_store.clone(), Some(100)));
    let adapter = Arc::new(PieceProviderAdapter::new(data_source, info_hash));

    // Start streaming
    let streaming = HttpStreaming::new(adapter).await.unwrap();
    let request = Request::builder().body(()).unwrap();
    let response = streaming.serve_http_stream(request).await;

    // Spawn a task to gradually add more pieces
    let piece_store_clone = piece_store.clone();
    let file_bytes_clone = file_bytes.clone();
    let add_pieces_task = tokio::spawn(async move {
        let total_pieces = file_bytes_clone.len().div_ceil(piece_size);

        for piece_index in initial_pieces..total_pieces {
            // Wait a bit before adding each piece
            tokio::time::sleep(Duration::from_millis(100)).await;

            let start = piece_index * piece_size;
            let end = std::cmp::min(start + piece_size, file_bytes_clone.len());
            let piece_data = file_bytes_clone[start..end].to_vec();

            let hash = sha1_hash(&piece_data);
            let piece = TorrentPiece {
                index: piece_index as u32,
                hash,
                data: piece_data,
            };
            piece_store_clone.add_piece(info_hash, piece).await;

            if piece_index % 10 == 0 {
                println!("Added piece {piece_index}/{total_pieces}");
            }
        }

        println!("Finished adding all pieces");
    });

    // Try to read from the response
    let result = timeout(Duration::from_secs(30), async {
        axum::body::to_bytes(response.into_body(), 50 * 1024 * 1024).await
    })
    .await;

    // Wait for the piece adding task to complete
    let _ = add_pieces_task.await;

    match result {
        Ok(Ok(bytes)) => {
            println!(
                "Successfully read {} bytes from progressive response",
                bytes.len()
            );

            // Check if it's valid MP4
            if bytes.len() >= 8 && &bytes[4..8] == b"ftyp" {
                println!("Progressive output appears to be valid MP4");
            } else {
                println!("Progressive output does not appear to be valid MP4");
                println!(
                    "First 32 bytes: {:?}",
                    &bytes[..std::cmp::min(32, bytes.len())]
                );
            }
        }
        Ok(Err(e)) => {
            println!("Error reading progressive response body: {e}");
        }
        Err(_) => {
            println!("Timeout reading progressive response body");
        }
    }
}
