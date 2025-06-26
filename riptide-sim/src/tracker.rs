//! Mock BitTorrent tracker for simulation with magneto integration

use std::collections::HashMap;
use std::net::SocketAddr;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use magneto::{ClientError, SearchRequest};
use rand::{Rng, SeedableRng};
use rand_chacha::ChaCha8Rng;
use riptide_core::torrent::InfoHash;

use super::magneto_provider::{
    MockMagnetoClient, MockMagnetoProviderBuilder, create_mock_magneto_client,
};

/// Mock tracker for offline development with magneto integration.
///
/// Simulates BitTorrent tracker responses for testing without real trackers.
/// Supports configurable peer counts, failure injection, realistic delays,
/// and integration with magneto for magnet link discovery simulation.
pub struct MockTracker {
    seeders: u32,
    leechers: u32,
    failure_rate: f32,
    peers: Vec<SocketAddr>,
    torrents: HashMap<InfoHash, MockTorrentData>,
    rng: ChaCha8Rng,
    magneto_client: Option<MockMagnetoClient>,
}

/// Mock torrent data for simulation.
#[derive(Debug, Clone)]
pub struct MockTorrentData {
    pub info_hash: InfoHash,
    pub name: String,
    pub size: u64,
    pub seeders: u32,
    pub leechers: u32,
    pub last_announce: SystemTime,
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
            torrents: HashMap::new(),
            rng: ChaCha8Rng::seed_from_u64(42),
            magneto_client: None,
        }
    }

    /// Returns builder for customizing tracker behavior.
    pub fn builder() -> MockTrackerBuilder {
        MockTrackerBuilder::new()
    }

    /// Enables magneto integration for magnet link discovery simulation.
    pub fn with_magneto(mut self) -> Self {
        // Use mock provider for deterministic results
        let provider = MockMagnetoProviderBuilder::new().with_seed(42).build();
        self.magneto_client = Some(create_mock_magneto_client(provider));
        self
    }

    /// Enables magneto integration with custom seed for deterministic behavior.
    pub fn with_magneto_seed(mut self, seed: u64) -> Self {
        let provider = MockMagnetoProviderBuilder::new().with_seed(seed).build();
        self.magneto_client = Some(create_mock_magneto_client(provider));
        self
    }

    /// Adds a mock torrent to the tracker database.
    pub fn add_torrent(&mut self, torrent_data: MockTorrentData) {
        self.torrents.insert(torrent_data.info_hash, torrent_data);
    }

    /// Searches for torrents using magneto (if enabled) and adds them to tracker.
    ///
    /// # Errors
    /// - `TrackerError::MagnetoNotEnabled` - Magneto client not configured
    /// - `TrackerError::SearchFailed` - Magneto search failed
    pub async fn search_and_add_torrents(
        &mut self,
        query: &str,
        limit: usize,
    ) -> Result<Vec<MockTorrentData>, TrackerError> {
        let magneto = self
            .magneto_client
            .as_mut()
            .ok_or(TrackerError::MagnetoNotEnabled)?;

        let search_request = SearchRequest::new(query);
        let search_results = magneto
            .search(search_request)
            .await
            .map_err(TrackerError::SearchFailed)?;

        let mut added_torrents = Vec::new();

        for (i, torrent_info) in search_results.into_iter().take(limit).enumerate() {
            let info_hash = self.generate_mock_info_hash(&torrent_info.name, i);

            let mock_data = MockTorrentData {
                info_hash,
                name: torrent_info.name,
                size: if torrent_info.size_bytes == 0 {
                    1_073_741_824
                } else {
                    torrent_info.size_bytes
                }, // Default 1GB if 0
                seeders: self.rng.random_range(1..=50),
                leechers: self.rng.random_range(0..=20),
                last_announce: SystemTime::now(),
            };

            self.add_torrent(mock_data.clone());
            added_torrents.push(mock_data);
        }

        Ok(added_torrents)
    }

    /// Simulate tracker announce request with enhanced torrent-specific data.
    ///
    /// # Errors
    /// - `TrackerError::ConnectionFailed` - Simulated connection failure based on failure rate
    /// - `TrackerError::TorrentNotFound` - Info hash not registered with tracker
    pub async fn announce(
        &mut self,
        info_hash: InfoHash,
    ) -> Result<AnnounceResponse, TrackerError> {
        // Simulate network delay
        let delay = self.rng.random_range(20..=200);
        tokio::time::sleep(Duration::from_millis(delay)).await;

        // Simulate failure if configured
        if self.rng.random_bool(self.failure_rate as f64) {
            return Err(TrackerError::ConnectionFailed);
        }

        // Check if torrent is known to tracker
        if let Some(torrent_data) = self.torrents.get_mut(&info_hash) {
            // Update last announce time
            torrent_data.last_announce = SystemTime::now();

            // Add some variation to peer counts over time
            let time_factor = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or(Duration::from_secs(0))
                .as_secs()
                % 100;

            let dynamic_seeders = torrent_data.seeders + (time_factor % 5) as u32;
            let dynamic_leechers = torrent_data.leechers + (time_factor % 3) as u32;
            let base_seeders = torrent_data.seeders;
            let torrent_name = torrent_data.name.clone();
            let torrent_size = torrent_data.size;

            // Generate peer list after borrowing ends
            let peer_count = dynamic_seeders + dynamic_leechers;
            let peers = self.generate_peer_list(peer_count);
            let downloaded = base_seeders + self.rng.random_range(0..=100);

            Ok(AnnounceResponse {
                interval: 1800,
                seeders: dynamic_seeders,
                leechers: dynamic_leechers,
                peers,
                complete: dynamic_seeders,
                incomplete: dynamic_leechers,
                downloaded,
                torrent_name: Some(torrent_name),
                torrent_size: Some(torrent_size),
            })
        } else {
            // Return default response for unknown torrents
            Ok(AnnounceResponse {
                interval: 1800,
                seeders: self.seeders,
                leechers: self.leechers,
                peers: self.peers.clone(),
                complete: self.seeders,
                incomplete: self.leechers,
                downloaded: self.seeders + self.rng.random_range(0..=50),
                torrent_name: None,
                torrent_size: None,
            })
        }
    }

    /// Simulates magnet link discovery for a given query.
    ///
    /// # Errors
    /// - `TrackerError::MagnetoNotEnabled` - Magneto client not configured
    /// - `TrackerError::SearchFailed` - Magneto search failed
    pub async fn discover_magnet_links(
        &mut self,
        query: &str,
        limit: usize,
    ) -> Result<Vec<String>, TrackerError> {
        let magneto = self
            .magneto_client
            .as_mut()
            .ok_or(TrackerError::MagnetoNotEnabled)?;

        let search_request = SearchRequest::new(query);
        let search_results = magneto
            .search(search_request)
            .await
            .map_err(TrackerError::SearchFailed)?;

        let magnet_links = search_results
            .into_iter()
            .take(limit)
            .map(|torrent| torrent.magnet_link)
            .collect();

        Ok(magnet_links)
    }

    /// Returns list of all tracked torrents.
    pub fn tracked_torrents(&self) -> Vec<&MockTorrentData> {
        self.torrents.values().collect()
    }

    /// Generates mock info hash for simulation.
    fn generate_mock_info_hash(&mut self, name: &str, index: usize) -> InfoHash {
        use sha1::{Digest, Sha1};

        let mut hasher = Sha1::new();
        hasher.update(name.as_bytes());
        hasher.update(index.to_le_bytes());
        hasher.update(self.rng.random::<u32>().to_le_bytes());

        let hash = hasher.finalize();
        let mut info_hash = [0u8; 20];
        info_hash.copy_from_slice(&hash[..20]);

        InfoHash::new(info_hash)
    }

    /// Generates realistic peer list for given count.
    fn generate_peer_list(&mut self, count: u32) -> Vec<SocketAddr> {
        let mut peers = Vec::new();

        for i in 0..count {
            // Generate diverse IP ranges for realism
            let ip = match i % 4 {
                0 => format!(
                    "192.168.{}.{}",
                    self.rng.random_range(1..=255),
                    self.rng.random_range(1..=254)
                ),
                1 => format!(
                    "10.0.{}.{}",
                    self.rng.random_range(1..=255),
                    self.rng.random_range(1..=254)
                ),
                2 => format!(
                    "172.16.{}.{}",
                    self.rng.random_range(1..=255),
                    self.rng.random_range(1..=254)
                ),
                _ => format!(
                    "203.{}.{}.{}",
                    self.rng.random_range(1..=255),
                    self.rng.random_range(1..=255),
                    self.rng.random_range(1..=254)
                ),
            };

            let port = self.rng.random_range(6881..=6999);

            if let Ok(addr) = format!("{ip}:{port}").parse() {
                peers.push(addr);
            }
        }

        peers
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
    enable_magneto: bool,
    seed: u64,
}

