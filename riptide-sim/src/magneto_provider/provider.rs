//! Core mock magneto provider implementation.

use std::collections::HashMap;
use std::time::Duration;

use magneto::{ClientError, SearchRequest, Torrent};
use rand::{Rng, SeedableRng};
use rand_chacha::ChaCha8Rng;

use super::content_database::create_default_content_database;

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
        for (category, entries) in create_default_content_database() {
            self.content_database.insert(category, entries);
        }
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

impl Default for MockMagnetoProvider {
    fn default() -> Self {
        Self::new()
    }
}