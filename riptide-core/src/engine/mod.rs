//! Unified torrent engine with trait-based abstraction
//!
//! Provides production and simulation torrent engine type aliases with
//! a shared trait interface for web handlers.

use async_trait::async_trait;

use super::torrent::protocol::PeerId;
use super::torrent::{
    EngineStats, InfoHash, NetworkPeerManager, PeerManager, TorrentEngine as CoreTorrentEngine,
    TorrentError, TorrentSession, TrackerManagement, TrackerManager,
};
use crate::config::RiptideConfig;

/// Trait for torrent engine operations regardless of implementation
#[async_trait]
pub trait TorrentEngineOps: Send + Sync {
    /// Add a torrent by magnet link
    async fn add_magnet(&mut self, magnet_link: &str) -> Result<InfoHash, TorrentError>;

    /// Start downloading a torrent
    async fn start_download(&mut self, info_hash: InfoHash) -> Result<(), TorrentError>;

    /// Get all active sessions
    fn active_sessions(&self) -> Vec<&TorrentSession>;

    /// Get count of active sessions
    fn active_session_count(&self) -> usize {
        self.active_sessions().len()
    }

    /// Simulate download progress (for simulation mode)
    fn simulate_download_progress(&mut self);

    /// Get download statistics
    async fn download_stats(&self) -> EngineStats;

    /// Get specific torrent session
    fn session(&self, info_hash: InfoHash) -> Result<&TorrentSession, TorrentError>;
}

/// Implement the trait generically for any TorrentEngine
#[async_trait]
impl<P: PeerManager, T: TrackerManagement> TorrentEngineOps for CoreTorrentEngine<P, T> {
    async fn add_magnet(&mut self, magnet_link: &str) -> Result<InfoHash, TorrentError> {
        self.add_magnet(magnet_link).await
    }

    async fn start_download(&mut self, info_hash: InfoHash) -> Result<(), TorrentError> {
        self.start_download(info_hash).await
    }

    fn active_sessions(&self) -> Vec<&TorrentSession> {
        self.active_sessions()
    }

    fn simulate_download_progress(&mut self) {
        self.simulate_download_progress()
    }

    async fn download_stats(&self) -> EngineStats {
        self.get_download_stats().await
    }

    fn session(&self, info_hash: InfoHash) -> Result<&TorrentSession, TorrentError> {
        self.get_session(info_hash)
    }
}

/// Production torrent engine using real network operations
pub type ProductionTorrentEngine = CoreTorrentEngine<NetworkPeerManager, TrackerManager>;

impl ProductionTorrentEngine {
    /// Create production engine with real HTTP client and TCP peer connections
    pub fn new_production(config: RiptideConfig) -> Self {
        let peer_id = PeerId::generate();
        let peer_manager = NetworkPeerManager::new(peer_id, config.network.max_peer_connections);
        let tracker_manager = TrackerManager::new(config.network.clone());
        Self::new(config, peer_manager, tracker_manager)
    }
}
