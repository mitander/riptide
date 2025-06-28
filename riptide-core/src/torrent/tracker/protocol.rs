//! TrackerClient trait implementation for HTTP tracker communication

use async_trait::async_trait;

use super::client::HttpTrackerClient;
use super::types::{
    AnnounceRequest, AnnounceResponse, ScrapeRequest, ScrapeResponse, TrackerClient,
};
use crate::torrent::TorrentError;

#[async_trait]
impl TrackerClient for HttpTrackerClient {
    async fn announce(&self, request: AnnounceRequest) -> Result<AnnounceResponse, TorrentError> {
        let url = self.build_announce_url(&request)?;

        let response = self.client.get(&url).send().await.map_err(|e| {
            TorrentError::TrackerConnectionFailed {
                url: format!("HTTP request failed: {e}"),
            }
        })?;

        if !response.status().is_success() {
            return Err(TorrentError::TrackerConnectionFailed {
                url: format!("HTTP error {}: {}", response.status(), self.announce_url),
            });
        }

        let body = response
            .bytes()
            .await
            .map_err(|e| TorrentError::TrackerConnectionFailed {
                url: format!("Failed to read response body: {e}"),
            })?;

        self.parse_announce_response(&body)
    }

    async fn scrape(&self, request: ScrapeRequest) -> Result<ScrapeResponse, TorrentError> {
        let url = self.build_scrape_url(&request)?;

        let response = self.client.get(&url).send().await.map_err(|e| {
            TorrentError::TrackerConnectionFailed {
                url: format!("HTTP scrape request failed: {e}"),
            }
        })?;

        if !response.status().is_success() {
            return Err(TorrentError::TrackerConnectionFailed {
                url: format!(
                    "HTTP scrape error {}: {}",
                    response.status(),
                    self.announce_url
                ),
            });
        }

        let body = response
            .bytes()
            .await
            .map_err(|e| TorrentError::TrackerConnectionFailed {
                url: format!("Failed to read scrape response: {e}"),
            })?;

        self.parse_scrape_response(&body)
    }

    fn tracker_url(&self) -> &str {
        &self.announce_url
    }
}

#[cfg(test)]
mod tracker_protocol_tests {
    use std::time::Duration;

    use super::*;
    use crate::config::NetworkConfig;
    use crate::torrent::tracker::AnnounceEvent;
    use crate::torrent::{InfoHash, PeerId};

    // Helper to create a test network config
    fn create_test_network_config() -> NetworkConfig {
        NetworkConfig {
            tracker_timeout: Duration::from_secs(5),
            min_announce_interval: Duration::from_secs(60),
            default_announce_interval: Duration::from_secs(300),
            user_agent: "Riptide/test",
            max_peer_connections: 25,
            download_limit: None,
            upload_limit: None,
            peer_timeout: Duration::from_secs(60),
        }
    }

    #[tokio::test]
    async fn test_announce_url_construction() {
        let config = create_test_network_config();
        let client =
            HttpTrackerClient::new("http://test.tracker:8080/announce".to_string(), &config);

        let info_hash = InfoHash::new([0x11; 20]);
        let peer_id = PeerId::new([0x22; 20]);
        let request = AnnounceRequest {
            info_hash,
            peer_id: *peer_id.as_bytes(),
            port: 6881,
            uploaded: 1000,
            downloaded: 500,
            left: 2000,
            event: AnnounceEvent::Started,
        };

        // Currently returns error since we don't have a real tracker
        let result = client.announce(request).await;
        assert!(result.is_err());

        // Verify the URL is constructed correctly
        assert_eq!(client.tracker_url(), "http://test.tracker:8080/announce");
    }

    #[test]
    fn test_tracker_url_access() {
        let config = create_test_network_config();
        let client = HttpTrackerClient::new("http://example.com/announce".to_string(), &config);

        assert_eq!(client.tracker_url(), "http://example.com/announce");
    }

    #[test]
    fn test_announce_request_structure() {
        let info_hash = InfoHash::new([0xAB; 20]);
        let peer_id = PeerId::new([0xCD; 20]);

        let request = AnnounceRequest {
            info_hash,
            peer_id: *peer_id.as_bytes(),
            port: 8080,
            uploaded: 12345,
            downloaded: 67890,
            left: 54321,
            event: AnnounceEvent::Started,
        };

        assert_eq!(request.port, 8080);
        assert_eq!(request.uploaded, 12345);
        assert_eq!(request.downloaded, 67890);
        assert_eq!(request.left, 54321);
        assert_eq!(request.event, AnnounceEvent::Started);
    }

    #[test]
    fn test_announce_event_types() {
        let events = vec![
            AnnounceEvent::Started,
            AnnounceEvent::Stopped,
            AnnounceEvent::Completed,
        ];

        for event in events {
            let info_hash = InfoHash::new([0xEE; 20]);
            let peer_id = PeerId::new([0xFF; 20]);

            let request = AnnounceRequest {
                info_hash,
                peer_id: *peer_id.as_bytes(),
                port: 6881,
                uploaded: 0,
                downloaded: 0,
                left: 1000000,
                event,
            };

            assert_eq!(request.event, event);
        }
    }

    #[test]
    fn test_client_creation() {
        let config = create_test_network_config();
        let announce_url = "https://tracker.example.com:443/announce".to_string();
        let client = HttpTrackerClient::new(announce_url.clone(), &config);

        assert_eq!(client.tracker_url(), announce_url);
    }
}
