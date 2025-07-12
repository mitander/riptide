//! BitTorrent protocol core functionality test
//!
//! Tests fundamental BitTorrent protocol mechanics: piece downloading,
//! hash validation, peer communication, and data integrity.
//! These tests validate the core protocol implementation.

use std::net::SocketAddr;
use std::sync::Arc;

use riptide_core::storage::FileStorage;
use riptide_core::torrent::downloader::PieceDownloader;
use riptide_core::torrent::parsing::types::{TorrentFile, TorrentMetadata};
use riptide_core::torrent::{InfoHash, PeerId, PieceIndex, PieceStore, TorrentPiece};
use riptide_sim::{DeterministicConfig, DeterministicPeers, InMemoryPieceStore};
use sha1::{Digest, Sha1};
use tokio::sync::RwLock;

#[tokio::test]
async fn test_simple_piece_download() {
    println!("SIMPLE_TEST: Starting simple piece download test");

    // Create test torrent metadata with proper hashes
    let info_hash = InfoHash::new([1u8; 20]);
    let piece_size = 32_768u32;
    let piece_count = 2;
    let total_size = piece_count as u64 * piece_size as u64;

    // Calculate proper SHA1 hashes for piece data
    let piece_hashes = (0..piece_count)
        .map(|i| {
            let mut hasher = Sha1::new();
            hasher.update(vec![i as u8; piece_size as usize]);
            let result = hasher.finalize();
            let mut hash = [0u8; 20];
            hash.copy_from_slice(&result);
            hash
        })
        .collect();

    let metadata = TorrentMetadata {
        info_hash,
        name: "test.bin".to_string(),
        total_length: total_size,
        piece_length: piece_size,
        piece_hashes,
        files: vec![TorrentFile {
            path: vec!["test.bin".to_string()],
            length: total_size,
        }],
        announce_urls: vec!["http://test.tracker/announce".to_string()],
    };

    println!("SIMPLE_TEST: Created metadata for {piece_count} pieces");

    // Create and populate piece store
    let piece_store = Arc::new(InMemoryPieceStore::new());

    // Create test piece data that matches the expected hashes
    let piece_0_data = vec![0x00; piece_size as usize];
    let piece_1_data = vec![0x01; piece_size as usize];

    println!("SIMPLE_TEST: Adding pieces to store");
    let pieces = vec![
        TorrentPiece {
            index: 0,
            hash: metadata.piece_hashes[0],
            data: piece_0_data.clone(),
        },
        TorrentPiece {
            index: 1,
            hash: metadata.piece_hashes[1],
            data: piece_1_data.clone(),
        },
    ];
    piece_store.add_torrent_pieces(info_hash, pieces).await;

    // Verify pieces were added
    let stored_piece_0 = piece_store
        .piece_data(info_hash, PieceIndex::new(0))
        .await
        .unwrap();
    assert_eq!(stored_piece_0.len(), piece_size as usize);
    println!(
        "SIMPLE_TEST: Verified piece 0 stored ({} bytes)",
        stored_piece_0.len()
    );

    // Create peer manager
    let config = DeterministicConfig::default();
    let mut peers = DeterministicPeers::new(config, piece_store.clone());

    println!("SIMPLE_TEST: Created peer manager");

    // Create test peer addresses
    let peer_addr: SocketAddr = "127.0.0.1:8080".parse().unwrap();

    // Inject single peer with all pieces available
    let available_pieces = vec![true; piece_count as usize];
    peers
        .inject_peer_with_pieces(
            peer_addr,
            info_hash,
            available_pieces,
            100_000, // 100KB/s upload rate
        )
        .await;

    println!("SIMPLE_TEST: Injected single peer {peer_addr} with all pieces");

    // Create temporary storage
    let temp_dir = tempfile::tempdir().unwrap();
    let storage = FileStorage::new(
        temp_dir.path().join("downloads"),
        temp_dir.path().join("library"),
    );

    println!("SIMPLE_TEST: Created storage at {:?}", temp_dir.path());

    // Create piece downloader
    let peer_id = PeerId::generate();
    let peers_arc = Arc::new(RwLock::new(peers));
    let mut piece_downloader = PieceDownloader::new(metadata, storage, peers_arc, peer_id).unwrap();

    println!("SIMPLE_TEST: Created piece downloader");

    // Update downloader with single peer address
    piece_downloader.update_peers(vec![peer_addr]).await;

    println!("SIMPLE_TEST: Updated downloader with single peer address");

    // Try to download piece 0
    println!("SIMPLE_TEST: Attempting to download piece 0");
    let result = piece_downloader.download_piece(PieceIndex::new(0)).await;

    match result {
        Ok(()) => {
            println!("SIMPLE_TEST: Successfully downloaded piece 0!");

            // Verify piece was completed
            let progress = piece_downloader.progress().await;
            let piece_0_progress = &progress[0];
            println!("SIMPLE_TEST: Piece 0 progress: {piece_0_progress:?}");

            assert!(matches!(
                piece_0_progress.status,
                riptide_core::torrent::downloader::PieceStatus::Complete
            ));
            println!("SIMPLE_TEST: Piece 0 marked as complete");

            // Try to download piece 1
            println!("SIMPLE_TEST: Attempting to download piece 1");
            let result = piece_downloader.download_piece(PieceIndex::new(1)).await;

            match result {
                Ok(()) => {
                    println!("SIMPLE_TEST: Successfully downloaded piece 1!");

                    // Check final progress
                    let final_progress = piece_downloader.progress().await;
                    let completed_count = final_progress
                        .iter()
                        .filter(|p| {
                            matches!(
                                p.status,
                                riptide_core::torrent::downloader::PieceStatus::Complete
                            )
                        })
                        .count();

                    println!("SIMPLE_TEST: Completed pieces: {completed_count}/{piece_count}");
                    assert_eq!(completed_count, piece_count as usize);

                    println!("SIMPLE_TEST: Test completed successfully!");
                }
                Err(e) => {
                    println!("SIMPLE_TEST: Failed to download piece 1: {e:?}");
                    panic!("Piece 1 download failed: {e:?}");
                }
            }
        }
        Err(e) => {
            println!("SIMPLE_TEST: Failed to download piece 0: {e:?}");
            panic!("Piece 0 download failed: {e:?}");
        }
    }
}

