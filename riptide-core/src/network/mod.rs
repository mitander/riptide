//! Network abstraction layer for production and simulation environments
//!
//! Provides unified traits for HTTP requests, enabling both real network operations
//! and deterministic simulation with the same core logic.

pub mod peer;
pub mod simulation;
pub mod token_bucket;

use std::time::Duration;

use async_trait::async_trait;
pub use peer::{PeerConnection, PeerLayer, ProductionPeerLayer, SimulationPeerLayer};
pub use simulation::SimulationNetworkLayer;
pub use token_bucket::{TokenBucket, TokenBucketError};

use crate::torrent::TorrentError;

/// HTTP response abstraction
#[derive(Debug, Clone)]
pub struct HttpResponse {
    /// HTTP status code (200, 404, 500, etc.)
    pub status_code: u16,
    /// Response body bytes
    pub body: Vec<u8>,
}

impl HttpResponse {
    /// Create new HTTP response with status code and body
    pub fn new(status_code: u16, body: Vec<u8>) -> Self {
        Self { status_code, body }
    }

    /// Returns true if the HTTP status code indicates success (2xx).
    pub fn is_success(&self) -> bool {
        self.status_code >= 200 && self.status_code < 300
    }
}

/// Network layer abstraction for HTTP operations
///
/// Enables both production HTTP clients and simulation environments
/// to use the same tracker and peer communication logic.
#[async_trait]
pub trait NetworkLayer: Send + Sync {
    /// Performs HTTP GET request
    ///
    /// # Errors
    ///
    /// - `TorrentError::TrackerConnectionFailed` - If network or HTTP error
    async fn http_get(&self, url: &str) -> Result<HttpResponse, TorrentError>;

    /// Performs HTTP POST request
    ///
    /// # Errors
    ///
    /// - `TorrentError::TrackerConnectionFailed` - If network or HTTP error
    async fn http_post(&self, url: &str, body: &[u8]) -> Result<HttpResponse, TorrentError>;

    /// Configure timeout for HTTP requests
    fn configure_http_timeout(&mut self, timeout: Duration);

    /// Returns current timeout setting
    fn timeout(&self) -> Duration;
}

/// Production HTTP client using reqwest
pub struct ProductionNetworkLayer {
    client: reqwest::Client,
    timeout: Duration,
}

impl ProductionNetworkLayer {
    /// Creates a new production network layer with the specified timeout.
    ///
    /// # Panics
    ///
    /// Panics if HTTP client creation fails due to invalid configuration.
    /// This should never happen with valid timeout and user agent values.
    pub fn new(timeout: Duration) -> Self {
        let client = reqwest::Client::builder()
            .timeout(timeout)
            .user_agent("riptide/0.1.0")
            .redirect(reqwest::redirect::Policy::limited(3))
            .build()
            .expect("HTTP client creation should not fail");

        Self { client, timeout }
    }
}

#[async_trait]
impl NetworkLayer for ProductionNetworkLayer {
    async fn http_get(&self, url: &str) -> Result<HttpResponse, TorrentError> {
        let response = self.client.get(url).send().await.map_err(|e| {
            let error_msg = if e.is_timeout() {
                format!("Request timed out: {url}")
            } else if e.is_connect() {
                format!("Failed to connect: {url}")
            } else if e.is_request() {
                format!("Invalid request: {url}")
            } else {
                format!("HTTP request failed: {e}")
            };

            TorrentError::TrackerConnectionFailed { url: error_msg }
        })?;

        let status = response.status().as_u16();
        let body = response
            .bytes()
            .await
            .map_err(|e| TorrentError::TrackerConnectionFailed {
                url: format!("Failed to read response body: {e}"),
            })?
            .to_vec();

        Ok(HttpResponse::new(status, body))
    }

    async fn http_post(&self, url: &str, body: &[u8]) -> Result<HttpResponse, TorrentError> {
        let response = self
            .client
            .post(url)
            .body(body.to_vec())
            .send()
            .await
            .map_err(|e| TorrentError::TrackerConnectionFailed {
                url: format!("HTTP POST failed: {e}"),
            })?;

        let status = response.status().as_u16();
        let response_body = response
            .bytes()
            .await
            .map_err(|e| TorrentError::TrackerConnectionFailed {
                url: format!("Failed to read response body: {e}"),
            })?
            .to_vec();

        Ok(HttpResponse::new(status, response_body))
    }

    fn configure_http_timeout(&mut self, timeout: Duration) {
        self.timeout = timeout;
        self.client = reqwest::Client::builder()
            .timeout(timeout)
            .user_agent("riptide/0.1.0")
            .redirect(reqwest::redirect::Policy::limited(3))
            .build()
            .expect("HTTP client creation should not fail");
    }

    fn timeout(&self) -> Duration {
        self.timeout
    }
}
