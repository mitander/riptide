//! Tracker management for handling multiple tracker URLs per torrent

use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;

use super::{
    AnnounceRequest, AnnounceResponse, HttpTrackerClient, ScrapeRequest, ScrapeResponse,
    TrackerClient,
};
use crate::config::NetworkConfig;
use crate::torrent::TorrentError;

/// Manages multiple tracker clients for efficient torrent downloading.
///
/// Handles tracker selection, failover, and connection pooling to optimize
/// peer discovery across multiple trackers per torrent.
pub struct Tracker {
    /// Network configuration for tracker clients
    network_config: NetworkConfig,
    /// Cached tracker clients keyed by URL
    tracker_clients: HashMap<String, Arc<HttpTrackerClient>>,
}

impl Tracker {
    /// Creates new tracker manager with network configuration.
    pub fn new(network_config: NetworkConfig) -> Self {
        Self {
            network_config,
            tracker_clients: HashMap::new(),
        }
    }

    /// Announces to the best available tracker from the provided list.
    ///
    /// Tries trackers in order until one succeeds. Caches successful
    /// tracker clients for reuse and implements basic failover logic.
    ///
    /// # Errors
    ///
    /// - `TorrentError::TrackerConnectionFailed` - If all trackers failed
    pub async fn announce_to_trackers(
        &mut self,
        tracker_urls: &[String],
        request: AnnounceRequest,
    ) -> Result<AnnounceResponse, TorrentError> {
        if tracker_urls.is_empty() {
            return Err(TorrentError::TrackerConnectionFailed {
                url: "No tracker URLs provided".to_string(),
            });
        }

        tracing::info!(
            "Announcing to {} trackers for torrent {}",
            tracker_urls.len(),
            hex::encode(&request.info_hash.as_bytes()[..8])
        );

        let mut last_error = None;

        for tracker_url in tracker_urls {
            tracing::info!("Connecting to tracker: {}", tracker_url);

            // Get or create tracker client
            let tracker_client = self.tracker_client_for_url(tracker_url);

            match tracker_client.announce(request.clone()).await {
                Ok(response) => {
                    tracing::info!(
                        "Tracker {} responded with {} peers, complete: {}, incomplete: {}",
                        tracker_url,
                        response.peers.len(),
                        response.complete,
                        response.incomplete
                    );
                    return Ok(response);
                }
                Err(e) => {
                    tracing::warn!("Tracker {} failed: {}", tracker_url, e);
                    last_error = Some(e);
                }
            }
        }

        Err(
            last_error.unwrap_or_else(|| TorrentError::TrackerConnectionFailed {
                url: "All tracker URLs failed".to_string(),
            }),
        )
    }

    /// Scrapes statistics from multiple trackers.
    ///
    /// Attempts to scrape from each tracker and returns the first successful response.
    /// Used for monitoring swarm health across tracker networks.
    ///
    /// # Errors
    ///
    /// - `TorrentError::TrackerConnectionFailed` - If all trackers failed
    pub async fn scrape_from_trackers(
        &mut self,
        tracker_urls: &[String],
        request: ScrapeRequest,
    ) -> Result<ScrapeResponse, TorrentError> {
        if tracker_urls.is_empty() {
            return Err(TorrentError::TrackerConnectionFailed {
                url: "No tracker URLs provided".to_string(),
            });
        }

        let mut last_error = None;

        for tracker_url in tracker_urls {
            let tracker_client = self.tracker_client_for_url(tracker_url);

            match tracker_client.scrape(request.clone()).await {
                Ok(response) => {
                    return Ok(response);
                }
                Err(e) => {
                    eprintln!("Tracker scrape {tracker_url} failed: {e}");
                    last_error = Some(e);
                }
            }
        }

        Err(
            last_error.unwrap_or_else(|| TorrentError::TrackerConnectionFailed {
                url: "All tracker scrape attempts failed".to_string(),
            }),
        )
    }

    /// Gets cached tracker client or creates new one for URL.
    fn tracker_client_for_url(&mut self, tracker_url: &str) -> Arc<HttpTrackerClient> {
        if let Some(client) = self.tracker_clients.get(tracker_url) {
            Arc::clone(client)
        } else {
            let client = Arc::new(HttpTrackerClient::new(
                tracker_url.to_string(),
                &self.network_config,
            ));
            self.tracker_clients
                .insert(tracker_url.to_string(), Arc::clone(&client));
            client
        }
    }

