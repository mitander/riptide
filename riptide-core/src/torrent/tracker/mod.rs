//! BitTorrent tracker communication abstractions and implementations.
//!
//! HTTP tracker client following BEP 3 with announce/scrape operations.
//! Supports automatic URL encoding, compact peer list parsing, and error handling.

pub mod client;
pub mod protocol;
pub mod types;

// Re-export public API
pub use client::HttpTrackerClient;
pub use types::{
    AnnounceEvent, AnnounceRequest, AnnounceResponse, ScrapeRequest, ScrapeResponse, ScrapeStats,
    TrackerClient,
};

#[cfg(test)]
mod tests {
    use super::client::HttpTrackerClient;
    use super::types::{AnnounceEvent, AnnounceRequest, ScrapeRequest, ScrapeStats, TrackerClient};
    use super::super::{InfoHash, TorrentError};
    use crate::config::NetworkConfig;

    #[test]
    fn test_announce_request_structure() {
        let request = AnnounceRequest {
            info_hash: InfoHash::new([0u8; 20]),
            peer_id: [1u8; 20],
            port: 6881,
            uploaded: 0,
            downloaded: 1024,
            left: 8192,
            event: AnnounceEvent::Started,
        };

        assert_eq!(request.port, 6881);
        assert_eq!(request.event, AnnounceEvent::Started);
    }

    #[tokio::test]
    async fn test_http_tracker_interface() {
        let config = NetworkConfig::default();
        let tracker =
            HttpTrackerClient::new("http://tracker.example.com/announce".to_string(), &config);

        assert_eq!(tracker.tracker_url(), "http://tracker.example.com/announce");

        let request = AnnounceRequest {
            info_hash: InfoHash::new([0u8; 20]),
            peer_id: [1u8; 20],
            port: 6881,
            uploaded: 0,
            downloaded: 0,
            left: 1000,
            event: AnnounceEvent::Started,
        };

        // Should fail with stub implementation
        let result = tracker.announce(request).await;
        assert!(result.is_err());
    }

    #[test]
    fn test_scrape_url_derivation() {
        let config = NetworkConfig::default();
        let tracker =
            HttpTrackerClient::new("http://tracker.example.com/announce".to_string(), &config);

        // Scrape URL should be derived correctly
        assert!(tracker.scrape_url.is_some());
        assert_eq!(
            tracker.scrape_url.unwrap(),
            "http://tracker.example.com/scrape"
        );
    }

    #[test]
    fn test_url_encode_bytes() {
        let test_bytes = [0x12, 0x34, 0xAB, 0xCD];
        let encoded = HttpTrackerClient::url_encode_bytes(&test_bytes);
        assert_eq!(encoded, "%12%34%AB%CD");
    }

    #[test]
    fn test_event_to_string() {
        assert_eq!(
            HttpTrackerClient::event_to_string(AnnounceEvent::Started),
            "started"
        );
        assert_eq!(
            HttpTrackerClient::event_to_string(AnnounceEvent::Stopped),
            "stopped"
        );
        assert_eq!(
            HttpTrackerClient::event_to_string(AnnounceEvent::Completed),
            "completed"
        );
    }

    #[test]
    fn test_parse_compact_peers() {
        // Test valid compact peer data (2 peers)
        let peer_data = [
            192, 168, 1, 100, 0x1A, 0xE1, // 192.168.1.100:6881
            10, 0, 0, 1, 0x1A, 0xE2, // 10.0.0.1:6882
        ];

        let peers = HttpTrackerClient::parse_compact_peers(&peer_data).unwrap();
        assert_eq!(peers.len(), 2);

        assert_eq!(peers[0].to_string(), "192.168.1.100:6881");
        assert_eq!(peers[1].to_string(), "10.0.0.1:6882");
    }

    #[test]
    fn test_parse_compact_peers_invalid_length() {
        // Invalid length (not multiple of 6)
        let invalid_data = [192, 168, 1, 100, 0x1A];
        let result = HttpTrackerClient::parse_compact_peers(&invalid_data);
        assert!(result.is_err());
    }

    #[test]
    fn test_announce_url_building() {
        let config = NetworkConfig::default();
        let tracker =
            HttpTrackerClient::new("http://tracker.example.com/announce".to_string(), &config);

        let request = AnnounceRequest {
            info_hash: InfoHash::new([0u8; 20]),
            peer_id: [1u8; 20],
            port: 6881,
            uploaded: 1024,
            downloaded: 2048,
            left: 4096,
            event: AnnounceEvent::Started,
        };

        let url = tracker.build_announce_url(&request).unwrap();

        // Verify URL contains required parameters
        assert!(url.contains("info_hash="));
        assert!(url.contains("peer_id="));
        assert!(url.contains("port=6881"));
        assert!(url.contains("uploaded=1024"));
        assert!(url.contains("downloaded=2048"));
        assert!(url.contains("left=4096"));
        assert!(url.contains("event=started"));
        assert!(url.contains("compact=1"));
    }