#[tokio::test]
async fn test_peers_basic_functionality() {
    println!("BASIC_TEST: Testing peer manager basic functionality");

    let info_hash = InfoHash::new([1u8; 20]);
    let piece_store = Arc::new(InMemoryPieceStore::new());

    // Create test piece data with proper hash
    let test_data = vec![0x00; 1024];
    let mut hasher = Sha1::new();
    hasher.update(&test_data);
    let result = hasher.finalize();
    let mut hash = [0u8; 20];
    hash.copy_from_slice(&result);

    let pieces = vec![TorrentPiece {
        index: 0,
        hash,
        data: test_data.clone(),
    }];
    piece_store.add_torrent_pieces(info_hash, pieces).await;

    let config = DeterministicConfig::default();
    let mut peers = DeterministicPeers::new(config, piece_store.clone());

    let peer_addr: SocketAddr = "127.0.0.1:9090".parse().unwrap();

    // Test peer injection
    peers
        .inject_peer_with_pieces(
            peer_addr,
            info_hash,
            vec![true], // Has piece 0
            50_000,
        )
        .await;

    println!("BASIC_TEST: Injected peer");

    // Test connection
    let peer_id = PeerId::generate();
    let result = peers.connect_peer(peer_addr, info_hash, peer_id).await;

    match result {
        Ok(()) => println!("BASIC_TEST: Successfully connected to peer"),
        Err(e) => println!("BASIC_TEST: Failed to connect to peer: {e:?}"),
    }

    // Test piece availability
    let has_piece = peers.peer_has_piece(peer_addr, PieceIndex::new(0)).await;
    println!("BASIC_TEST: Peer has piece 0: {has_piece}");
    assert!(has_piece);

    println!("BASIC_TEST: Basic functionality test completed");
}
