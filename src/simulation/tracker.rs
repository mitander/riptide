//! Mock BitTorrent tracker for simulation

use std::net::SocketAddr;
use std::time::Duration;

/// Mock tracker for offline development.
///
/// Simulates BitTorrent tracker responses for testing without real trackers.
/// Supports configurable peer counts, failure injection, and realistic delays.
pub struct MockTracker {
    seeders: u32,
    leechers: u32,
    failure_rate: f32,
    peers: Vec<SocketAddr>,
}

impl Default for MockTracker {
    fn default() -> Self {
        Self::new()
    }
}

impl MockTracker {
    /// Creates new mock tracker with default settings.
    pub fn new() -> Self {
        Self {
            seeders: 10,
            leechers: 5,
            failure_rate: 0.0,
            peers: Vec::new(),
        }
    }

    /// Returns builder for customizing tracker behavior.
    pub fn builder() -> MockTrackerBuilder {
        MockTrackerBuilder::new()
    }

    /// Simulate tracker announce request
    ///
    /// # Errors
    /// - `TrackerError::ConnectionFailed` - Simulated connection failure based on failure rate
    pub async fn announce(&self, _info_hash: &[u8; 20]) -> Result<AnnounceResponse, TrackerError> {
        // Simulate network delay
        tokio::time::sleep(Duration::from_millis(50)).await;

        // Simulate failure if configured
        if rand::random::<f32>() < self.failure_rate {
            return Err(TrackerError::ConnectionFailed);
        }

        Ok(AnnounceResponse {
            interval: 1800,
            seeders: self.seeders,
            leechers: self.leechers,
            peers: self.peers.clone(),
        })
    }
}

/// Builder for configuring mock tracker behavior.
///
/// Allows customization of peer counts, failure rates, and other
/// simulation parameters before creating the tracker instance.
pub struct MockTrackerBuilder {
    seeders: u32,
    leechers: u32,
    failure_rate: f32,
}

impl MockTrackerBuilder {
    fn new() -> Self {
        Self {
            seeders: 10,
            leechers: 5,
            failure_rate: 0.0,
        }
    }

    /// Sets number of seeders to report in responses.
    pub fn with_seeders(mut self, count: u32) -> Self {
        self.seeders = count;
        self
    }

    /// Sets number of leechers to report in responses.
    pub fn with_leechers(mut self, count: u32) -> Self {
        self.leechers = count;
        self
    }

    /// Sets probability (0.0-1.0) of simulated connection failures.
    pub fn with_failure_rate(mut self, rate: f32) -> Self {
        self.failure_rate = rate;
        self
    }

    /// Creates mock tracker with configured settings.
    pub fn build(self) -> MockTracker {
        let mut peers = Vec::new();

        // Generate realistic peer addresses
        for i in 0..(self.seeders + self.leechers) {
            let ip = format!("192.168.1.{}", 100 + (i % 50));
            let port = 6881 + (i % 100) as u16;
            if let Ok(addr) = format!("{}:{}", ip, port).parse() {
                peers.push(addr);
            }
        }

        MockTracker {
            seeders: self.seeders,
            leechers: self.leechers,
            failure_rate: self.failure_rate,
            peers,
        }
    }
}

/// Mock tracker announce response.
///
/// Contains peer list and swarm statistics returned by tracker
/// in response to announce requests.
#[derive(Debug)]
pub struct AnnounceResponse {
    pub interval: u32,
    pub seeders: u32,
    pub leechers: u32,
    pub peers: Vec<SocketAddr>,
}

/// Errors that can occur during tracker simulation.
///
/// Covers connection failures and protocol errors that may
/// occur during tracker communication simulation.
#[derive(Debug, thiserror::Error)]
pub enum TrackerError {
    #[error("Connection to tracker failed")]
    ConnectionFailed,

    #[error("Invalid response from tracker")]
    InvalidResponse,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_mock_tracker_successful_announce() {
        let tracker = MockTracker::builder()
            .with_seeders(5)
            .with_leechers(3)
            .build();

        let info_hash = [0u8; 20];
        let response = tracker.announce(&info_hash).await.unwrap();

        assert_eq!(response.seeders, 5);
        assert_eq!(response.leechers, 3);
        assert_eq!(response.peers.len(), 8);
        assert_eq!(response.interval, 1800);
    }

    #[tokio::test]
    async fn test_mock_tracker_failure_injection() {
        let tracker = MockTracker::builder()
            .with_failure_rate(1.0) // Always fail
            .build();

        let info_hash = [0u8; 20];
        let result = tracker.announce(&info_hash).await;

        assert!(result.is_err());
        matches!(result.unwrap_err(), TrackerError::ConnectionFailed);
    }

    #[test]
    fn test_tracker_builder_generates_valid_peers() {
        let tracker = MockTracker::builder()
            .with_seeders(2)
            .with_leechers(1)
            .build();

        assert_eq!(tracker.peers.len(), 3);

        // Verify all peers have valid socket addresses
        for peer in &tracker.peers {
            assert!(peer.ip().is_ipv4());
            assert!(peer.port() >= 6881);
        }
    }
}
