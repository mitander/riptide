//! Simulation network layer for deterministic testing

use std::collections::HashMap;
use std::time::Duration;

use async_trait::async_trait;

use super::{HttpResponse, NetworkLayer};
use crate::torrent::TorrentError;

/// Simulated network layer for testing
///
/// Provides deterministic responses for HTTP requests without real network calls.
/// Useful for testing, simulation, and development environments.
pub struct SimulationNetworkLayer {
    timeout: Duration,
    responses: HashMap<String, HttpResponse>,
    failure_rate: f32,
    #[allow(dead_code)]
    request_count: usize,
}

impl SimulationNetworkLayer {
    /// Creates new simulation network layer with default settings
    pub fn new() -> Self {
        Self {
            timeout: Duration::from_secs(5),
            responses: HashMap::new(),
            failure_rate: 0.0,
            request_count: 0,
        }
    }

    /// Add a predefined response for a URL
    pub fn add_response(&mut self, url: &str, response: HttpResponse) {
        self.responses.insert(url.to_string(), response);
    }

    /// Configure failure rate (0.0 = never fail, 1.0 = always fail)
    pub fn configure_failure_rate(&mut self, rate: f32) {
        self.failure_rate = rate.clamp(0.0, 1.0);
    }

    /// Add common tracker responses for testing
    pub fn add_tracker_responses(&mut self) {
        // Successful tracker response with peers
        let peer_response = self.create_tracker_response_with_peers();
        self.add_response("http://tracker.example.com/announce", peer_response);

        // Empty tracker response
        let empty_response = self.create_empty_tracker_response();
        self.add_response("http://empty.tracker.com/announce", empty_response);
    }

    fn create_tracker_response_with_peers(&self) -> HttpResponse {
        // Bencode response: d8:intervali1800e5:peers6:<6 bytes of peer data>e
        // Peer data: 127.0.0.1:6881 (127,0,0,1,26,225)
        let bencode = b"d8:intervali1800e5:peers6:\x7f\x00\x00\x01\x1a\xe1e";
        HttpResponse::new(200, bencode.to_vec())
    }

    fn create_empty_tracker_response(&self) -> HttpResponse {
        // Bencode response with no peers
        let bencode = b"d8:intervali1800e5:peers0:e";
        HttpResponse::new(200, bencode.to_vec())
    }

    #[allow(dead_code)]
    fn should_fail(&mut self) -> bool {
        self.request_count += 1;
        let fail = rand::random::<f32>() < self.failure_rate;
        if fail {
            tracing::debug!(
                "Simulating network failure for request {}",
                self.request_count
            );
        }
        fail
    }
}

impl Default for SimulationNetworkLayer {
    fn default() -> Self {
        let mut layer = Self::new();
        layer.add_tracker_responses();
        layer
    }
}

#[async_trait]
impl NetworkLayer for SimulationNetworkLayer {
    async fn http_get(&self, url: &str) -> Result<HttpResponse, TorrentError> {
        // Add small delay to simulate network latency
        tokio::time::sleep(Duration::from_millis(50)).await;

        // Check for predefined response
        if let Some(response) = self.responses.get(url) {
            tracing::debug!("Simulation: returning predefined response for {}", url);
            return Ok(response.clone());
        }

        // Default to 404 for unknown URLs
        tracing::debug!(
            "Simulation: no response configured for {}, returning 404",
            url
        );
        Ok(HttpResponse::new(404, b"Not Found".to_vec()))
    }

    async fn http_post(&self, url: &str, _body: &[u8]) -> Result<HttpResponse, TorrentError> {
        // For simulation, POST behaves the same as GET
        self.http_get(url).await
    }

    fn configure_http_timeout(&mut self, timeout: Duration) {
        self.timeout = timeout;
    }

    fn timeout(&self) -> Duration {
        self.timeout
    }
}
