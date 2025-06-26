//! Custom magneto provider for deterministic torrent discovery simulation
//!
//! Provides offline torrent discovery with realistic data for testing BitTorrent
//! functionality without network dependencies or rate limiting.

use std::collections::HashMap;
use std::time::Duration;

use magneto::{ClientError, SearchRequest, Torrent};
use rand::{Rng, SeedableRng};
use rand_chacha::ChaCha8Rng;

/// Mock magneto provider for deterministic torrent discovery.
///
/// Returns realistic torrent data based on predefined content database
/// and deterministic random generation for consistent test results.
pub struct MockMagnetoProvider {
    content_database: HashMap<String, Vec<MockTorrentEntry>>,
    rng: ChaCha8Rng,
    failure_rate: f64,
    response_delay: Duration,
}

/// Mock torrent entry in the content database.
#[derive(Debug, Clone)]
pub struct MockTorrentEntry {
    pub name: String,
    pub size_bytes: u64,
    pub seeders: u32,
    pub leechers: u32,
    pub magnet_template: String,
    pub categories: Vec<String>,
}

/// Parameters for creating a new torrent entry.
#[derive(Debug)]
pub struct TorrentEntryParams {
    pub name: String,
    pub size_bytes: u64,
    pub seeders: u32,
    pub leechers: u32,
}

impl MockMagnetoProvider {
    /// Creates new mock provider with default content database.
    pub fn new() -> Self {
        let mut provider = Self {
            content_database: HashMap::new(),
            rng: ChaCha8Rng::seed_from_u64(42),
            failure_rate: 0.0,
            response_delay: Duration::from_millis(100),
        };

        provider.populate_default_content();
        provider
    }

    /// Creates provider with custom seed for deterministic behavior.
    pub fn with_seed(seed: u64) -> Self {
        let mut provider = Self {
            content_database: HashMap::new(),
            rng: ChaCha8Rng::seed_from_u64(seed),
            failure_rate: 0.0,
            response_delay: Duration::from_millis(100),
        };

        provider.populate_default_content();
        provider
    }

    /// Sets failure rate for simulating network issues.
    pub fn with_failure_rate(mut self, rate: f64) -> Self {
        self.failure_rate = rate.clamp(0.0, 1.0);
        self
    }

    /// Sets simulated network response delay.
    pub fn with_response_delay(mut self, delay: Duration) -> Self {
        self.response_delay = delay;
        self
    }

    /// Adds custom content category to the database.
    pub fn add_content_category(&mut self, category: String, entries: Vec<MockTorrentEntry>) {
        self.content_database.insert(category, entries);
    }

