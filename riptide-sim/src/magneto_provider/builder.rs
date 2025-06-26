//! Builder pattern for creating customized mock magneto providers.

use std::collections::HashMap;
use std::time::Duration;

use super::{MockMagnetoProvider, MockTorrentEntry, TorrentEntryParams};

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