impl MockTrackerBuilder {
    fn new() -> Self {
        Self {
            seeders: 10,
            leechers: 5,
            failure_rate: 0.0,
            enable_magneto: false,
            seed: 42,
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

    /// Enables magneto integration for magnet link discovery.
    pub fn with_magneto(mut self, enable: bool) -> Self {
        self.enable_magneto = enable;
        self
    }

    /// Sets random seed for deterministic behavior.
    pub fn with_seed(mut self, seed: u64) -> Self {
        self.seed = seed;
        self
    }

    /// Creates mock tracker with configured settings.
    pub fn build(self) -> MockTracker {
        let rng = ChaCha8Rng::seed_from_u64(self.seed);
        let mut peers = Vec::new();

        // Generate realistic peer addresses
        for i in 0..(self.seeders + self.leechers) {
            let ip = format!("192.168.1.{}", 100 + (i % 50));
            let port = 6881 + (i % 100) as u16;
            if let Ok(addr) = format!("{ip}:{port}").parse() {
                peers.push(addr);
            }
        }

        MockTracker {
            seeders: self.seeders,
            leechers: self.leechers,
            failure_rate: self.failure_rate,
            peers,
            torrents: HashMap::new(),
            rng,
            magneto_client: if self.enable_magneto {
                let provider = MockMagnetoProviderBuilder::new()
                    .with_seed(self.seed)
                    .build();
                Some(create_mock_magneto_client(provider))
            } else {
                None
            },
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
    pub complete: u32,
    pub incomplete: u32,
    pub downloaded: u32,
    pub torrent_name: Option<String>,
    pub torrent_size: Option<u64>,
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

    #[error("Torrent not found in tracker database")]
    TorrentNotFound,

    #[error("Magneto client not enabled")]
    MagnetoNotEnabled,

    #[error("Magneto search failed")]
    SearchFailed(#[from] ClientError),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_mock_tracker_successful_announce() {
        let mut tracker = MockTracker::builder()
            .with_seeders(5)
            .with_leechers(3)
            .with_seed(123)
            .build();

        let info_hash = InfoHash::new([0u8; 20]);
        let response = tracker.announce(info_hash).await.unwrap();

        assert_eq!(response.seeders, 5);
        assert_eq!(response.leechers, 3);
        assert_eq!(response.interval, 1800);
        assert!(response.peers.len() >= 8);
    }

    #[tokio::test]
    async fn test_mock_tracker_failure_injection() {
        let mut tracker = MockTracker::builder()
            .with_failure_rate(1.0) // Always fail
            .with_seed(456)
            .build();

        let info_hash = InfoHash::new([0u8; 20]);
        let result = tracker.announce(info_hash).await;

        assert!(result.is_err());
        matches!(result.unwrap_err(), TrackerError::ConnectionFailed);
    }

    #[test]
    fn test_tracker_builder_generates_valid_peers() {
        let tracker = MockTracker::builder()
            .with_seeders(2)
            .with_leechers(1)
            .with_seed(789)
            .build();

        assert_eq!(tracker.peers.len(), 3);

        // Verify all peers have valid socket addresses
        for peer in &tracker.peers {
            assert!(peer.ip().is_ipv4());
            assert!(peer.port() >= 6881);
        }
    }

    #[tokio::test]
    async fn test_mock_tracker_with_torrent_data() {
        let mut tracker = MockTracker::builder().with_seed(101112).build();

        // Add a mock torrent
        let info_hash = InfoHash::new([1u8; 20]);
        let torrent_data = MockTorrentData {
            info_hash,
            name: "Test Movie 2024".to_string(),
            size: 2_000_000_000, // 2GB
            seeders: 15,
            leechers: 8,
            last_announce: SystemTime::now(),
        };

        tracker.add_torrent(torrent_data);

        // Test announce for known torrent
        let response = tracker.announce(info_hash).await.unwrap();
        assert_eq!(response.torrent_name, Some("Test Movie 2024".to_string()));
        assert_eq!(response.torrent_size, Some(2_000_000_000));
        assert!(response.seeders >= 15); // May have dynamic variation
    }

    #[test]
    fn test_tracker_builder_with_magneto() {
        let tracker = MockTracker::builder()
            .with_magneto(true)
            .with_seed(131415)
            .build();

        assert!(tracker.magneto_client.is_some());
    }
}
