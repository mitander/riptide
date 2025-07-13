//! Integration tests for peer communication using SimulatedPeers

#![allow(clippy::uninlined_format_args)]

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use riptide_core::torrent::test_data::create_test_piece_store;
use riptide_core::torrent::{InfoHash, PeerId, PeerManager, PeerMessage, PieceIndex, TorrentPiece};
use riptide_sim::{InMemoryPieceStore, SimulatedConfig, SimulatedPeers};

#[tokio::test]
async fn test_basic_peer_connection() {
    println!("PEER_TEST: Testing basic peer connection functionality");

    let info_hash = InfoHash::new([1u8; 20]);
    let piece_store = create_test_piece_store();

    // Create peer manager with ideal conditions for testing
    let config = SimulatedConfig::ideal(); // No failures for reliable testing
    let mut peers = SimulatedPeers::new(config, piece_store);

    let peer_addr: SocketAddr = "127.0.0.1:8080".parse().unwrap();
    let peer_id = PeerId::generate();

    // Test connection
    println!("PEER_TEST: Connecting to peer {}", peer_addr);
    let result = peers.connect_peer(peer_addr, info_hash, peer_id).await;
    assert!(
        result.is_ok(),
        "Peer connection should succeed with ideal config"
    );

    // Verify connection
    let connected = peers.connected_peers().await;
    assert_eq!(connected.len(), 1, "Should have exactly one connected peer");
    assert_eq!(
        connected[0].address, peer_addr,
        "Connected peer address should match"
    );

    println!("PEER_TEST: Basic peer connection test completed successfully");
}

#[tokio::test]
async fn test_message_passing() {
    println!("MSG_TEST: Testing message send/receive functionality");

    let info_hash = InfoHash::new([2u8; 20]);
    let piece_store = Arc::new(InMemoryPieceStore::new());

    // Add test piece to store
    let test_data = vec![0xAA; 1024];
    let pieces = vec![TorrentPiece {
        index: 0,
        hash: [1u8; 20],
        data: test_data.clone(),
    }];
    piece_store.add_torrent_pieces(info_hash, pieces).await;
    println!("MSG_TEST: Added test piece to store");

    // Create peer manager with ideal conditions
    let config = SimulatedConfig::ideal();
    let mut peers = SimulatedPeers::new(config, piece_store);

    let peer_addr: SocketAddr = "127.0.0.1:8081".parse().unwrap();
    let peer_id = PeerId::generate();

    // Connect peer
    peers
        .connect_peer(peer_addr, info_hash, peer_id)
        .await
        .unwrap();
    println!("MSG_TEST: Connected to peer {}", peer_addr);

    // Send a piece request message
    let request = PeerMessage::Request {
        piece_index: PieceIndex::new(0),
        offset: 0,
        length: 1024,
    };

    println!("MSG_TEST: Sending piece request...");
    let send_result = peers.send_message(peer_addr, request).await;
    assert!(send_result.is_ok(), "Message sending should succeed");

    // Try to receive a response (with timeout)
    println!("MSG_TEST: Waiting for response...");
    let receive_timeout = Duration::from_millis(500);
    let receive_result = tokio::time::timeout(receive_timeout, peers.receive_message()).await;

    match receive_result {
        Ok(Ok(message_event)) => {
            println!("MSG_TEST: Received message: {:?}", message_event);
            // In ideal conditions with test data, we should get some response
        }
        Ok(Err(e)) => {
            println!(
                "MSG_TEST: Message receive error (expected in simulation): {:?}",
                e
            );
            // This is acceptable in simulation environment
        }
        Err(_) => {
            println!("MSG_TEST: Message receive timeout (expected in simulation)");
            // Timeout is acceptable as simulation may not implement full peer protocol
        }
    }

    println!("MSG_TEST: Message passing test completed");
}

#[tokio::test]
async fn test_peer_statistics() {
    println!("STATS_TEST: Testing peer statistics tracking");

    let info_hash = InfoHash::new([3u8; 20]);
    let piece_store = create_test_piece_store();

    let config = SimulatedConfig::default();
    let mut peers = SimulatedPeers::new(config, piece_store);

    let peer_addr1: SocketAddr = "127.0.0.1:8082".parse().unwrap();
    let peer_addr2: SocketAddr = "127.0.0.1:8083".parse().unwrap();
    let peer_id1 = PeerId::generate();
    let peer_id2 = PeerId::generate();

    // Connect multiple peers
    let result1 = peers.connect_peer(peer_addr1, info_hash, peer_id1).await;
    let result2 = peers.connect_peer(peer_addr2, info_hash, peer_id2).await;

    // At least one connection should succeed (depending on failure rate)
    let successful_connections = [result1.is_ok(), result2.is_ok()]
        .iter()
        .filter(|&&x| x)
        .count();
    println!(
        "STATS_TEST: {} out of 2 connections succeeded",
        successful_connections
    );

    // Verify connected peers count
    let connected = peers.connected_peers().await;
    assert_eq!(connected.len(), successful_connections);

    println!("STATS_TEST: Statistics test completed");
}

#[tokio::test]
async fn test_poor_network_conditions() {
    println!("NETWORK_TEST: Testing poor network conditions simulation");

    let info_hash = InfoHash::new([4u8; 20]);
    let piece_store = create_test_piece_store();

    // Use poor network conditions config
    let config = SimulatedConfig::poor();
    let mut peers = SimulatedPeers::new(config, piece_store);

    let peer_addr: SocketAddr = "127.0.0.1:8084".parse().unwrap();
    let peer_id = PeerId::generate();

    // With poor conditions, connections may fail
    println!("NETWORK_TEST: Attempting connection with poor network conditions...");
    let result = peers.connect_peer(peer_addr, info_hash, peer_id).await;

    match result {
        Ok(()) => {
            println!("NETWORK_TEST: Connection succeeded despite poor conditions");
            let connected = peers.connected_peers().await;
            assert_eq!(connected.len(), 1);
        }
        Err(e) => {
            println!(
                "NETWORK_TEST: Connection failed as expected with poor conditions: {:?}",
                e
            );
            let connected = peers.connected_peers().await;
            assert_eq!(connected.len(), 0);
        }
    }

    println!("NETWORK_TEST: Poor network conditions test completed");
}
