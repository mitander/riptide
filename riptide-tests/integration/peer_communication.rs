//! Minimal test to isolate message passing issues in SimPeers

#![allow(clippy::uninlined_format_args)]

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use riptide_core::torrent::{InfoHash, PeerId, PeerMessage, Peers, PieceIndex, TorrentPiece};
use riptide_sim::{InMemoryPeerConfig, InMemoryPieceStore, SimPeers};
use tokio::sync::RwLock;

#[tokio::test]
async fn test_basic_message_send_receive() {
    println!("MSG_TEST: Starting basic message send/receive test");

    let info_hash = InfoHash::new([1u8; 20]);
    let piece_store = Arc::new(InMemoryPieceStore::new());

    // Add a piece to the store
    let test_data = vec![0xAA; 1024];
    let pieces = vec![TorrentPiece {
        index: 0,
        hash: [1u8; 20],
        data: test_data.clone(),
    }];
    piece_store
        .add_torrent_pieces(info_hash, pieces)
        .await
        .unwrap();
    println!("MSG_TEST: Added piece to store");

    // Create peer manager
    let config = InMemoryPeerConfig::default();
    let mut peers = SimPeers::new(config, piece_store.clone());

    let peer_addr: SocketAddr = "127.0.0.1:8080".parse().unwrap();
    let peer_id = PeerId::generate();

    // Inject peer with piece available
    peers
        .inject_peer_with_pieces(peer_addr, info_hash, vec![true], 100_000)
        .await;
    println!("MSG_TEST: Injected peer {}", peer_addr);

    // Connect to peer
    println!("MSG_TEST: Connecting to peer...");
    peers
        .connect_peer(peer_addr, info_hash, peer_id)
        .await
        .unwrap();
    println!("MSG_TEST: Connected to peer");

    // Send a piece request
    let request = PeerMessage::Request {
        piece_index: PieceIndex::new(0),
        offset: 0,
        length: 512,
    };
    println!("MSG_TEST: Sending request: {:?}", request);
    peers.send_message(peer_addr, request).await.unwrap();
    println!("MSG_TEST: Request sent");

    // Try to receive response with timeout
    println!("MSG_TEST: Waiting for response...");
    let timeout_result = tokio::time::timeout(Duration::from_secs(5), async {
        let mut attempt = 0;
        loop {
            attempt += 1;
            println!("MSG_TEST: Receive attempt {}", attempt);

            match peers.receive_message().await {
                Ok(msg_event) => {
                    println!(
                        "MSG_TEST: Received message from {}: {:?}",
                        msg_event.peer_address, msg_event.message
                    );

                    match msg_event.message {
                        PeerMessage::Piece {
                            piece_index,
                            offset,
                            data,
                        } => {
                            println!(
                                "MSG_TEST: Got piece response: piece={}, offset={}, size={}",
                                piece_index,
                                offset,
                                data.len()
                            );
                            return Ok(data.len());
                        }
                        PeerMessage::Bitfield { .. } => {
                            println!("MSG_TEST: Got bitfield, continuing...");
                            continue;
                        }
                        _ => {
                            println!("MSG_TEST: Got other message, continuing...");
                            continue;
                        }
                    }
                }
                Err(e) => {
                    println!("MSG_TEST: Error receiving message: {}", e);
                    return Err(e);
                }
            }
        }
    })
    .await;

    match timeout_result {
        Ok(Ok(data_size)) => {
            println!(
                "MSG_TEST: SUCCESS - Received piece data of {} bytes",
                data_size
            );
            assert_eq!(data_size, 512);
        }
        Ok(Err(e)) => {
            panic!("MSG_TEST: FAILED - Error receiving message: {:?}", e);
        }
        Err(_) => {
            panic!("MSG_TEST: FAILED - Timeout waiting for piece response");
        }
    }

    println!("MSG_TEST: Test completed successfully");
}

