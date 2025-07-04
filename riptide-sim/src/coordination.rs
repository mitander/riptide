//! Coordination between simulated tracker and peer managers
//!
//! Ensures that tracker announce responses return peer addresses that
//! the peer manager knows about and can handle connections to.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;

use riptide_core::torrent::{InfoHash, PieceStore};
use tokio::sync::RwLock;

use crate::tracker::simulated::ResponseConfig;
use crate::{ContentAwarePeerManager, InMemoryPeerConfig, SimulatedTrackerManager};

/// Coordinates simulated tracker and peer manager for realistic simulation.
///
/// Ensures that when a torrent engine announces to the tracker, it receives
/// peer addresses that the peer manager can actually handle connections to.
/// This enables end-to-end torrent downloading in development mode.
pub struct SimulationCoordinator<P: PieceStore> {
    peer_manager: Arc<RwLock<ContentAwarePeerManager<P>>>,
    tracker_manager: SimulatedTrackerManager,
    seeded_torrents: Arc<RwLock<HashMap<InfoHash, Vec<SocketAddr>>>>,
}

impl<P: PieceStore + 'static> SimulationCoordinator<P> {
    /// Creates a new simulation coordinator with connected managers.
    ///
    /// The tracker manager will be configured to use the peer manager's
    /// peer addresses in announce responses.
    pub async fn new(
        peer_config: InMemoryPeerConfig,
        tracker_config: ResponseConfig,
        piece_store: Arc<P>,
    ) -> Self {
        let peer_manager = Arc::new(RwLock::new(ContentAwarePeerManager::new(
            peer_config,
            piece_store,
        )));
        let seeded_torrents: Arc<RwLock<HashMap<InfoHash, Vec<SocketAddr>>>> =
            Arc::new(RwLock::new(HashMap::new()));

        // Create coordinator closure for tracker
        let peer_manager_ref = peer_manager.clone();
        let seeded_torrents_ref = seeded_torrents.clone();
        let coordinator = move |info_hash: &InfoHash| -> Vec<SocketAddr> {
            // Use blocking call since we can't make async closures
            let runtime = tokio::runtime::Handle::current();
            runtime.block_on(async {
                // First check if we have seeded peers for this torrent
                {
                    let seeded = seeded_torrents_ref.read().await;
                    if let Some(peers) = seeded.get(info_hash) {
                        return peers.clone();
                    }
                }

                // Otherwise get peers from the manager
                let manager = peer_manager_ref.read().await;
                manager.get_peers_for_torrent(*info_hash).await
            })
        };

        let tracker_manager =
            SimulatedTrackerManager::with_peer_coordinator(tracker_config, coordinator);

        Self {
            peer_manager,
            tracker_manager,
            seeded_torrents,
        }
    }

    /// Seeds simulated peers for a torrent.
    ///
    /// Creates content-aware peers that have all pieces available and can
    /// serve them to downloaders. Returns the peer addresses that were created.
    pub async fn seed_torrent(&self, info_hash: InfoHash, peer_count: usize) -> Vec<SocketAddr> {
        let addresses = {
            let mut manager = self.peer_manager.write().await;
            manager.seed_peers_for_torrent(info_hash, peer_count).await
        };

        // Store the seeded addresses for the coordinator
        {
            let mut seeded = self.seeded_torrents.write().await;
            seeded.insert(info_hash, addresses.clone());
        }

        addresses
    }

    /// Returns the peer manager for direct access.
    pub fn peer_manager(&self) -> Arc<RwLock<ContentAwarePeerManager<P>>> {
        self.peer_manager.clone()
    }

    /// Returns the tracker manager for use with torrent engine.
    pub fn tracker_manager(self) -> SimulatedTrackerManager {
        self.tracker_manager
    }

    /// Gets all seeded peer addresses for a torrent.
    pub async fn get_seeded_peers(&self, info_hash: InfoHash) -> Vec<SocketAddr> {
        let seeded = self.seeded_torrents.read().await;
        seeded.get(&info_hash).cloned().unwrap_or_default()
    }

    /// Removes seeded peers for a torrent (cleanup).
    pub async fn unseed_torrent(&self, info_hash: InfoHash) {
        let mut seeded = self.seeded_torrents.write().await;
        seeded.remove(&info_hash);
    }
}

#[cfg(test)]
mod tests {
    use riptide_core::torrent::TorrentPiece;

    use super::*;
    use crate::piece_store::InMemoryPieceStore;

    #[tokio::test]
    async fn test_simulation_coordinator_creation() {
        let piece_store = Arc::new(InMemoryPieceStore::new());
        let peer_config = InMemoryPeerConfig::default();
        let tracker_config = ResponseConfig::default();

        let coordinator =
            SimulationCoordinator::new(peer_config, tracker_config, piece_store).await;

        // Verify initial state
        let info_hash = InfoHash::new([1u8; 20]);
        let peers = coordinator.get_seeded_peers(info_hash).await;
        assert!(peers.is_empty());
    }

    #[tokio::test]
    async fn test_torrent_seeding_coordination() {
        let piece_store = Arc::new(InMemoryPieceStore::new());
        let info_hash = InfoHash::new([2u8; 20]);

        // Add test piece to store
        let test_piece = TorrentPiece {
            index: 0,
            hash: [0u8; 20],
            data: b"Coordination test data".to_vec(),
        };
        piece_store
            .add_torrent_pieces(info_hash, vec![test_piece])
            .await
            .unwrap();

        let peer_config = InMemoryPeerConfig::default();
        let tracker_config = ResponseConfig::default();

        let coordinator =
            SimulationCoordinator::new(peer_config, tracker_config, piece_store).await;

        // Seed the torrent
        let seeded_addresses = coordinator.seed_torrent(info_hash, 5).await;
        assert_eq!(seeded_addresses.len(), 5);

        // Verify addresses are stored
        let stored_addresses = coordinator.get_seeded_peers(info_hash).await;
        assert_eq!(stored_addresses, seeded_addresses);

        // Verify peers exist in manager
        let binding = coordinator.peer_manager();
        let manager = binding.read().await;
        let manager_peers = manager.get_peers_for_torrent(info_hash).await;
        assert_eq!(manager_peers.len(), 5);

        for address in &seeded_addresses {
            assert!(manager_peers.contains(address));
        }
    }

    #[tokio::test]
    async fn test_coordinator_cleanup() {
        let piece_store = Arc::new(InMemoryPieceStore::new());
        let info_hash = InfoHash::new([3u8; 20]);

        let peer_config = InMemoryPeerConfig::default();
        let tracker_config = ResponseConfig::default();

        let coordinator =
            SimulationCoordinator::new(peer_config, tracker_config, piece_store).await;

        // Seed and verify
        coordinator.seed_torrent(info_hash, 3).await;
        let peers_before = coordinator.get_seeded_peers(info_hash).await;
        assert_eq!(peers_before.len(), 3);

        // Cleanup and verify
        coordinator.unseed_torrent(info_hash).await;
        let peers_after = coordinator.get_seeded_peers(info_hash).await;
        assert!(peers_after.is_empty());
    }
}
