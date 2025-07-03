//! Mock implementations for testing the torrent engine.

use std::any::Any;
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;

use async_trait::async_trait;
use tokio::sync::RwLock;

use crate::torrent::tracker::{
    AnnounceRequest, AnnounceResponse, ScrapeRequest, ScrapeResponse, TrackerManagement,
};
use crate::torrent::{
    InfoHash, PeerId, PeerInfo, PeerManager, PeerMessage, PeerMessageEvent, TorrentError,
};

// Test timing constants
const MOCK_NETWORK_DELAY_MS: u64 = 50;
const MOCK_WAIT_PERIOD_MS: u64 = 100;

/// Mock peer manager for testing.
#[derive(Debug, Clone)]
pub struct MockPeerManager {
    connected_peers: Arc<RwLock<HashMap<InfoHash, Vec<SocketAddr>>>>,
    should_fail_connection: bool,
    pending_requests: Arc<RwLock<Vec<(SocketAddr, PeerMessage)>>>,
    simulate_piece_data: bool,
}

impl MockPeerManager {
    /// Creates a new mock peer manager.
    pub fn new() -> Self {
        Self {
            connected_peers: Arc::new(RwLock::new(HashMap::new())),
            should_fail_connection: false,
            pending_requests: Arc::new(RwLock::new(Vec::new())),
            simulate_piece_data: true,
        }
    }

    /// Creates a mock peer manager that fails connection attempts.
    pub fn new_with_connection_failure() -> Self {
        Self {
            connected_peers: Arc::new(RwLock::new(HashMap::new())),
            should_fail_connection: true,
            pending_requests: Arc::new(RwLock::new(Vec::new())),
            simulate_piece_data: false,
        }
    }

    /// Adds a mock peer for testing.
    pub async fn add_mock_peer(&self, info_hash: InfoHash, peer_address: SocketAddr) {
        let mut peers = self.connected_peers.write().await;
        peers.entry(info_hash).or_default().push(peer_address);
    }

    /// Enables piece data simulation for testing piece downloading.
    pub fn enable_piece_data_simulation(&mut self) {
        self.simulate_piece_data = true;
    }
}

impl Default for MockPeerManager {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl PeerManager for MockPeerManager {
    async fn connect_peer(
        &mut self,
        address: SocketAddr,
        info_hash: InfoHash,
        _peer_id: PeerId,
    ) -> Result<(), TorrentError> {
        if self.should_fail_connection {
            return Err(TorrentError::PeerConnectionError {
                reason: "Mock connection failure".to_string(),
            });
        }

        self.add_mock_peer(info_hash, address).await;
        Ok(())
    }

    async fn disconnect_peer(&mut self, _address: SocketAddr) -> Result<(), TorrentError> {
        // Mock implementation - no actual disconnection needed
        Ok(())
    }

    async fn send_message(
        &mut self,
        peer_address: SocketAddr,
        message: PeerMessage,
    ) -> Result<(), TorrentError> {
        if !self.simulate_piece_data {
            return Ok(()); // Just succeed without responses
        }

        // Store the request for later response generation
        let mut requests = self.pending_requests.write().await;
        requests.push((peer_address, message));
        Ok(())
    }

    async fn receive_message(&mut self) -> Result<PeerMessageEvent, TorrentError> {
        if !self.simulate_piece_data {
            return Err(TorrentError::PeerConnectionError {
                reason: "No messages available in mock".to_string(),
            });
        }

        // Check for pending requests and generate responses
        let mut requests = self.pending_requests.write().await;
        if let Some((peer_address, message)) = requests.pop() {
            // Simulate realistic network delay for piece responses
            tokio::time::sleep(std::time::Duration::from_millis(MOCK_NETWORK_DELAY_MS)).await;

            match message {
                PeerMessage::Request {
                    piece_index,
                    offset,
                    length,
                } => {
                    // Generate deterministic mock piece data that will pass hash verification
                    let mock_data = vec![piece_index.as_u32() as u8; length as usize];

                    Ok(PeerMessageEvent {
                        peer_address,
                        message: PeerMessage::Piece {
                            piece_index,
                            offset,
                            data: bytes::Bytes::from(mock_data),
                        },
                        received_at: std::time::Instant::now(),
                    })
                }
                _ => {
                    // For other messages, just return a keep-alive
                    Ok(PeerMessageEvent {
                        peer_address,
                        message: PeerMessage::KeepAlive,
                        received_at: std::time::Instant::now(),
                    })
                }
            }
        } else {
            // No pending requests - simulate realistic waiting period
            tokio::time::sleep(std::time::Duration::from_millis(MOCK_WAIT_PERIOD_MS)).await;
            Err(TorrentError::PeerConnectionError {
                reason: "No pending requests available".to_string(),
            })
        }
    }

