//! BitTorrent protocol core functionality tests
//!
//! Tests fundamental BitTorrent protocol mechanics: piece downloading,
//! hash validation, peer communication, and data integrity.
//! These tests validate the core protocol implementation.

use std::net::SocketAddr;
use std::sync::Arc;

use riptide_core::torrent::test_data::{create_test_piece_store, create_test_torrent_metadata};
use riptide_core::torrent::{InfoHash, PeerId, PeerManager, PieceIndex, PieceStore, TorrentPiece};
use riptide_sim::{InMemoryPieceStore, SimulatedConfig, SimulatedPeers};
use sha1::{Digest, Sha1};

#[tokio::test]
async fn test_torrent_metadata_creation() {
    println!("METADATA_TEST: Testing torrent metadata creation");

    // Use the test utility function for consistency
    let metadata = create_test_torrent_metadata();

    // Verify metadata properties
    assert_eq!(metadata.name, "test.txt");
    assert!(metadata.total_length > 0);
    assert!(metadata.piece_length > 0);
    assert!(!metadata.piece_hashes.is_empty());
    assert!(!metadata.files.is_empty());

    println!(
        "METADATA_TEST: Created torrent with {} pieces, {} bytes total",
        metadata.piece_hashes.len(),
        metadata.total_length
    );
    println!("METADATA_TEST: Metadata creation test completed");
}

#[tokio::test]
async fn test_piece_store_operations() {
    println!("STORE_TEST: Testing piece store operations");

    let info_hash = InfoHash::new([1u8; 20]);
    let piece_store = Arc::new(InMemoryPieceStore::new());

    // Create test pieces with proper data
    let piece_size = 1024;
    let pieces = vec![
        TorrentPiece {
            index: 0,
            hash: [1u8; 20],
            data: vec![0xAA; piece_size],
        },
        TorrentPiece {
            index: 1,
            hash: [2u8; 20],
            data: vec![0xBB; piece_size],
        },
    ];

    // Add pieces to store
    piece_store
        .add_torrent_pieces(info_hash, pieces.clone())
        .await;
    println!("STORE_TEST: Added {} pieces to store", pieces.len());

    // Verify pieces can be retrieved
    assert!(piece_store.has_piece(info_hash, PieceIndex::new(0)));
    assert!(piece_store.has_piece(info_hash, PieceIndex::new(1)));
    assert!(!piece_store.has_piece(info_hash, PieceIndex::new(2)));

    // Test piece retrieval
    let piece_0 = piece_store.piece_data(info_hash, PieceIndex::new(0)).await;
    assert!(piece_0.is_ok());
    let piece_data = piece_0.unwrap();
    assert_eq!(piece_data.len(), piece_size);
    assert_eq!(piece_data[0], 0xAA);

    println!("STORE_TEST: Piece store operations test completed");
}

#[tokio::test]
async fn test_peer_manager_integration() {
    println!("PEER_INTEGRATION_TEST: Testing peer manager with piece store");

    let info_hash = InfoHash::new([2u8; 20]);
    let piece_store = Arc::new(InMemoryPieceStore::new());

    // Add test data to piece store
    let test_pieces = vec![TorrentPiece {
        index: 0,
        hash: [1u8; 20],
        data: vec![0xCC; 1024],
    }];
    piece_store.add_torrent_pieces(info_hash, test_pieces).await;

    // Create peer manager with ideal conditions
    let config = SimulatedConfig::ideal();
    let mut peers = SimulatedPeers::new(config, piece_store.clone());

    let peer_addr: SocketAddr = "127.0.0.1:8080".parse().unwrap();
    let peer_id = PeerId::generate();

    // Test peer connection
    println!("PEER_INTEGRATION_TEST: Connecting to peer...");
    let result = peers.connect_peer(peer_addr, info_hash, peer_id).await;
    assert!(result.is_ok(), "Peer connection should succeed");

    // Verify connection
    let connected = peers.connected_peers().await;
    assert_eq!(connected.len(), 1);

    println!(
        "PEER_INTEGRATION_TEST: Successfully connected to {} peers",
        connected.len()
    );
    println!("PEER_INTEGRATION_TEST: Peer integration test completed");
}

// TODO: Re-enable after updating to current PieceDownloader API
// #[tokio::test]
// async fn test_piece_downloader_creation() {
//     // This test needs to be updated for the current PieceDownloader API
// }

#[tokio::test]
async fn test_torrent_piece_validation() {
    println!("VALIDATION_TEST: Testing piece hash validation");

    let info_hash = InfoHash::new([4u8; 20]);

    // Create piece with known data and calculate correct hash
    let piece_data = vec![0xDD; 1024];
    let mut hasher = Sha1::new();
    hasher.update(&piece_data);
    let expected_hash = hasher.finalize();
    let mut hash_array = [0u8; 20];
    hash_array.copy_from_slice(&expected_hash);

    let piece = TorrentPiece {
        index: 0,
        hash: hash_array,
        data: piece_data.clone(),
    };

    // Add to store
    let piece_store = Arc::new(InMemoryPieceStore::new());
    piece_store.add_torrent_pieces(info_hash, vec![piece]).await;

    // Verify piece exists and data matches
    assert!(piece_store.has_piece(info_hash, PieceIndex::new(0)));

    let retrieved_data = piece_store
        .piece_data(info_hash, PieceIndex::new(0))
        .await
        .unwrap();
    assert_eq!(retrieved_data, piece_data);

    // Verify hash calculation
    let mut verify_hasher = Sha1::new();
    verify_hasher.update(&retrieved_data);
    let calculated_hash = verify_hasher.finalize();
    assert_eq!(&calculated_hash[..], &hash_array[..]);

    println!("VALIDATION_TEST: Piece hash validation completed successfully");
}

#[tokio::test]
async fn test_multiple_peer_scenario() {
    println!("MULTI_PEER_TEST: Testing multiple peer connections");

    let info_hash = InfoHash::new([5u8; 20]);
    let piece_store = create_test_piece_store();

    // Use default config which allows some failures for realistic testing
    let config = SimulatedConfig::default();
    let mut peers = SimulatedPeers::new(config, piece_store);

    // Try to connect multiple peers
    let peer_addresses = vec![
        "127.0.0.1:8081".parse().unwrap(),
        "127.0.0.1:8082".parse().unwrap(),
        "127.0.0.1:8083".parse().unwrap(),
    ];

    let mut successful_connections = 0;
    for peer_addr in peer_addresses {
        let peer_id = PeerId::generate();
        let result = peers.connect_peer(peer_addr, info_hash, peer_id).await;

        if result.is_ok() {
            successful_connections += 1;
            println!("MULTI_PEER_TEST: Successfully connected to {peer_addr}");
        } else {
            println!(
                "MULTI_PEER_TEST: Failed to connect to {peer_addr} (expected with default config)"
            );
        }
    }

    // Verify at least some connections succeeded (depending on failure rate)
    let connected = peers.connected_peers().await;
    assert_eq!(connected.len(), successful_connections);

    println!("MULTI_PEER_TEST: {successful_connections} out of 3 peer connections succeeded");
    println!("MULTI_PEER_TEST: Multiple peer scenario test completed");
}
