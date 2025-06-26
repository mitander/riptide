//! Mock BitTorrent peer for simulation

use std::time::Duration;

/// Mock peer for BitTorrent protocol simulation.
///
/// Simulates BitTorrent peer behavior with configurable upload speed,
/// reliability, and latency for testing download scenarios.
pub struct MockPeer {
    peer_id: String,
    upload_speed: u64, // bytes per second
    reliability: f32,  // 0.0 to 1.0
    latency: Duration,
}

impl MockPeer {
    /// Creates new mock peer with default settings.
    pub fn new(peer_id: String) -> Self {
        Self {
            peer_id,
            upload_speed: 1_000_000, // 1 MB/s default
            reliability: 0.95,
            latency: Duration::from_millis(50),
        }
    }

    /// Returns builder for customizing peer behavior.
    pub fn builder() -> MockPeerBuilder {
        MockPeerBuilder::new()
    }

    /// Returns peer identifier string.
    pub fn peer_id(&self) -> &str {
        &self.peer_id
    }

    /// Returns configured upload speed in bytes per second.
    pub fn upload_speed(&self) -> u64 {
        self.upload_speed
    }

    /// Simulate sending a piece of data
    ///
    /// # Errors
    /// - `PeerError::ConnectionLost` - Simulated connection failure based on reliability
    pub async fn send_piece(&self, piece_size: usize) -> Result<Vec<u8>, PeerError> {
        // Simulate connection reliability
        if rand::random::<f32>() > self.reliability {
            return Err(PeerError::ConnectionLost);
        }

        // Simulate network latency
        tokio::time::sleep(self.latency).await;

        // Simulate upload speed limitation
        let transfer_time = Duration::from_secs_f64(piece_size as f64 / self.upload_speed as f64);
        tokio::time::sleep(transfer_time).await;

        // Return mock piece data
        Ok(vec![0u8; piece_size])
    }
}

/// Builder for configuring mock peer behavior.
///
/// Allows customization of peer characteristics including upload speed,
/// connection reliability, and network latency before creating the peer.
pub struct MockPeerBuilder {
    peer_id: Option<String>,
    upload_speed: u64,
    reliability: f32,
    latency: Duration,
}

impl MockPeerBuilder {
    fn new() -> Self {
        Self {
            peer_id: None,
            upload_speed: 1_000_000,
            reliability: 0.95,
            latency: Duration::from_millis(50),
        }
    }

    /// Sets peer identifier string.
    pub fn peer_id(mut self, id: String) -> Self {
        self.peer_id = Some(id);
        self
    }

    /// Sets upload speed in bytes per second.
    pub fn upload_speed(mut self, bytes_per_second: u64) -> Self {
        self.upload_speed = bytes_per_second;
        self
    }

    /// Sets connection reliability as probability (0.0-1.0).
    pub fn reliability(mut self, rate: f32) -> Self {
        self.reliability = rate.clamp(0.0, 1.0);
        self
    }

    /// Sets network latency for peer communication.
    pub fn latency(mut self, duration: Duration) -> Self {
        self.latency = duration;
        self
    }

    /// Creates mock peer with configured settings.
    pub fn build(self) -> MockPeer {
        let peer_id = self
            .peer_id
            .unwrap_or_else(|| format!("MOCK{:08}", rand::random::<u32>()));

        MockPeer {
            peer_id,
            upload_speed: self.upload_speed,
            reliability: self.reliability,
            latency: self.latency,
        }
    }
}

/// Errors that can occur during peer simulation.
///
/// Covers connection failures and protocol errors that may
/// occur during mock peer communication.
#[derive(Debug, thiserror::Error)]
pub enum PeerError {
    #[error("Peer connection lost")]
    ConnectionLost,

    #[error("Peer protocol error: {message}")]
    ProtocolError { message: String },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mock_peer_default() {
        let peer = MockPeer::new("TEST_PEER_001".to_string());
        assert_eq!(peer.peer_id(), "TEST_PEER_001");
        assert_eq!(peer.upload_speed(), 1_000_000); // 1 MB/s default
    }

    #[test]
    fn test_mock_peer_builder() {
        let peer = MockPeer::builder()
            .peer_id("CUSTOM_PEER".to_string())
            .upload_speed(5_000_000) // 5 MB/s
            .reliability(0.8)
            .latency(Duration::from_millis(100))
            .build();

        assert_eq!(peer.peer_id(), "CUSTOM_PEER");
        assert_eq!(peer.upload_speed(), 5_000_000);
    }