    /// Populates database with realistic content for testing.
    fn populate_default_content(&mut self) {
        // Open source movies
        let open_source_movies = vec![
            MockTorrentEntry {
                name: "Big Buck Bunny (2008) [1080p]".to_string(),
                size_bytes: 1_500_000_000, // 1.5GB
                seeders: 245,
                leechers: 23,
                magnet_template: "magnet:?xt=urn:btih:BIG_BUCK_BUNNY_HASH".to_string(),
                categories: vec!["movie".to_string(), "1080p".to_string()],
            },
            MockTorrentEntry {
                name: "Sintel (2010) [4K]".to_string(),
                size_bytes: 8_500_000_000, // 8.5GB
                seeders: 156,
                leechers: 45,
                magnet_template: "magnet:?xt=urn:btih:SINTEL_4K_HASH".to_string(),
                categories: vec!["movie".to_string(), "4k".to_string()],
            },
            MockTorrentEntry {
                name: "Tears of Steel (2012) [720p]".to_string(),
                size_bytes: 750_000_000, // 750MB
                seeders: 89,
                leechers: 12,
                magnet_template: "magnet:?xt=urn:btih:TEARS_OF_STEEL_HASH".to_string(),
                categories: vec!["movie".to_string(), "720p".to_string()],
            },
            MockTorrentEntry {
                name: "Elephants Dream (2006) [1080p]".to_string(),
                size_bytes: 1_200_000_000, // 1.2GB
                seeders: 67,
                leechers: 8,
                magnet_template: "magnet:?xt=urn:btih:ELEPHANTS_DREAM_HASH".to_string(),
                categories: vec!["movie".to_string(), "1080p".to_string()],
            },
        ];

        // Creative Commons content
        let creative_commons = vec![
            MockTorrentEntry {
                name: "Creative Commons Movie Collection [Mixed Quality]".to_string(),
                size_bytes: 25_000_000_000, // 25GB
                seeders: 423,
                leechers: 156,
                magnet_template: "magnet:?xt=urn:btih:CC_COLLECTION_HASH".to_string(),
                categories: vec!["collection".to_string(), "creative_commons".to_string()],
            },
            MockTorrentEntry {
                name: "Internet Archive Documentary Pack".to_string(),
                size_bytes: 45_000_000_000, // 45GB
                seeders: 234,
                leechers: 67,
                magnet_template: "magnet:?xt=urn:btih:IA_DOCS_HASH".to_string(),
                categories: vec!["documentary".to_string(), "collection".to_string()],
            },
        ];

        // Linux distributions
        let linux_distros = vec![
            MockTorrentEntry {
                name: "Ubuntu 22.04.3 Desktop amd64".to_string(),
                size_bytes: 4_700_000_000, // 4.7GB
                seeders: 1245,
                leechers: 234,
                magnet_template: "magnet:?xt=urn:btih:UBUNTU_22_04_HASH".to_string(),
                categories: vec!["software".to_string(), "linux".to_string()],
            },
            MockTorrentEntry {
                name: "Debian 12.2.0 amd64 DVD".to_string(),
                size_bytes: 3_900_000_000, // 3.9GB
                seeders: 567,
                leechers: 89,
                magnet_template: "magnet:?xt=urn:btih:DEBIAN_12_HASH".to_string(),
                categories: vec!["software".to_string(), "linux".to_string()],
            },
        ];

        // Games and software
        let games_software = vec![
            MockTorrentEntry {
                name: "OpenTTD 13.4 Full Game Collection".to_string(),
                size_bytes: 2_100_000_000, // 2.1GB
                seeders: 145,
                leechers: 34,
                magnet_template: "magnet:?xt=urn:btih:OPENTTD_HASH".to_string(),
                categories: vec!["game".to_string(), "open_source".to_string()],
            },
            MockTorrentEntry {
                name: "Blender 4.0 Complete Suite + Assets".to_string(),
                size_bytes: 12_000_000_000, // 12GB
                seeders: 234,
                leechers: 78,
                magnet_template: "magnet:?xt=urn:btih:BLENDER_4_HASH".to_string(),
                categories: vec!["software".to_string(), "creative".to_string()],
            },
        ];

        self.content_database.insert(
            "big buck bunny".to_string(),
            vec![open_source_movies[0].clone()],
        );
        self.content_database
            .insert("sintel".to_string(), vec![open_source_movies[1].clone()]);
        self.content_database.insert(
            "tears of steel".to_string(),
            vec![open_source_movies[2].clone()],
        );
        self.content_database.insert(
            "elephants dream".to_string(),
            vec![open_source_movies[3].clone()],
        );
        self.content_database
            .insert("open source movies".to_string(), open_source_movies);
        self.content_database
            .insert("creative commons".to_string(), creative_commons);
        self.content_database
            .insert("linux".to_string(), linux_distros.clone());
        self.content_database
            .insert("ubuntu".to_string(), vec![linux_distros[0].clone()]);
        self.content_database
            .insert("debian".to_string(), vec![linux_distros[1].clone()]);
        self.content_database
            .insert("games".to_string(), games_software.clone());
        self.content_database
            .insert("software".to_string(), games_software);
        self.content_database.insert(
            "blender".to_string(),
            vec![MockTorrentEntry {
                name: "Blender 4.0 Complete Suite + Assets".to_string(),
                size_bytes: 12_000_000_000,
                seeders: 234,
                leechers: 78,
                magnet_template: "magnet:?xt=urn:btih:BLENDER_4_HASH".to_string(),
                categories: vec!["software".to_string(), "creative".to_string()],
            }],
        );
    }

    /// Searches content database for matching torrents.
    fn search_content(&mut self, query: &str, limit: usize) -> Vec<Torrent> {
        let query_lower = query.to_lowercase();
        let mut results = Vec::new();
        let mut processed_names = std::collections::HashSet::new();

        // Clone data to avoid borrow issues
        let database = self.content_database.clone();

        // Direct category matches
        if let Some(entries) = database.get(&query_lower) {
            for entry in entries.iter().take(limit) {
                let torrent = self.create_torrent_from_entry(entry);
                processed_names.insert(torrent.name.clone());
                results.push(torrent);
            }
        }

        // Fuzzy search across all categories if not enough results
        if results.len() < limit {
            for (category, entries) in &database {
                if category.contains(&query_lower) || query_lower.contains(category) {
                    for entry in entries.iter().take(limit - results.len()) {
                        if !processed_names.contains(&entry.name) {
                            let torrent = self.create_torrent_from_entry(entry);
                            processed_names.insert(torrent.name.clone());
                            results.push(torrent);
                        }
                    }
                }
            }
        }

        // Search within torrent names if still not enough
        if results.len() < limit {
            for entries in database.values() {
                for entry in entries {
                    if entry.name.to_lowercase().contains(&query_lower)
                        && !processed_names.contains(&entry.name)
                        && results.len() < limit
                    {
                        let torrent = self.create_torrent_from_entry(entry);
                        processed_names.insert(torrent.name.clone());
                        results.push(torrent);
                    }
                }
            }
        }

        // Add some variation to results
        for torrent in &mut results {
            // Vary peer counts slightly
            let variation = self.rng.random_range(-10..=20);
            torrent.seeders = (torrent.seeders as i32 + variation).max(1) as u32;

            let leecher_variation = self.rng.random_range(-5..=15);
            torrent.peers = (torrent.peers as i32 + leecher_variation).max(0) as u32;
        }

        results
    }

