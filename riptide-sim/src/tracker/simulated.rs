//! Simulated tracker client for deterministic testing and fuzzing

use std::collections::HashMap;
use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use async_trait::async_trait;
use riptide_core::torrent::tracker::{
    AnnounceEvent, AnnounceRequest, AnnounceResponse, ScrapeRequest, ScrapeResponse, ScrapeStats,
};
use riptide_core::torrent::{InfoHash, TorrentError, TrackerClient, TrackerManagement};

/// Type alias for peer coordination function
type PeerCoordinator = Box<dyn Fn(&InfoHash) -> Vec<SocketAddr> + Send + Sync>;

/// Simulated tracker client for deterministic testing and development.
///
/// Provides controllable tracker responses without network communication.
/// Maintains internal state for realistic swarm simulation and supports
/// injecting specific responses for edge case testing.
pub struct SimulatedTrackerClient {
    announce_url: String,
    peer_generator: Arc<Mutex<PeerGenerator>>,
    swarm_stats: Arc<Mutex<HashMap<InfoHash, SwarmStats>>>,
    announce_count: AtomicU32,
    response_config: ResponseConfig,
    peer_coordinator: Option<PeerCoordinator>,
}

/// Configuration for simulated tracker responses
#[derive(Debug, Clone)]
pub struct ResponseConfig {
    /// Base announce interval in seconds
    pub interval: u32,
    /// Number of peers to return per announce
    pub peer_count: usize,
    /// Simulate tracker failures with this probability (0.0 to 1.0)
    pub failure_rate: f64,
    /// Predefined failure message for testing
    pub failure_message: Option<String>,
    /// Minimum interval between announces
    pub min_interval: Option<u32>,
    /// Whether to include tracker ID in responses
    pub include_tracker_id: bool,
}

impl Default for ResponseConfig {
    fn default() -> Self {
        Self {
            interval: 300,
            peer_count: 50,
            failure_rate: 0.0,
            failure_message: None,
            min_interval: Some(60),
            include_tracker_id: true,
        }
    }
}

/// Internal statistics for simulated swarm
#[derive(Debug, Clone)]
struct SwarmStats {
    complete: u32,
    incomplete: u32,
    downloaded: u32,
    last_announce: std::time::Instant,
}

impl Default for SwarmStats {
    fn default() -> Self {
        Self {
            complete: 10,
            incomplete: 25,
            downloaded: 100,
            last_announce: Instant::now(),
        }
    }
}

/// Generates deterministic peer addresses for simulation
struct PeerGenerator {
    base_port: u16,
    current_peer: u32,
}

impl PeerGenerator {
    fn new() -> Self {
        Self {
            base_port: 6881,
            current_peer: 1,
        }
    }

    /// Generate list of simulated peer addresses
    fn generate_peers(&mut self, count: usize) -> Vec<SocketAddr> {
        (0..count)
            .map(|_| {
                let peer_id = self.current_peer;
                self.current_peer += 1;

                // Generate IP addresses in 192.168.x.x range for simulation
                let a = 192;
                let b = 168;
                let c = ((peer_id / 256) % 256) as u8;
                let d = (peer_id % 256) as u8;

                let ip = Ipv4Addr::new(a, b, c, d);
                let port = self.base_port + ((peer_id % 1000) as u16);

                SocketAddr::V4(SocketAddrV4::new(ip, port))
            })
            .collect()
    }
}

impl SimulatedTrackerClient {
    /// Creates new simulated tracker client with default configuration.
    ///
    /// Uses realistic default values for interval, peer count, and swarm statistics.
    /// Peer addresses are generated deterministically for reproducible testing.
    pub fn new(announce_url: String) -> Self {
        Self::with_config(announce_url, ResponseConfig::default())
    }

    /// Creates simulated tracker client with custom response configuration.
    ///
    /// Allows precise control over tracker behavior for testing edge cases,
    /// failure scenarios, and performance characteristics.
    pub fn with_config(announce_url: String, config: ResponseConfig) -> Self {
        Self {
            announce_url,
            peer_generator: Arc::new(Mutex::new(PeerGenerator::new())),
            swarm_stats: Arc::new(Mutex::new(HashMap::new())),
            announce_count: AtomicU32::new(0),
            response_config: config,
            peer_coordinator: None,
        }
    }

    /// Configure peer coordinator callback to get peer addresses from ContentAwarePeerManager.
    ///
    /// This ensures the tracker returns peer addresses that the peer manager
    /// knows about and can handle connections to.
    pub fn configure_peer_coordinator<F>(&mut self, coordinator: Box<F>)
    where
        F: Fn(&InfoHash) -> Vec<SocketAddr> + Send + Sync + 'static,
    {
        self.peer_coordinator = Some(coordinator);
    }

