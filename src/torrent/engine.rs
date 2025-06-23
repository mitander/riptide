//! Core torrent download engine

use super::{InfoHash, PeerManager, TorrentError};
use crate::config::RiptideConfig;
use std::collections::HashMap;

/// Main orchestrator for torrent downloads
pub struct TorrentEngine {
    /// Peer connection manager
    peer_manager: PeerManager,
    /// Active torrents being downloaded
    active_torrents: HashMap<InfoHash, TorrentSession>,
    /// Configuration
    config: RiptideConfig,
}

/// Download session for a single torrent
#[derive(Debug)]
pub struct TorrentSession {
    /// Torrent info hash
    pub info_hash: InfoHash,
    /// Number of pieces in torrent
    pub piece_count: u32,
    /// Size of each piece in bytes
    pub piece_size: u32,
    /// Pieces we have completed
    pub completed_pieces: Vec<bool>,
    /// Download progress (0.0 to 1.0)
    pub progress: f32,
}

impl TorrentEngine {
    /// Creates new torrent engine with configuration
    pub fn new(config: RiptideConfig) -> Self {
        let peer_manager = PeerManager::new(config.clone());
        
        Self {
            peer_manager,
            active_torrents: HashMap::new(),
            config,
        }
    }

    /// Add a torrent by magnet link
    ///
    /// # Errors
    /// - `TorrentError::ProtocolError` - Magnet link parsing not implemented
    pub async fn add_magnet(&mut self, _magnet_link: &str) -> Result<InfoHash, TorrentError> {
        // TODO: Parse magnet link, extract info_hash, announce to tracker
        Err(TorrentError::ProtocolError {
            message: "Magnet link parsing not implemented".to_string(),
        })
    }

    /// Start downloading a torrent with peer discovery
    ///
    /// # Errors
    /// - `TorrentError::ProtocolError` - Download orchestration not implemented
    pub async fn start_download(&mut self, _info_hash: InfoHash) -> Result<(), TorrentError> {
        // TODO: Initialize peer connections, start piece downloading
        Err(TorrentError::ProtocolError {
            message: "Download orchestration not implemented".to_string(),
        })
    }

    /// Add discovered peer to torrent
    ///
    /// # Errors
    /// - `TorrentError::ConnectionLimitExceeded` - Too many active connections
    /// - `TorrentError::ConnectionFailed` - Failed to establish connection
    pub async fn add_peer(
        &mut self,
        info_hash: InfoHash,
        addr: std::net::SocketAddr,
    ) -> Result<(), TorrentError> {
        self.peer_manager.add_peer(info_hash, addr).await
    }

    /// Get download statistics for all active torrents
    pub async fn get_download_stats(&self) -> EngineStats {
        let peer_stats = self.peer_manager.get_stats().await;
        
        let total_progress: f32 = self.active_torrents.values()
            .map(|session| session.progress)
            .sum();
        
        let average_progress = if self.active_torrents.is_empty() {
            0.0
        } else {
            total_progress / self.active_torrents.len() as f32
        };

        EngineStats {
            active_torrents: self.active_torrents.len(),
            total_peers: peer_stats.total_connections,
            bytes_downloaded: peer_stats.bytes_downloaded,
            bytes_uploaded: peer_stats.bytes_uploaded,
            average_progress,
        }
    }

    /// Cleanup stale connections and update statistics
    pub async fn maintenance(&mut self) {
        self.peer_manager.cleanup_stale_connections().await;
    }
}

impl Default for TorrentEngine {
    fn default() -> Self {
        Self::new(RiptideConfig::default())
    }
}

impl TorrentSession {
    /// Creates new torrent session
    pub fn new(info_hash: InfoHash, piece_count: u32, piece_size: u32) -> Self {
        Self {
            info_hash,
            piece_count,
            piece_size,
            completed_pieces: vec![false; piece_count as usize],
            progress: 0.0,
        }
    }

    /// Mark piece as completed and update progress
    pub fn complete_piece(&mut self, piece_index: u32) {
        if let Some(completed) = self.completed_pieces.get_mut(piece_index as usize) {
            *completed = true;
            self.update_progress();
        }
    }

    /// Check if torrent download is complete
    pub fn is_complete(&self) -> bool {
        self.completed_pieces.iter().all(|&completed| completed)
    }

    /// Update download progress based on completed pieces
    fn update_progress(&mut self) {
        let completed_count = self.completed_pieces.iter().filter(|&&c| c).count();
        self.progress = completed_count as f32 / self.piece_count as f32;
    }
}

/// Overall engine statistics
#[derive(Debug, Clone)]
pub struct EngineStats {
    /// Number of active torrent downloads
    pub active_torrents: usize,
    /// Total connected peers across all torrents
    pub total_peers: usize,
    /// Total bytes downloaded
    pub bytes_downloaded: u64,
    /// Total bytes uploaded
    pub bytes_uploaded: u64,
    /// Average download progress (0.0 to 1.0)
    pub average_progress: f32,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::torrent::test_data::create_test_info_hash;
    use std::net::{IpAddr, Ipv4Addr, SocketAddr};

    #[tokio::test]
    async fn test_torrent_engine_creation() {
        let config = RiptideConfig::default();
        let engine = TorrentEngine::new(config);
        
        let stats = engine.get_download_stats().await;
        assert_eq!(stats.active_torrents, 0);
        assert_eq!(stats.total_peers, 0);
    }

    #[tokio::test]
    async fn test_add_peer_to_torrent() {
        let mut engine = TorrentEngine::default();
        let info_hash = create_test_info_hash();
        let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 8080);

        engine.add_peer(info_hash, addr).await.unwrap();
        
        let stats = engine.get_download_stats().await;
        assert_eq!(stats.total_peers, 1);
    }

    #[test]
    fn test_torrent_session_progress() {
        let info_hash = create_test_info_hash();
        let mut session = TorrentSession::new(info_hash, 4, 32768);
        
        assert_eq!(session.progress, 0.0);
        assert!(!session.is_complete());
        
        session.complete_piece(0);
        assert_eq!(session.progress, 0.25);
        
        session.complete_piece(1);
        session.complete_piece(2);
        session.complete_piece(3);
        assert_eq!(session.progress, 1.0);
        assert!(session.is_complete());
    }

    #[tokio::test]
    async fn test_maintenance_cleanup() {
        let mut engine = TorrentEngine::default();
        
        // Maintenance should not panic on empty engine
        engine.maintenance().await;
        
        let stats = engine.get_download_stats().await;
        assert_eq!(stats.active_torrents, 0);
    }
}
