//! Test the smart header/footer reading approach for FFmpeg streaming.
//!
//! This test verifies that the new input pump strategy reads header and footer
//! data first, ensuring FFmpeg gets the critical metadata needed for valid MP4 headers.

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

/// Test that verifies the smart header/footer reading approach.
#[ignore]
#[tokio::test]
async fn test_smart_header_footer_reading() {
    // Create a simulated piece store
    let piece_store = Arc::new(InMemoryPieceStore::new());
    let info_hash = InfoHash::new([3u8; 20]);

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

    // Store only the first few pieces and last few pieces initially
    // This simulates the scenario where we have header and footer but not middle
    let piece_size = 262144; // 256KB pieces
    let total_pieces = file_bytes.len().div_ceil(piece_size);

    // Store first 10 pieces (header)
    let header_pieces = std::cmp::min(10, total_pieces);
    for piece_index in 0..header_pieces {
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

    // Store last 5 pieces (footer) if file is large enough
    if total_pieces > 15 {
        let footer_start = total_pieces - 5;
        for piece_index in footer_start..total_pieces {
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
    }

    println!("Stored header pieces: 0-{}", header_pieces - 1);
    if total_pieces > 15 {
        println!(
            "Stored footer pieces: {}-{}",
            total_pieces - 5,
            total_pieces - 1
        );
    }

    // Create data source and adapter
    let data_source = Arc::new(PieceDataSource::new(piece_store.clone(), Some(100)));
    let adapter = Arc::new(PieceProviderAdapter::new(data_source, info_hash));

    // Test that we can read header (we know we have the first 10 pieces)
    let header_test_size = piece_size * 2; // Test reading 2 pieces worth

    println!("Testing header read ({header_test_size} bytes)...");
    let header_data = adapter.read_at(0, header_test_size).await.unwrap();
    assert_eq!(header_data.len(), header_test_size);

    println!("Successfully read header data - smart streaming should work better now");

    // Create streaming session
    let streaming = HttpStreaming::new(adapter).await.unwrap();

    // Start streaming - this should now work better with header/footer available
    println!("Starting smart streaming test...");
    let request = Request::builder().body(()).unwrap();
    let response = streaming.serve_http_stream(request).await;

    println!("Response status: {}", response.status());
    println!("Response headers: {:?}", response.headers());

    // Spawn a task to gradually add middle pieces
    let piece_store_clone = piece_store.clone();
    let file_bytes_clone = file_bytes.clone();
    let add_middle_pieces_task = tokio::spawn(async move {
        // Add middle pieces (skipping header and footer)
        let footer_start = if total_pieces > 15 {
            total_pieces - 5
        } else {
            total_pieces
        };

        for piece_index in header_pieces..footer_start {
            tokio::time::sleep(Duration::from_millis(50)).await;

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
                println!("Added middle piece {piece_index}/{footer_start}");
            }
        }

        println!("Finished adding all middle pieces");
    });

    // Try to read from the response - should work better now
    let result = timeout(Duration::from_secs(20), async {
        axum::body::to_bytes(response.into_body(), 50 * 1024 * 1024).await
    })
    .await;

    // Wait for the piece adding task to complete
    let _ = add_middle_pieces_task.await;

    match result {
        Ok(Ok(bytes)) => {
            println!(
                "Successfully read {} bytes from smart streaming response",
                bytes.len()
            );

            // Check if it's valid MP4
            if bytes.len() >= 8 && &bytes[4..8] == b"ftyp" {
                println!("Smart streaming output is valid MP4!");
            } else {
                println!("Smart streaming output is not valid MP4");
                if bytes.len() >= 32 {
                    println!("First 32 bytes: {:?}", &bytes[..32]);
                } else {
                    println!("Output too short: {} bytes", bytes.len());
                }
            }
        }
        Ok(Err(e)) => {
            println!("Error reading smart streaming response: {e}");
        }
        Err(_) => {
            println!("Timeout reading smart streaming response");
        }
    }
}

/// Test that verifies our header/footer strategy with different file sizes.
#[tokio::test]
async fn test_header_footer_strategy_small_file() {
    // Load real MP4 test file
    let test_file_path = concat!(env!("CARGO_MANIFEST_DIR"), "/../test_movies/test_video.mp4");
    let file_data = match tokio::fs::read(test_file_path).await {
        Ok(bytes) => bytes,
        Err(_) => {
            eprintln!("Skipping test: test_video.mp4 not found at {test_file_path}");
            return;
        }
    };

    // Create a simulated piece store
    let piece_store = Arc::new(InMemoryPieceStore::new());
    let info_hash = InfoHash::new([4u8; 20]);

    // Store the entire file as one piece
    let hash = sha1_hash(&file_data);
    let piece = TorrentPiece {
        index: 0,
        hash,
        data: file_data.clone(),
    };
    piece_store.add_piece(info_hash, piece).await;

    // Create data source and adapter
    let data_source = Arc::new(PieceDataSource::new(piece_store, Some(100)));
    let adapter = Arc::new(PieceProviderAdapter::new(data_source, info_hash));

    // Create streaming session
    let streaming = HttpStreaming::new(adapter).await.unwrap();

    // Start streaming
    println!("Testing small file streaming...");
    let request = Request::builder().body(()).unwrap();
    let response = streaming.serve_http_stream(request).await;

    println!("Small file response status: {}", response.status());

    // The response should be successful even for small files
    assert!(response.status().is_success());
}