#[tokio::test]
async fn test_message_queue_behavior() {
    println!("QUEUE_TEST: Testing message queue behavior");

    let info_hash = InfoHash::new([2u8; 20]);
    let piece_store = Arc::new(InMemoryPieceStore::new());

    // Add pieces to store
    let pieces = vec![
        TorrentPiece {
            index: 0,
            hash: [1u8; 20],
            data: vec![0xAA; 512],
        },
        TorrentPiece {
            index: 1,
            hash: [2u8; 20],
            data: vec![0xBB; 512],
        },
    ];
    piece_store
        .add_torrent_pieces(info_hash, pieces)
        .await
        .unwrap();

    let config = InMemoryPeerConfig::default();
    let mut peers = SimPeers::new(config, piece_store);

    let peer1: SocketAddr = "127.0.0.1:8081".parse().unwrap();
    let peer2: SocketAddr = "127.0.0.1:8082".parse().unwrap();
    let peer_id = PeerId::generate();

    // Inject two peers
    peers
        .inject_peer_with_pieces(peer1, info_hash, vec![true, false], 100_000)
        .await;
    peers
        .inject_peer_with_pieces(peer2, info_hash, vec![false, true], 100_000)
        .await;

    // Connect to both peers
    peers.connect_peer(peer1, info_hash, peer_id).await.unwrap();
    peers.connect_peer(peer2, info_hash, peer_id).await.unwrap();

    println!("QUEUE_TEST: Connected to both peers");

    // Send requests to both peers simultaneously
    let request1 = PeerMessage::Request {
        piece_index: PieceIndex::new(0),
        offset: 0,
        length: 256,
    };
    let request2 = PeerMessage::Request {
        piece_index: PieceIndex::new(1),
        offset: 0,
        length: 256,
    };

    println!("QUEUE_TEST: Sending request for piece 0 to peer1");
    peers.send_message(peer1, request1).await.unwrap();

    println!("QUEUE_TEST: Sending request for piece 1 to peer2");
    peers.send_message(peer2, request2).await.unwrap();

    // Collect all messages within timeout
    let mut received_messages = Vec::new();
    let timeout_result = tokio::time::timeout(Duration::from_secs(3), async {
        while received_messages.len() < 4 {
            // Expect 2 bitfields + 2 piece responses
            match peers.receive_message().await {
                Ok(msg_event) => {
                    println!(
                        "QUEUE_TEST: Received from {}: {:?}",
                        msg_event.peer_address, msg_event.message
                    );
                    received_messages.push(msg_event);
                }
                Err(e) => {
                    println!("QUEUE_TEST: Error: {}", e);
                    break;
                }
            }
        }
    })
    .await;

    if timeout_result.is_err() {
        println!(
            "QUEUE_TEST: Timeout - received {} messages",
            received_messages.len()
        );
    }

    // Analyze received messages
    let piece_responses = received_messages
        .iter()
        .filter(|msg| matches!(msg.message, PeerMessage::Piece { .. }))
        .count();

    println!(
        "QUEUE_TEST: Received {} piece responses out of {} total messages",
        piece_responses,
        received_messages.len()
    );

    // For this test, we just want to see what messages we get
    assert!(
        !received_messages.is_empty(),
        "Should receive at least some messages"
    );

    println!("QUEUE_TEST: Test completed");
}

#[tokio::test]
async fn test_peers_in_isolation() {
    println!("ISO_TEST: Testing peer manager in complete isolation");

    let info_hash = InfoHash::new([3u8; 20]);
    let piece_store = Arc::new(InMemoryPieceStore::new());
    let config = InMemoryPeerConfig::default();

    // Use Arc<RwLock> wrapper like PieceDownloader does
    let peers = Arc::new(RwLock::new(SimPeers::new(config, piece_store.clone())));

    let peer_addr: SocketAddr = "127.0.0.1:9999".parse().unwrap();

    // Step 1: Inject peer through the Arc<RwLock> wrapper
    {
        let mut pm = peers.write().await;
        pm.inject_peer_with_pieces(peer_addr, info_hash, vec![true], 50_000)
            .await;
    }
    println!("ISO_TEST: Injected peer through Arc<RwLock>");

    // Step 2: Connect through the wrapper
    {
        let mut pm = peers.write().await;
        let result = pm
            .connect_peer(peer_addr, info_hash, PeerId::generate())
            .await;
        println!("ISO_TEST: Connect result: {:?}", result);
    }

    // Step 3: Send message through wrapper
    {
        let mut pm = peers.write().await;
        let request = PeerMessage::Interested;
        let result = pm.send_message(peer_addr, request).await;
        println!("ISO_TEST: Send result: {:?}", result);
    }

    // Step 4: Try to receive
    println!("ISO_TEST: Attempting to receive message...");
    let receive_result = tokio::time::timeout(Duration::from_secs(2), async {
        let mut pm = peers.write().await;
        pm.receive_message().await
    })
    .await;

    match receive_result {
        Ok(Ok(msg)) => println!("ISO_TEST: Received: {:?}", msg.message),
        Ok(Err(e)) => println!("ISO_TEST: Receive error: {}", e),
        Err(_) => println!("ISO_TEST: Receive timeout"),
    }

    println!("ISO_TEST: Test completed");
}
