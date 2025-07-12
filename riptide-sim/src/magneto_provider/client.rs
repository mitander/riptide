//! Mock magneto client wrapper implementation.

use magneto::{ClientError, SearchRequest, Torrent};

use super::{MockMagnetoProvider, MockMagnetoProviderBuilder, TorrentEntryParams};

/// Mock magneto client wrapper that uses our custom provider.
pub struct MockMagnetoClient {
    provider: MockMagnetoProvider,
}

impl MockMagnetoClient {
    /// Creates new mock client with custom provider.
    pub fn new(provider: MockMagnetoProvider) -> Self {
        Self { provider }
    }

    /// Searches for torrents using the mock provider.
    ///
    /// # Errors
    ///
    /// - `ClientError::RequestFailed` - If network or search request failed
    /// - `ClientError::InvalidQuery` - If search query format invalid
    pub async fn search(
        &mut self,
        request: SearchRequest<'_>,
    ) -> Result<Vec<Torrent>, ClientError> {
        self.provider.search(request).await
    }
}

/// Creates mock magneto client with custom provider for testing.
pub fn create_mock_magneto_client(provider: MockMagnetoProvider) -> MockMagnetoClient {
    MockMagnetoClient::new(provider)
}

/// Creates mock magneto client with streaming-optimized content database.
pub fn create_streaming_test_client(seed: u64) -> MockMagnetoClient {
    let provider = MockMagnetoProviderBuilder::new()
        .with_seed(seed)
        .add_torrent(
            "streaming_test".to_string(),
            TorrentEntryParams {
                name: "Test Movie 2024 [1080p] [5.1]".to_string(),
                size_bytes: 4_500_000_000, // 4.5GB
                seeders: 156,
                leechers: 23,
            },
        )
        .add_torrent(
            "streaming_test".to_string(),
            TorrentEntryParams {
                name: "Sample Series S01E01-E10 [720p]".to_string(),
                size_bytes: 12_000_000_000, // 12GB
                seeders: 89,
                leechers: 45,
            },
        )
        .add_torrent(
            "streaming_test".to_string(),
            TorrentEntryParams {
                name: "Documentary Collection [4K]".to_string(),
                size_bytes: 35_000_000_000, // 35GB
                seeders: 234,
                leechers: 67,
            },
        )
        .build();

    create_mock_magneto_client(provider)
}

#[cfg(test)]
mod tests {
    use magneto::SearchRequest;

    use super::*;

    #[tokio::test]
    async fn test_mock_provider_search() {
        let mut provider = MockMagnetoProvider::with_seed(12345);
        let request = SearchRequest::new("big buck bunny");

        let results = provider.search(request).await.unwrap();
        assert!(!results.is_empty());
        assert!(results[0].name.contains("Big Buck Bunny"));
    }

    #[tokio::test]
    async fn test_provider_deterministic_behavior() {
        let mut provider1 = MockMagnetoProvider::with_seed(42);
        let mut provider2 = MockMagnetoProvider::with_seed(42);

        let request1 = SearchRequest::new("creative commons");
        let request2 = SearchRequest::new("creative commons");

        let results1 = provider1.search(request1).await.unwrap();
        let results2 = provider2.search(request2).await.unwrap();

        assert_eq!(results1.len(), results2.len());
        for (r1, r2) in results1.iter().zip(results2.iter()) {
            assert_eq!(r1.name, r2.name);
            assert_eq!(r1.seeders, r2.seeders);
        }
    }

    #[tokio::test]
    async fn test_failure_simulation() {
        let mut provider = MockMagnetoProvider::with_seed(999).with_failure_rate(1.0); // Always fail

        let request = SearchRequest::new("test");
        let result = provider.search(request).await;

        assert!(result.is_err());
    }

    #[test]
    fn test_builder_pattern() {
        let provider = MockMagnetoProviderBuilder::new()
            .with_seed(123)
            .with_failure_rate(0.1)
            .add_torrent(
                "test".to_string(),
                TorrentEntryParams {
                    name: "Test Torrent".to_string(),
                    size_bytes: 1_000_000_000,
                    seeders: 50,
                    leechers: 10,
                },
            )
            .build();

        // Note: content_database is private, so we test functionality instead
        // by verifying the provider was built successfully
        assert_eq!(provider.name(), "MockMagnetoProvider");
    }

    #[tokio::test]
    async fn test_streaming_client_creation() {
        let mut client = create_streaming_test_client(456);
        let request = SearchRequest::new("streaming_test");

        // This should work with the mock provider
        let results = client.search(request).await;
        assert!(results.is_ok());
    }
}
