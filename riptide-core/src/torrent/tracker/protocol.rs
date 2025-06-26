//! TrackerClient trait implementation for HTTP tracker communication

use async_trait::async_trait;

use super::super::TorrentError;
use super::client::HttpTrackerClient;
use super::types::{AnnounceRequest, AnnounceResponse, ScrapeRequest, ScrapeResponse, TrackerClient};

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