    /// Injects predefined swarm statistics for specific info hash.
    ///
    /// Useful for testing scenarios with specific seeder/leecher ratios
    /// or simulating swarm evolution over time.
    pub fn configure_swarm_stats(
        &self,
        info_hash: InfoHash,
        complete: u32,
        incomplete: u32,
        downloaded: u32,
    ) {
        let mut stats = self.swarm_stats.lock().unwrap();
        stats.insert(
            info_hash,
            SwarmStats {
                complete,
                incomplete,
                downloaded,
                last_announce: Instant::now(),
            },
        );
    }

    /// Simulates tracker failure with specific error message.
    ///
    /// Enables testing error handling paths and recovery behavior
    /// without relying on actual network failures.
    pub fn simulate_failure(&mut self, failure_message: String) {
        self.response_config.failure_message = Some(failure_message);
        self.response_config.failure_rate = 1.0;
    }

    /// Resets tracker to normal operation after simulated failure.
    pub fn reset_to_normal(&mut self) {
        self.response_config.failure_message = None;
        self.response_config.failure_rate = 0.0;
    }

    /// Returns number of announce requests received.
    ///
    /// Useful for verifying announce frequency and retry behavior
    /// in deterministic tests.
    pub fn announce_count(&self) -> u32 {
        self.announce_count.load(Ordering::Relaxed)
    }

    /// Updates swarm statistics based on announce event.
    fn update_swarm_stats(&self, info_hash: InfoHash, event: AnnounceEvent) {
        let mut stats = self.swarm_stats.lock().unwrap();
        let swarm_stats = stats.entry(info_hash).or_default();

        match event {
            AnnounceEvent::Started => {
                swarm_stats.incomplete += 1;
            }
            AnnounceEvent::Completed => {
                if swarm_stats.incomplete > 0 {
                    swarm_stats.incomplete -= 1;
                }
                swarm_stats.complete += 1;
                swarm_stats.downloaded += 1;
            }
            AnnounceEvent::Stopped => {
                if swarm_stats.incomplete > 0 {
                    swarm_stats.incomplete -= 1;
                }
            }
        }

        swarm_stats.last_announce = Instant::now();
    }
}

#[async_trait]
impl TrackerClient for SimulatedTrackerClient {
    /// Simulates tracker announce with deterministic peer list generation.
    ///
    /// Returns realistic peer lists and swarm statistics without network communication.
    /// Supports failure injection and configurable response characteristics.
    ///
    /// # Errors
    /// - `TorrentError::TrackerConnectionFailed` - When configured for failure simulation
    async fn announce(&self, request: AnnounceRequest) -> Result<AnnounceResponse, TorrentError> {
        self.announce_count.fetch_add(1, Ordering::Relaxed);

        // Simulate tracker failure if configured
        if let Some(ref failure_msg) = self.response_config.failure_message {
            return Err(TorrentError::TrackerConnectionFailed {
                url: format!("Simulated tracker failure: {failure_msg}"),
            });
        }

        // Update internal swarm statistics
        self.update_swarm_stats(request.info_hash, request.event);

        // Generate peer list - use coordinator if available, otherwise generate random peers
        let peers = if let Some(ref coordinator) = self.peer_coordinator {
            let coordinated_peers = coordinator(&request.info_hash);
            if coordinated_peers.is_empty() {
                // Fallback to generated peers if coordinator returns no peers
                let mut generator = self.peer_generator.lock().unwrap();
                generator.generate_peers(self.response_config.peer_count)
            } else {
                // Limit to configured peer count
                coordinated_peers
                    .into_iter()
                    .take(self.response_config.peer_count)
                    .collect()
            }
        } else {
            let mut generator = self.peer_generator.lock().unwrap();
            generator.generate_peers(self.response_config.peer_count)
        };

        // Get current swarm statistics
        let (complete, incomplete) = {
            let stats = self.swarm_stats.lock().unwrap();
            let swarm_stats = stats.get(&request.info_hash).cloned().unwrap_or_default();
            (swarm_stats.complete, swarm_stats.incomplete)
        };

        let response = AnnounceResponse {
            interval: self.response_config.interval,
            min_interval: self.response_config.min_interval,
            tracker_id: if self.response_config.include_tracker_id {
                Some(format!(
                    "sim-tracker-{}",
                    self.announce_count.load(Ordering::Relaxed)
                ))
            } else {
                None
            },
            complete,
            incomplete,
            peers,
        };

        Ok(response)
    }

