//! Mock implementation of PieceProvider for deterministic testing.
//!
//! This module provides a test implementation that serves bytes from an
//! in-memory buffer, enabling comprehensive offline testing of the streaming
//! pipeline without requiring actual BitTorrent downloads.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::Duration;

use bytes::Bytes;
use riptide_core::streaming::traits::{PieceProvider, PieceProviderError};

/// Mock piece provider for deterministic simulation testing.
///
/// Serves bytes from an in-memory buffer with configurable failure modes
/// and delays to test streaming pipeline robustness. This enables complete
/// end-to-end testing without network dependencies.
#[derive(Clone)]
pub struct MockPieceProvider {
    /// Complete file data loaded into memory.
    data: Bytes,
    /// Whether to fail on the next read attempt.
    fail_on_read: Arc<AtomicBool>,
    /// Simulated delay before returning data.
    read_delay: Duration,
    /// Counter for read operations (useful for testing).
    read_count: Arc<AtomicU64>,
    /// Whether to return NotYetAvailable errors.
    simulate_unavailable: Arc<AtomicBool>,
}

impl MockPieceProvider {
    /// Creates a new mock provider with the given file data.
    ///
    /// The provider will serve bytes from the provided data buffer
    /// with no delays or failures by default.
    pub fn new(data: Bytes) -> Self {
        Self {
            data,
            fail_on_read: Arc::new(AtomicBool::new(false)),
            read_delay: Duration::ZERO,
            read_count: Arc::new(AtomicU64::new(0)),
            simulate_unavailable: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Configures the provider to fail on the next read.
    ///
    /// Useful for testing error handling in the streaming pipeline.
    /// The failure is cleared after one read attempt.
    pub fn fail_next_read(&self) {
        self.fail_on_read.store(true, Ordering::Release);
    }

    /// Sets a delay to simulate slow piece downloads.
    ///
    /// Each read will sleep for the specified duration before returning,
    /// simulating network latency or slow peers.
    pub fn with_read_delay(mut self, delay: Duration) -> Self {
        self.read_delay = delay;
        self
    }

    /// Configures the provider to simulate unavailable pieces.
    ///
    /// When enabled, reads will return NotYetAvailable errors,
    /// simulating pieces that haven't been downloaded yet.
    pub fn simulate_unavailable(&self, unavailable: bool) {
        self.simulate_unavailable
            .store(unavailable, Ordering::Release);
    }

    /// Returns the number of read operations performed.
    ///
    /// Useful for verifying that the streaming pipeline is making
    /// efficient read requests.
    pub fn read_count(&self) -> u64 {
        self.read_count.load(Ordering::Acquire)
    }

    /// Resets all failure modes and counters.
    ///
    /// Returns the provider to its initial state for test isolation.
    pub fn reset(&self) {
        self.fail_on_read.store(false, Ordering::Release);
        self.simulate_unavailable.store(false, Ordering::Release);
        self.read_count.store(0, Ordering::Release);
    }
}

#[async_trait::async_trait]
impl PieceProvider for MockPieceProvider {
    async fn read_at(&self, offset: u64, length: usize) -> Result<Bytes, PieceProviderError> {
        // Increment read counter
        self.read_count.fetch_add(1, Ordering::AcqRel);

        // Simulate read delay if configured
        if !self.read_delay.is_zero() {
            tokio::time::sleep(self.read_delay).await;
        }

        // Check for simulated unavailability
        if self.simulate_unavailable.load(Ordering::Acquire) {
            return Err(PieceProviderError::NotYetAvailable);
        }

        // Check for one-time failure
        if self.fail_on_read.swap(false, Ordering::AcqRel) {
            return Err(PieceProviderError::StorageError(
                "simulated storage failure".to_string(),
            ));
        }

        // Validate range
        let file_size = self.data.len() as u64;
        if offset + length as u64 > file_size {
            return Err(PieceProviderError::InvalidRange {
                offset,
                length,
                file_size,
            });
        }

        // Return the requested slice
        let start = offset as usize;
        let end = start + length;
        Ok(self.data.slice(start..end))
    }

    async fn size(&self) -> u64 {
        self.data.len() as u64
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn mock_provider_serves_correct_bytes() {
        let data = Bytes::from(vec![0u8, 1, 2, 3, 4, 5, 6, 7, 8, 9]);
        let provider = MockPieceProvider::new(data);

        // Read middle bytes
        let result = provider.read_at(3, 4).await.unwrap();
        assert_eq!(result, vec![3, 4, 5, 6]);

        // Read from beginning
        let result = provider.read_at(0, 3).await.unwrap();
        assert_eq!(result, vec![0, 1, 2]);

        // Read to end
        let result = provider.read_at(7, 3).await.unwrap();
        assert_eq!(result, vec![7, 8, 9]);
    }

    #[tokio::test]
    async fn mock_provider_validates_range() {
        let data = Bytes::from(vec![0u8; 10]);
        let provider = MockPieceProvider::new(data);

        // Valid range at boundary
        assert!(provider.read_at(0, 10).await.is_ok());

        // Invalid range
        let err = provider.read_at(5, 10).await.unwrap_err();
        match err {
            PieceProviderError::InvalidRange {
                offset,
                length,
                file_size,
            } => {
                assert_eq!(offset, 5);
                assert_eq!(length, 10);
                assert_eq!(file_size, 10);
            }
            _ => panic!("Expected InvalidRange error"),
        }
    }

    #[tokio::test]
    async fn mock_provider_simulates_failures() {
        let data = Bytes::from(vec![0u8; 100]);
        let provider = MockPieceProvider::new(data);

        // Simulate one-time failure
        provider.fail_next_read();
        let err = provider.read_at(0, 10).await.unwrap_err();
        assert!(matches!(err, PieceProviderError::StorageError(_)));

        // Next read should succeed
        assert!(provider.read_at(0, 10).await.is_ok());
    }

    #[tokio::test]
    async fn mock_provider_simulates_unavailability() {
        let data = Bytes::from(vec![0u8; 100]);
        let provider = MockPieceProvider::new(data);

        // Enable unavailability simulation
        provider.simulate_unavailable(true);
        let err = provider.read_at(0, 10).await.unwrap_err();
        assert!(matches!(err, PieceProviderError::NotYetAvailable));

        // Disable and verify success
        provider.simulate_unavailable(false);
        assert!(provider.read_at(0, 10).await.is_ok());
    }

    #[tokio::test]
    async fn mock_provider_tracks_read_count() {
        let data = Bytes::from(vec![0u8; 100]);
        let provider = MockPieceProvider::new(data);

        assert_eq!(provider.read_count(), 0);

        provider.read_at(0, 10).await.unwrap();
        assert_eq!(provider.read_count(), 1);

        provider.read_at(10, 20).await.unwrap();
        assert_eq!(provider.read_count(), 2);

        // Failed reads also count
        provider.fail_next_read();
        let _ = provider.read_at(0, 10).await;
        assert_eq!(provider.read_count(), 3);
    }

    #[tokio::test]
    async fn mock_provider_reset_clears_state() {
        let data = Bytes::from(vec![0u8; 100]);
        let provider = MockPieceProvider::new(data);

        // Set up some state
        provider.simulate_unavailable(true);
        provider.fail_next_read();
        provider.read_at(0, 10).await.ok();

        // Reset
        provider.reset();

        // Verify clean state
        assert_eq!(provider.read_count(), 0);
        assert!(provider.read_at(0, 10).await.is_ok());
    }
}