    /// Creates Torrent from MockTorrentEntry with realistic data.
    fn create_torrent_from_entry(&mut self, entry: &MockTorrentEntry) -> Torrent {
        // Generate realistic hash for magnet link
        let hash = self.generate_info_hash(&entry.name);
        let magnet_link = format!(
            "magnet:?xt=urn:btih:{}&dn={}&tr=udp://tracker.example.com:8080/announce",
            hash,
            urlencoding::encode(&entry.name)
        );

        Torrent {
            name: entry.name.clone(),
            magnet_link,
            seeders: entry.seeders,
            peers: entry.leechers,
            size_bytes: entry.size_bytes,
            provider: "MockMagnetoProvider".to_string(),
        }
    }

    /// Generates deterministic info hash for torrent name.
    fn generate_info_hash(&mut self, name: &str) -> String {
        use sha1::{Digest, Sha1};

        let mut hasher = Sha1::new();
        hasher.update(name.as_bytes());
        hasher.update(self.rng.random::<u64>().to_le_bytes());

        let hash = hasher.finalize();
        hex::encode(&hash[..20])
    }
}

impl Default for MockMagnetoProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl MockMagnetoProvider {
    /// Searches for torrents based on query with simulated network behavior.
    pub async fn search(
        &mut self,
        request: SearchRequest<'_>,
    ) -> Result<Vec<Torrent>, ClientError> {
        // Simulate network delay
        tokio::time::sleep(self.response_delay).await;

        // Simulate network failures
        if self.rng.random_bool(self.failure_rate) {
            return Err(ClientError::ResponseError(anyhow::anyhow!(
                "Simulated network failure"
            )));
        }

        let limit = request.number_of_results.min(50);
        let results = self.search_content(request.query, limit);

        Ok(results)
    }

    /// Returns provider name for identification.
    pub fn name(&self) -> &str {
        "MockMagnetoProvider"
    }
}

/// Builder for creating customized mock magneto providers.
pub struct MockMagnetoProviderBuilder {
    seed: u64,
    failure_rate: f64,
    response_delay: Duration,
    custom_content: HashMap<String, Vec<MockTorrentEntry>>,
}

impl MockMagnetoProviderBuilder {
    /// Creates new builder with default settings.
    pub fn new() -> Self {
        Self {
            seed: 42,
            failure_rate: 0.0,
            response_delay: Duration::from_millis(100),
            custom_content: HashMap::new(),
        }
    }

    /// Sets deterministic seed for reproducible results.
    pub fn with_seed(mut self, seed: u64) -> Self {
        self.seed = seed;
        self
    }

    /// Sets failure rate for network simulation.
    pub fn with_failure_rate(mut self, rate: f64) -> Self {
        self.failure_rate = rate.clamp(0.0, 1.0);
        self
    }

    /// Sets simulated response delay.
    pub fn with_response_delay(mut self, delay: Duration) -> Self {
        self.response_delay = delay;
        self
    }

    /// Adds custom content category for specialized testing.
    pub fn add_content_category(
        mut self,
        category: String,
        entries: Vec<MockTorrentEntry>,
    ) -> Self {
        self.custom_content.insert(category, entries);
        self
    }

    /// Adds single torrent entry for specific testing.
    pub fn add_torrent(mut self, category: String, params: TorrentEntryParams) -> Self {
        let entry = MockTorrentEntry {
            name: params.name,
            size_bytes: params.size_bytes,
            seeders: params.seeders,
            leechers: params.leechers,
            magnet_template: "magnet:?xt=urn:btih:CUSTOM_HASH".to_string(),
            categories: vec![category.clone()],
        };

        self.custom_content.entry(category).or_default().push(entry);

        self
    }

    /// Builds the configured mock provider.
    pub fn build(self) -> MockMagnetoProvider {
        let mut provider = MockMagnetoProvider::with_seed(self.seed)
            .with_failure_rate(self.failure_rate)
            .with_response_delay(self.response_delay);

        // Add custom content
        for (category, entries) in self.custom_content {
            provider.add_content_category(category, entries);
        }

        provider
    }
}

impl Default for MockMagnetoProviderBuilder {
    fn default() -> Self {
        Self::new()
    }
}

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

        assert!(provider.content_database.contains_key("test"));
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