    /// Clears cached tracker clients to free memory.
    pub fn clear_cache(&mut self) {
        self.tracker_clients.clear();
    }

    /// Returns number of cached tracker clients.
    pub fn cached_trackers_count(&self) -> usize {
        self.tracker_clients.len()
    }
}

/// Trait for tracker management abstraction.
///
/// Enables both real and simulated tracker management for testing
/// while maintaining consistent interface for torrent engine.
#[async_trait]
pub trait TrackerManager: Send + Sync {
    /// Announces to best available tracker from list.
    ///
    /// # Errors
    ///
    /// - `TorrentError::TrackerConnectionFailed` - If all trackers failed
    async fn announce_to_trackers(
        &mut self,
        tracker_urls: &[String],
        request: AnnounceRequest,
    ) -> Result<AnnounceResponse, TorrentError>;

    /// Scrapes statistics from trackers.
    ///
    /// # Errors
    ///
    /// - `TorrentError::TrackerConnectionFailed` - If all trackers failed
    async fn scrape_from_trackers(
        &mut self,
        tracker_urls: &[String],
        request: ScrapeRequest,
    ) -> Result<ScrapeResponse, TorrentError>;
}

#[async_trait]
impl TrackerManager for Tracker {
    async fn announce_to_trackers(
        &mut self,
        tracker_urls: &[String],
        request: AnnounceRequest,
    ) -> Result<AnnounceResponse, TorrentError> {
        self.announce_to_trackers(tracker_urls, request).await
    }

    async fn scrape_from_trackers(
        &mut self,
        tracker_urls: &[String],
        request: ScrapeRequest,
    ) -> Result<ScrapeResponse, TorrentError> {
        self.scrape_from_trackers(tracker_urls, request).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::RiptideConfig;
    use crate::torrent::test_data::create_test_info_hash;
    use crate::torrent::tracker::AnnounceEvent;

    #[tokio::test]
    async fn test_tracker_creation() {
        let config = RiptideConfig::default();
        let manager = Tracker::new(config.network);

        assert_eq!(manager.cached_trackers_count(), 0);
    }

    #[tokio::test]
    async fn test_empty_tracker_urls() {
        let config = RiptideConfig::default();
        let mut manager = Tracker::new(config.network);
        let info_hash = create_test_info_hash();

        let request = AnnounceRequest {
            info_hash,
            peer_id: [0; 20],
            port: 6881,
            uploaded: 0,
            downloaded: 0,
            left: 1000,
            event: AnnounceEvent::Started,
        };

        let result = manager.announce_to_trackers(&[], request).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_tracker_failover() {
        // This test requires a real network connection to a non-existent port
        // to simulate a connection failure.
        let config = RiptideConfig::default();
        let mut manager = Tracker::new(config.network);
        let info_hash = create_test_info_hash();

        let trackers = vec![
            "http://127.0.0.1:1234/announce".to_string(), // Guaranteed to fail
            "http://tracker.example.com/announce".to_string(), // Should not be called
        ];

        let request = AnnounceRequest {
            info_hash,
            peer_id: [0; 20],
            port: 6881,
            uploaded: 0,
            downloaded: 0,
            left: 1000,
            event: AnnounceEvent::Started,
        };

        // We can't easily test the success case without a real tracker.
        // The goal here is to ensure it tries the first, fails, and then we can
        // infer it would try the next. The error should be from the last attempt.
        let result = manager.announce_to_trackers(&trackers, request).await;
        assert!(result.is_err());
    }

    #[test]
    fn test_cache_management() {
        let config = RiptideConfig::default();
        let mut manager = Tracker::new(config.network);

        // Simulate creating tracker clients
        let _client1 = manager.tracker_client_for_url("http://tracker1.example.com");
        let _client2 = manager.tracker_client_for_url("http://tracker2.example.com");

        assert_eq!(manager.cached_trackers_count(), 2);

        manager.clear_cache();
        assert_eq!(manager.cached_trackers_count(), 0);
    }
}