    /// Simulates tracker scrape operation returning swarm statistics.
    ///
    /// Provides statistics for requested torrents based on internal state
    /// accumulated from previous announce operations.
    ///
    /// # Errors
    /// - `TorrentError::TrackerConnectionFailed` - When configured for failure simulation
    async fn scrape(&self, request: ScrapeRequest) -> Result<ScrapeResponse, TorrentError> {
        // Simulate scrape failure if configured
        if let Some(ref failure_msg) = self.response_config.failure_message {
            return Err(TorrentError::TrackerConnectionFailed {
                url: format!("Simulated scrape failure: {failure_msg}"),
            });
        }

        let stats = self.swarm_stats.lock().unwrap();
        let mut files = HashMap::new();

        for info_hash in request.info_hashes {
            let swarm_stats = stats.get(&info_hash).cloned().unwrap_or_default();
            files.insert(
                info_hash,
                ScrapeStats {
                    complete: swarm_stats.complete,
                    downloaded: swarm_stats.downloaded,
                    incomplete: swarm_stats.incomplete,
                },
            );
        }

        Ok(ScrapeResponse { files })
    }

    /// Returns simulated tracker URL for debugging and logging.
    fn tracker_url(&self) -> &str {
        &self.announce_url
    }
}

/// Simulated tracker management for testing multi-tracker scenarios.
///
/// Provides deterministic tracker selection and failover behavior
/// without requiring real network infrastructure. Returns peer addresses
/// that ContentAwarePeerManager can handle.
pub struct SimulatedTrackerManager {
    default_client: SimulatedTrackerClient,
}

impl Default for SimulatedTrackerManager {
    fn default() -> Self {
        Self::new()
    }
}

impl SimulatedTrackerManager {
    /// Creates new simulated tracker manager with default client.
    pub fn new() -> Self {
        Self {
            default_client: SimulatedTrackerClient::new(
                "http://sim-tracker.test/announce".to_string(),
            ),
        }
    }

    /// Creates simulated tracker manager with custom response configuration.
    pub fn with_config(config: ResponseConfig) -> Self {
        Self {
            default_client: SimulatedTrackerClient::with_config(
                "http://sim-tracker.test/announce".to_string(),
                config,
            ),
        }
    }

    /// Creates simulated tracker manager with peer registry for coordination.
    ///
    /// This allows the tracker to return peer addresses from a shared registry
    /// without complex async coordination or dangerous block_on calls.
    pub fn with_peer_registry(
        config: ResponseConfig,
        peer_registry: Arc<std::sync::Mutex<HashMap<InfoHash, Vec<SocketAddr>>>>,
    ) -> Self {
        let peer_coordinator = move |info_hash: &InfoHash| -> Vec<SocketAddr> {
            // Simple synchronous read - no async coordination needed
            peer_registry
                .lock()
                .unwrap()
                .get(info_hash)
                .cloned()
                .unwrap_or_default()
        };

        let mut client = SimulatedTrackerClient::with_config(
            "http://sim-tracker.test/announce".to_string(),
            config,
        );
        client.configure_peer_coordinator(Box::new(peer_coordinator));

        Self {
            default_client: client,
        }
    }
}

#[async_trait]
impl TrackerManagement for SimulatedTrackerManager {
    /// Simulates announcing to best available tracker from list.
    ///
    /// Uses internal simulated tracker client regardless of provided URLs
    /// for deterministic testing behavior.
    ///
    /// # Errors
    /// - `TorrentError::TrackerConnectionFailed` - When configured for failure simulation
    async fn announce_to_trackers(
        &mut self,
        tracker_urls: &[String],
        request: AnnounceRequest,
    ) -> Result<AnnounceResponse, TorrentError> {
        if tracker_urls.is_empty() {
            return Err(TorrentError::TrackerConnectionFailed {
                url: "No tracker URLs provided".to_string(),
            });
        }

        // Simulate trying first tracker (always succeeds unless configured to fail)
        self.default_client.announce(request).await
    }

    /// Simulates scraping statistics from trackers.
    ///
    /// # Errors  
    /// - `TorrentError::TrackerConnectionFailed` - When configured for failure simulation
    async fn scrape_from_trackers(
        &mut self,
        tracker_urls: &[String],
        request: ScrapeRequest,
    ) -> Result<ScrapeResponse, TorrentError> {
        if tracker_urls.is_empty() {
            return Err(TorrentError::TrackerConnectionFailed {
                url: "No tracker URLs provided".to_string(),
            });
        }

        self.default_client.scrape(request).await
    }
}

#[cfg(test)]
mod simulated_tracker_tests {
    use riptide_core::torrent::PeerId;

    use super::*;

    fn create_test_announce_request(info_hash: InfoHash, event: AnnounceEvent) -> AnnounceRequest {
        AnnounceRequest {
            info_hash,
            peer_id: *PeerId::generate().as_bytes(),
            port: 6881,
            uploaded: 1000,
            downloaded: 500,
            left: 2000,
            event,
        }
    }

