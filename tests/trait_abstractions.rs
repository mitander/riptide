//! Integration tests for trait abstractions and swappable implementations

use riptide::config::NetworkConfig;
use riptide::torrent::tracker::AnnounceEvent;
use riptide::torrent::{
    AnnounceRequest, BencodeTorrentParser, BitTorrentPeerProtocol, HttpTrackerClient, InfoHash,
    PeerId, PeerProtocol, TorrentParser, TrackerClient,
};

#[tokio::test]
async fn test_swappable_torrent_parser_implementations() {
    // Test that different parser implementations are interchangeable
    let parsers: Vec<Box<dyn TorrentParser>> = vec![
        Box::new(BencodeTorrentParser::new()),
        // Future implementations can be added here
    ];

    for parser in parsers {
        // All implementations should handle the same interface
        let result = parser.parse_magnet_link("magnet:?xt=urn:btih:test").await;
        // Currently returns error with stub implementation
        assert!(result.is_err());
    }
}

#[tokio::test]
async fn test_swappable_tracker_client_implementations() {
    // Test that different tracker implementations are interchangeable
    let config = NetworkConfig::default();
    let clients: Vec<Box<dyn TrackerClient>> = vec![Box::new(HttpTrackerClient::new(
        "http://tracker.example.com/announce".to_string(),
        &config,
    ))];

    let request = AnnounceRequest {
        info_hash: InfoHash::new([0u8; 20]),
        peer_id: [1u8; 20],
        port: 6881,
        uploaded: 0,
        downloaded: 0,
        left: 1000,
        event: AnnounceEvent::Started,
    };

    for client in clients {
        // All implementations should handle the same interface
        let result = client.announce(request.clone()).await;
        // Currently returns error with stub implementation
        assert!(result.is_err());

        // URL should be accessible
        assert!(!client.tracker_url().is_empty());
    }
}

#[tokio::test]
async fn test_swappable_peer_protocol_implementations() {
    // Test that different peer protocol implementations are interchangeable
    let mut protocols: Vec<Box<dyn PeerProtocol>> = vec![
        Box::new(BitTorrentPeerProtocol::new()),
        // Future implementations can be added here
    ];

    let info_hash = InfoHash::new([0u8; 20]);
    let peer_id = PeerId::generate();
    let handshake = riptide::torrent::protocol::PeerHandshake::new(info_hash, peer_id);

    for protocol in &mut protocols {
        // All implementations should handle the same interface
        let addr = "127.0.0.1:6881".parse().unwrap();
        let result = protocol.connect(addr, handshake.clone()).await;
        // Currently returns error with stub implementation
        assert!(result.is_err());

        // State should be accessible
        let _state = protocol.peer_state();
    }
}

#[test]
fn test_type_safety_across_abstractions() {
    // Test that our domain types work consistently across all abstractions
    let info_hash = InfoHash::new([42u8; 20]);
    let peer_id = PeerId::generate();

    // InfoHash should be usable in all contexts
    let _request = AnnounceRequest {
        info_hash,
        peer_id: *peer_id.as_bytes(),
        port: 6881,
        uploaded: 0,
        downloaded: 0,
        left: 1000,
        event: AnnounceEvent::Started,
    };

    // Domain types should have consistent display behavior
    assert_eq!(info_hash.to_string().len(), 40); // 20 bytes as hex
}

#[test]
fn test_trait_isolation() {
    // Test that trait implementations can be used independently

    // Parser can be used standalone
    let _parser = BencodeTorrentParser::new();

    // Tracker clients can be created independently
    let config = NetworkConfig::default();
    let _http_tracker =
        HttpTrackerClient::new("http://tracker.example.com/announce".to_string(), &config);

    // Peer protocol can be instantiated independently
    let _peer_protocol = BitTorrentPeerProtocol::new();

    // All types are Send + Sync for async contexts
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<BencodeTorrentParser>();
    assert_send_sync::<HttpTrackerClient>();
    assert_send_sync::<BitTorrentPeerProtocol>();
}