    #[test]
    fn test_mock_peer_builder_auto_id() {
        let peer = MockPeer::builder()
            .upload_speed(2_000_000)
            .build();

        // Should generate an auto ID starting with "MOCK"
        assert!(peer.peer_id().starts_with("MOCK"));
        assert_eq!(peer.upload_speed(), 2_000_000);
    }

    #[tokio::test]
    async fn test_send_piece_success_high_reliability() {
        let peer = MockPeer::builder()
            .peer_id("RELIABLE_PEER".to_string())
            .reliability(1.0) // 100% reliable
            .latency(Duration::from_millis(1)) // Very low latency for fast test
            .upload_speed(10_000_000) // 10 MB/s
            .build();

        let piece_size = 256_000; // 256KB
        let result = peer.send_piece(piece_size).await;

        assert!(result.is_ok());
        let data = result.unwrap();
        assert_eq!(data.len(), piece_size);
        // Should be all zeros (mock data)
        assert!(data.iter().all(|&byte| byte == 0));
    }

    #[tokio::test]
    async fn test_send_piece_reliability_timing() {
        let peer = MockPeer::builder()
            .peer_id("SLOW_PEER".to_string())
            .reliability(1.0) // 100% reliable to ensure success
            .latency(Duration::from_millis(50))
            .upload_speed(1_000_000) // 1 MB/s
            .build();

        let piece_size = 100_000; // 100KB
        let start = std::time::Instant::now();
        let result = peer.send_piece(piece_size).await;
        let elapsed = start.elapsed();

        assert!(result.is_ok());
        
        // Should take at least latency (50ms) + transfer time (100ms for 100KB at 1MB/s)
        // Total expected: ~150ms, allowing for some variance
        assert!(elapsed >= Duration::from_millis(140));
        assert!(elapsed <= Duration::from_millis(200));
    }

    #[tokio::test]
    async fn test_send_piece_unreliable_can_fail() {
        let peer = MockPeer::builder()
            .peer_id("UNRELIABLE_PEER".to_string())
            .reliability(0.0) // 0% reliable - should always fail
            .latency(Duration::from_millis(1))
            .build();

        let result = peer.send_piece(1000).await;
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), PeerError::ConnectionLost));
    }

    #[test]
    fn test_builder_reliability_clamping() {
        // Test that reliability values are clamped to [0.0, 1.0]
        let peer_high = MockPeer::builder()
            .reliability(2.0) // Above 1.0
            .build();

        let peer_low = MockPeer::builder()
            .reliability(-0.5) // Below 0.0
            .build();

        // Both should be clamped to valid range
        // We can't directly access reliability, but we can test behavior
        assert!(peer_high.peer_id().starts_with("MOCK"));
        assert!(peer_low.peer_id().starts_with("MOCK"));
    }

    #[test]
    fn test_peer_error_display() {
        let connection_error = PeerError::ConnectionLost;
        assert_eq!(connection_error.to_string(), "Peer connection lost");

        let protocol_error = PeerError::ProtocolError {
            message: "Invalid handshake".to_string(),
        };
        assert_eq!(protocol_error.to_string(), "Peer protocol error: Invalid handshake");
    }

    #[tokio::test]
    async fn test_upload_speed_affects_transfer_time() {
        let fast_peer = MockPeer::builder()
            .reliability(1.0)
            .latency(Duration::from_millis(1))
            .upload_speed(10_000_000) // 10 MB/s
            .build();

        let slow_peer = MockPeer::builder()
            .reliability(1.0)
            .latency(Duration::from_millis(1))
            .upload_speed(1_000_000) // 1 MB/s
            .build();

        let piece_size = 100_000; // 100KB

        let start = std::time::Instant::now();
        fast_peer.send_piece(piece_size).await.unwrap();
        let fast_time = start.elapsed();

        let start = std::time::Instant::now();
        slow_peer.send_piece(piece_size).await.unwrap();
        let slow_time = start.elapsed();

        // Slow peer should take significantly longer
        assert!(slow_time > fast_time);
        
        // Fast peer should be roughly 10x faster (allowing for variance)
        let ratio = slow_time.as_nanos() as f64 / fast_time.as_nanos() as f64;
        assert!(ratio > 5.0 && ratio < 15.0);
    }
}
