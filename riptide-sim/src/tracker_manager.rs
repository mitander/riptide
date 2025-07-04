//! Development tracker manager that provides BitTorrent tracker functionality
//! for development mode by spawning real peer servers and coordinating them.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;

use async_trait::async_trait;
use riptide_core::torrent::{
    AnnounceRequest, AnnounceResponse, InfoHash, ScrapeRequest, ScrapeResponse, TorrentError,
    TrackerManagement,
};
use tokio::sync::RwLock;

use crate::{InMemoryPieceStore, spawn_peer_servers_for_torrent};

/// Tracker manager that spawns real peer servers for simulation
///
/// This provides a TrackerManagement implementation that looks like production
/// to the engine but internally manages simulated peer servers.
pub struct TrackerManager {
    piece_store: Arc<InMemoryPieceStore>,
    spawned_peers: Arc<RwLock<HashMap<InfoHash, Vec<SocketAddr>>>>,
}

impl TrackerManager {
    /// Creates a new tracker manager for simulation
    pub fn new(piece_store: Arc<InMemoryPieceStore>) -> Self {
        Self {
            piece_store,
            spawned_peers: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Creates a tracker manager that shares peer registry with another instance
    pub fn with_shared_peers(
        piece_store: Arc<InMemoryPieceStore>,
        shared_peers: Arc<RwLock<HashMap<InfoHash, Vec<SocketAddr>>>>,
    ) -> Self {
        Self {
            piece_store,
            spawned_peers: shared_peers,
        }
    }

    /// Gets the shared peer registry for creating multiple coordinated instances
    pub fn peer_registry(&self) -> Arc<RwLock<HashMap<InfoHash, Vec<SocketAddr>>>> {
        self.spawned_peers.clone()
    }

    /// Seeds peer servers for a torrent
    pub async fn seed_torrent(
        &self,
        info_hash: InfoHash,
        peer_count: usize,
    ) -> Result<Vec<SocketAddr>, Box<dyn std::error::Error + Send + Sync>> {
        // Spawn peer servers that serve pieces for this torrent
        let peer_addresses =
            spawn_peer_servers_for_torrent(info_hash, self.piece_store.clone(), peer_count).await?;

        // Track the spawned peers
        {
            let mut spawned_peers = self.spawned_peers.write().await;
            spawned_peers.insert(info_hash, peer_addresses.clone());
        }

        Ok(peer_addresses)
    }
}

#[async_trait]
impl TrackerManagement for TrackerManager {
    /// Returns spawned peer addresses for announce requests
    async fn announce_to_trackers(
        &mut self,
        _tracker_urls: &[String],
        request: AnnounceRequest,
    ) -> Result<AnnounceResponse, TorrentError> {
        // Get spawned peers for this torrent
        let peers = {
            let spawned_peers = self.spawned_peers.read().await;
            spawned_peers
                .get(&request.info_hash)
                .cloned()
                .unwrap_or_default()
        };

        // Return a realistic announce response with our spawned peer servers
        Ok(AnnounceResponse {
            interval: 300,          // 5 minutes
            min_interval: Some(60), // 1 minute
            tracker_id: Some("development-tracker".to_string()),
            complete: 1,   // We're the only seeder
            incomplete: 0, // No other leechers
            peers,
        })
    }

    /// Returns realistic scrape data
    async fn scrape_from_trackers(
        &mut self,
        _tracker_urls: &[String],
        request: ScrapeRequest,
    ) -> Result<ScrapeResponse, TorrentError> {
        let mut files = HashMap::new();

        for info_hash in request.info_hashes {
            files.insert(
                info_hash,
                riptide_core::torrent::ScrapeStats {
                    complete: 1,   // Our seeded torrent
                    incomplete: 0, // No leechers
                    downloaded: 1, // Downloaded once (by us)
                },
            );
        }

        Ok(ScrapeResponse { files })
    }
}