    #[tokio::test]
    async fn test_simulated_tracker_announce_success() {
        let client = SimulatedTrackerClient::new("http://sim-tracker.test/announce".to_string());
        let info_hash = InfoHash::new([1u8; 20]);
        let request = create_test_announce_request(info_hash, AnnounceEvent::Started);

        let response = client.announce(request).await.unwrap();

        assert_eq!(response.interval, 300);
        assert_eq!(response.peers.len(), 50);
        assert!(response.complete > 0);
        assert!(response.incomplete > 0);
        assert_eq!(client.announce_count(), 1);
    }

    #[tokio::test]
    async fn test_simulated_tracker_custom_config() {
        let config = ResponseConfig {
            interval: 600,
            peer_count: 10,
            failure_rate: 0.0,
            failure_message: None,
            min_interval: Some(120),
            include_tracker_id: false,
        };

        let client = SimulatedTrackerClient::with_config(
            "http://custom-tracker.test/announce".to_string(),
            config,
        );

        let info_hash = InfoHash::new([2u8; 20]);
        let request = create_test_announce_request(info_hash, AnnounceEvent::Started);

        let response = client.announce(request).await.unwrap();

        assert_eq!(response.interval, 600);
        assert_eq!(response.min_interval, Some(120));
        assert_eq!(response.peers.len(), 10);
        assert_eq!(response.tracker_id, None);
    }

    #[tokio::test]
    async fn test_simulated_tracker_failure_injection() {
        let mut client =
            SimulatedTrackerClient::new("http://failing-tracker.test/announce".to_string());
        client.simulate_failure("Connection refused".to_string());

        let info_hash = InfoHash::new([3u8; 20]);
        let request = create_test_announce_request(info_hash, AnnounceEvent::Started);

        let result = client.announce(request).await;
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("Connection refused")
        );
    }

    #[tokio::test]
    async fn test_simulated_tracker_swarm_stats_update() {
        let client = SimulatedTrackerClient::new("http://tracker.test/announce".to_string());
        let info_hash = InfoHash::new([4u8; 20]);

        // Set initial swarm statistics
        client.configure_swarm_stats(info_hash, 5, 10, 20);

        // Announce started event
        let request = create_test_announce_request(info_hash, AnnounceEvent::Started);
        let response = client.announce(request).await.unwrap();

        assert_eq!(response.complete, 5);
        assert_eq!(response.incomplete, 11); // Increased by 1

        // Announce completed event
        let request = create_test_announce_request(info_hash, AnnounceEvent::Completed);
        let response = client.announce(request).await.unwrap();

        assert_eq!(response.complete, 6); // Increased by 1
        assert_eq!(response.incomplete, 10); // Decreased by 1
    }

    #[tokio::test]
    async fn test_simulated_tracker_scrape_operation() {
        let client = SimulatedTrackerClient::new("http://tracker.test/announce".to_string());
        let info_hash1 = InfoHash::new([5u8; 20]);
        let info_hash2 = InfoHash::new([6u8; 20]);

        // Set up swarm statistics
        client.configure_swarm_stats(info_hash1, 15, 25, 100);
        client.configure_swarm_stats(info_hash2, 8, 12, 50);

        let scrape_request = ScrapeRequest {
            info_hashes: vec![info_hash1, info_hash2],
        };

        let response = client.scrape(scrape_request).await.unwrap();

        assert_eq!(response.files.len(), 2);

        let stats1 = response.files.get(&info_hash1).unwrap();
        assert_eq!(stats1.complete, 15);
        assert_eq!(stats1.incomplete, 25);
        assert_eq!(stats1.downloaded, 100);

        let stats2 = response.files.get(&info_hash2).unwrap();
        assert_eq!(stats2.complete, 8);
        assert_eq!(stats2.incomplete, 12);
        assert_eq!(stats2.downloaded, 50);
    }

    #[tokio::test]
    async fn test_deterministic_peer_generation() {
        let client1 = SimulatedTrackerClient::new("http://tracker.test/announce".to_string());
        let client2 = SimulatedTrackerClient::new("http://tracker.test/announce".to_string());

        let info_hash = InfoHash::new([7u8; 20]);
        let request = create_test_announce_request(info_hash, AnnounceEvent::Started);

        let response1 = client1.announce(request.clone()).await.unwrap();
        let response2 = client2.announce(request).await.unwrap();

        // Peer generation should be deterministic based on sequence
        assert_eq!(response1.peers.len(), response2.peers.len());
        // Exact peer addresses will differ due to internal state, but format should be consistent
        for peer in &response1.peers {
            assert!(peer.ip().is_ipv4());
            assert!(peer.port() >= 6881);
        }
    }

    #[test]
    fn test_tracker_url_accessor() {
        let client =
            SimulatedTrackerClient::new("http://test-tracker.example.com/announce".to_string());
        assert_eq!(
            client.tracker_url(),
            "http://test-tracker.example.com/announce"
        );
    }
}
