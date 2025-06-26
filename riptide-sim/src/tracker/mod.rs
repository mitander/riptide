//! Mock BitTorrent tracker for simulation with magneto integration

mod builder;
mod mock;
mod types;

pub use builder::MockTrackerBuilder;
pub use mock::MockTracker;
pub use types::{AnnounceResponse, MockTorrentData, TrackerError};

#[cfg(test)]
mod tests {
    use std::time::SystemTime;

    use riptide_core::torrent::InfoHash;

    use super::*;

    #[tokio::test]
    async fn test_mock_tracker_successful_announce() {
        let mut tracker = MockTracker::builder()
            .with_seeders(5)
            .with_leechers(3)
            .with_seed(123)
            .build();

        let info_hash = InfoHash::new([0u8; 20]);

        // Add torrent data for this info hash
        let torrent_data = MockTorrentData {
            info_hash,
            name: "Test Torrent".to_string(),
            size: 1_000_000_000,
            seeders: 5,
            leechers: 3,
            last_announce: SystemTime::now(),
        };
        tracker.add_torrent(torrent_data);

        let response = tracker.announce(info_hash).await.unwrap();

        assert!(response.seeders >= 5); // May have dynamic variation
        assert!(response.leechers >= 3); // May have dynamic variation
        assert_eq!(response.interval, 1800);
        assert!(!response.peers.is_empty());
    }

    #[tokio::test]
    async fn test_mock_tracker_failure_injection() {
        let mut tracker = MockTracker::builder()
            .with_failure_rate(1.0) // Always fail
            .with_seed(456)
            .build();

        let info_hash = InfoHash::new([0u8; 20]);
        let result = tracker.announce(info_hash).await;

        assert!(result.is_err());
        matches!(result.unwrap_err(), TrackerError::ConnectionFailed);
    }

    #[test]
    fn test_tracker_builder_generates_valid_peers() {
        let tracker = MockTracker::builder()
            .with_seeders(2)
            .with_leechers(1)
            .with_seed(789)
            .build();

        assert_eq!(tracker.peers.len(), 3);

        // Verify all peers have valid socket addresses
        for peer in &tracker.peers {
            assert!(peer.ip().is_ipv4());
            assert!(peer.port() >= 6881);
        }
    }

    #[tokio::test]
    async fn test_mock_tracker_with_torrent_data() {
        let mut tracker = MockTracker::builder().with_seed(101112).build();

        // Add a mock torrent
        let info_hash = InfoHash::new([1u8; 20]);
        let torrent_data = MockTorrentData {
            info_hash,
            name: "Test Movie 2024".to_string(),
            size: 2_000_000_000, // 2GB
            seeders: 15,
            leechers: 8,
            last_announce: SystemTime::now(),
        };

        tracker.add_torrent(torrent_data);

        // Test announce for known torrent
        let response = tracker.announce(info_hash).await.unwrap();
        assert_eq!(response.torrent_name, Some("Test Movie 2024".to_string()));
        assert_eq!(response.torrent_size, Some(2_000_000_000));
        assert!(response.seeders >= 15); // May have dynamic variation
    }

    #[test]
    fn test_tracker_builder_with_magneto() {
        let tracker = MockTracker::builder()
            .with_magneto(true)
            .with_seed(131415)
            .build();

        assert!(tracker.magneto_client.is_some());
    }
}