    async fn connected_peers(&self) -> Vec<PeerInfo> {
        // Mock implementation - return empty list
        vec![]
    }

    async fn connection_count(&self) -> usize {
        let peers = self.connected_peers.read().await;
        peers.values().map(|v| v.len()).sum()
    }

    async fn shutdown(&mut self) -> Result<(), TorrentError> {
        // Mock implementation - clear connections
        let mut peers = self.connected_peers.write().await;
        peers.clear();
        Ok(())
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

/// Mock tracker manager for testing.
#[derive(Debug, Clone)]
pub struct MockTrackerManager {
    should_fail_announce: bool,
    mock_peers: Vec<SocketAddr>,
}

impl MockTrackerManager {
    /// Creates a new mock tracker manager.
    pub fn new() -> Self {
        Self {
            should_fail_announce: false,
            mock_peers: vec![
                "127.0.0.1:6881".parse().unwrap(),
                "127.0.0.1:6882".parse().unwrap(),
            ],
        }
    }

    /// Creates a mock tracker manager that fails announce requests.
    pub fn new_with_announce_failure() -> Self {
        Self {
            should_fail_announce: true,
            mock_peers: vec![],
        }
    }

    /// Sets the mock peers that will be returned by announce requests.
    pub fn set_mock_peers(&mut self, peers: Vec<SocketAddr>) {
        self.mock_peers = peers;
    }
}

impl Default for MockTrackerManager {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl TrackerManagement for MockTrackerManager {
    async fn announce_to_trackers(
        &mut self,
        tracker_urls: &[String],
        _request: AnnounceRequest,
    ) -> Result<AnnounceResponse, TorrentError> {
        if self.should_fail_announce {
            return Err(TorrentError::TrackerConnectionFailed {
                url: tracker_urls
                    .first()
                    .unwrap_or(&"unknown".to_string())
                    .clone(),
            });
        }

        Ok(AnnounceResponse {
            interval: 300,
            min_interval: Some(60),
            tracker_id: Some("mock_tracker".to_string()),
            complete: 10,
            incomplete: 5,
            peers: self.mock_peers.clone(),
        })
    }

    async fn scrape_from_trackers(
        &mut self,
        _tracker_urls: &[String],
        _request: ScrapeRequest,
    ) -> Result<ScrapeResponse, TorrentError> {
        // Mock implementation - return empty scrape results
        Ok(ScrapeResponse {
            files: HashMap::new(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_mock_peer_manager_basic_operations() {
        let mut manager = MockPeerManager::new();
        let info_hash = InfoHash::new([1u8; 20]);
        let peer_addr: SocketAddr = "127.0.0.1:6881".parse().unwrap();
        let peer_id = PeerId::generate();

        // Test connection
        let result = manager.connect_peer(peer_addr, info_hash, peer_id).await;
        assert!(result.is_ok());

        // Test peer count
        let count = manager.connection_count().await;
        assert_eq!(count, 1);

        // Test getting connected peers
        let peers = manager.connected_peers().await;
        assert_eq!(peers.len(), 0); // Mock returns empty list
    }

    #[tokio::test]
    async fn test_mock_peer_manager_connection_failure() {
        let mut manager = MockPeerManager::new_with_connection_failure();
        let info_hash = InfoHash::new([1u8; 20]);
        let peer_addr: SocketAddr = "127.0.0.1:6881".parse().unwrap();
        let peer_id = PeerId::generate();

        // Test connection failure
        let result = manager.connect_peer(peer_addr, info_hash, peer_id).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_mock_tracker_manager_announce() {
        let mut manager = MockTrackerManager::new();
        let request = AnnounceRequest {
            info_hash: InfoHash::new([1u8; 20]),
            peer_id: *PeerId::generate().as_bytes(),
            port: 6881,
            uploaded: 0,
            downloaded: 0,
            left: 1000,
            event: crate::torrent::tracker::AnnounceEvent::Started,
        };

        let result = manager
            .announce_to_trackers(&["http://test-tracker.com/announce".to_string()], request)
            .await;
        assert!(result.is_ok());

        let response = result.unwrap();
        assert_eq!(response.interval, 300);
        assert_eq!(response.peers.len(), 2);
    }

    #[tokio::test]
    async fn test_mock_tracker_manager_announce_failure() {
        let mut manager = MockTrackerManager::new_with_announce_failure();
        let request = AnnounceRequest {
            info_hash: InfoHash::new([1u8; 20]),
            peer_id: *PeerId::generate().as_bytes(),
            port: 6881,
            uploaded: 0,
            downloaded: 0,
            left: 1000,
            event: crate::torrent::tracker::AnnounceEvent::Started,
        };

        let result = manager
            .announce_to_trackers(&["http://test-tracker.com/announce".to_string()], request)
            .await;
        assert!(result.is_err());
    }
}