    #[test]
    fn test_tracker_response_parsing() {
        let config = NetworkConfig::default();
        let tracker = HttpTrackerClient::new("http://test.com/announce".to_string(), &config);

        // Create a mock bencode tracker response
        let response_data =
            b"d8:intervali1800e5:peers12:\xC0\xA8\x01\x64\x1A\xE1\xC0\xA8\x01\x65\x1A\xE2e";

        let response = tracker.parse_announce_response(response_data).unwrap();
        assert_eq!(response.interval, 1800);
        assert_eq!(response.peers.len(), 2);
    }

    #[test]
    fn test_scrape_request_creation() {
        let info_hash1 = InfoHash::new([1u8; 20]);
        let info_hash2 = InfoHash::new([2u8; 20]);

        let request = ScrapeRequest {
            info_hashes: vec![info_hash1, info_hash2],
        };

        assert_eq!(request.info_hashes.len(), 2);
        assert_eq!(request.info_hashes[0], info_hash1);
        assert_eq!(request.info_hashes[1], info_hash2);
    }

    #[test]
    fn test_scrape_stats_structure() {
        let stats = ScrapeStats {
            complete: 50,
            downloaded: 1000,
            incomplete: 25,
        };

        assert_eq!(stats.complete, 50);
        assert_eq!(stats.downloaded, 1000);
        assert_eq!(stats.incomplete, 25);
    }

    #[test]
    fn test_scrape_response_parsing() {
        let config = NetworkConfig::default();
        let tracker = HttpTrackerClient::new("http://test.com/announce".to_string(), &config);

        // Create mock scrape response with 20-byte info hash
        let info_hash_bytes = [
            0x12, 0x34, 0x56, 0x78, 0x9A, 0xBC, 0xDE, 0xF0, 0x12, 0x34, 0x56, 0x78, 0x9A, 0xBC,
            0xDE, 0xF0, 0x12, 0x34, 0x56, 0x78,
        ];

        // Build bencode scrape response: d5:filesd20:<hash>d8:completei50e10:downloadedi1000e10:incompletei25eeee
        let mut response_data = Vec::new();
        response_data.extend_from_slice(b"d5:filesd20:");
        response_data.extend_from_slice(&info_hash_bytes);
        response_data.extend_from_slice(b"d8:completei50e10:downloadedi1000e10:incompletei25eeee");

        let response = tracker.parse_scrape_response(&response_data).unwrap();

        let info_hash = InfoHash::new(info_hash_bytes);
        let stats = response.files.get(&info_hash).unwrap();

        assert_eq!(stats.complete, 50);
        assert_eq!(stats.downloaded, 1000);
        assert_eq!(stats.incomplete, 25);
    }

    #[test]
    fn test_scrape_response_parsing_empty_files() {
        let config = NetworkConfig::default();
        let tracker = HttpTrackerClient::new("http://test.com/announce".to_string(), &config);

        // Empty files dictionary
        let response_data = b"d5:filesd\x65e";

        let response = tracker.parse_scrape_response(response_data).unwrap();
        assert!(response.files.is_empty());
    }

    #[test]
    fn test_scrape_response_parsing_failure() {
        let config = NetworkConfig::default();
        let tracker = HttpTrackerClient::new("http://test.com/announce".to_string(), &config);

        // Response with failure reason
        let response_data = b"d14:failure reason13:Access deniede";

        let result = tracker.parse_scrape_response(response_data);
        assert!(result.is_err());

        if let Err(TorrentError::TrackerConnectionFailed { url }) = result {
            assert!(url.contains("Access denied"));
        } else {
            panic!("Expected TrackerConnectionFailed error");
        }
    }

    #[test]
    fn test_scrape_response_invalid_hash_length() {
        let config = NetworkConfig::default();
        let tracker = HttpTrackerClient::new("http://test.com/announce".to_string(), &config);

        // Hash that's not 20 bytes
        let response_data = b"d5:filesd10:short_hashd8:completei10eeee";

        let response = tracker.parse_scrape_response(response_data).unwrap();
        assert!(response.files.is_empty()); // Invalid hash lengths are ignored
    }

    #[test]
    fn test_tracker_response_with_failure() {
        let config = NetworkConfig::default();
        let tracker = HttpTrackerClient::new("http://test.com/announce".to_string(), &config);

        // Create a bencode response with failure reason
        let response_data = b"d14:failure reason19:Torrent not registerede";

        let result = tracker.parse_announce_response(response_data);
        assert!(result.is_err());
        match result {
            Err(TorrentError::TrackerConnectionFailed { url }) => {
                assert!(url.contains("Failed to parse tracker response"));
            }
            Err(other) => panic!("Unexpected error type: {other:?}"),
            Ok(_) => panic!("Expected error"),
        }
    }
}