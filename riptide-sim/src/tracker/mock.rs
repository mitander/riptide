//! Mock BitTorrent tracker implementation

use std::collections::HashMap;
use std::net::SocketAddr;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use magneto::SearchRequest;
use rand::Rng;
use rand_chacha::ChaCha8Rng;
use riptide_core::torrent::InfoHash;

use super::builder::MockTrackerBuilder;
use super::types::{AnnounceResponse, MockTorrentData, TrackerError};
use crate::magneto_provider::{
    MockMagnetoClient, MockMagnetoProviderBuilder, create_mock_magneto_client,
};

/// Mock tracker for offline development with magneto integration.
///
/// Simulates BitTorrent tracker responses for testing without real trackers.
/// Supports configurable peer counts, failure injection, realistic delays,
/// and integration with magneto for magnet link discovery simulation.
pub struct MockTracker {
    #[allow(dead_code)]
    pub(crate) seeders: u32,
    #[allow(dead_code)]
    pub(crate) leechers: u32,
    pub(crate) failure_rate: f32,
    #[allow(dead_code)]
    pub(crate) peers: Vec<SocketAddr>,
    pub(crate) torrents: HashMap<InfoHash, MockTorrentData>,
    pub(crate) rng: ChaCha8Rng,
    pub(crate) magneto_client: Option<MockMagnetoClient>,
}

impl Default for MockTracker {
    fn default() -> Self {
        Self::new()
    }
}

impl MockTracker {
    /// Creates new mock tracker with default settings.
    pub fn new() -> Self {
        Self::builder().build()
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
            Err(TrackerError::TorrentNotFound)
        }
    }

    /// Generates a unique list of peer addresses for testing.
    pub fn generate_peer_list(&mut self, count: u32) -> Vec<SocketAddr> {
        let mut peers = Vec::new();
        for i in 0..count {
            let ip = format!("192.168.{}.{}", 1 + (i / 100), 100 + (i % 100));
            let port = 6881 + (i % 1000) as u16;
            if let Ok(addr) = format!("{ip}:{port}").parse() {
                peers.push(addr);
            }
        }
        peers
    }

    /// Generates mock info hash from torrent name and index.
    fn generate_mock_info_hash(&mut self, name: &str, index: usize) -> InfoHash {
        // Create deterministic but unique hash from name and index
        let mut hash_bytes = [0u8; 20];
        let name_bytes = name.as_bytes();

        // Fill with name bytes (cycling if needed)
        for (i, byte) in hash_bytes.iter_mut().enumerate() {
            *byte = name_bytes[i % name_bytes.len()];
        }

        // Add index variation
        hash_bytes[0] = (index % 256) as u8;
        hash_bytes[1] = ((index / 256) % 256) as u8;

        // Add some randomness based on RNG state
        for byte in hash_bytes.iter_mut().take(6).skip(2) {
            *byte = self.rng.random();
        }

        InfoHash::new(hash_bytes)
    }
}
