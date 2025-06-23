//! Core torrent download engine

use std::collections::HashMap;

use super::{BencodeTorrentParser, InfoHash, PeerManager, TorrentError, TorrentParser};
use crate::config::RiptideConfig;

/// Main orchestrator for torrent downloads
pub struct TorrentEngine {
    /// Peer connection manager
    peer_manager: PeerManager,
    /// Active torrents being downloaded
    active_torrents: HashMap<InfoHash, TorrentSession>,
    /// Torrent parser for metadata extraction
    parser: BencodeTorrentParser,
    /// Configuration
    config: RiptideConfig,
}

/// Active download session for a single torrent.
///
/// Tracks download progress, piece completion status, and session metadata
/// for an individual torrent being downloaded by the engine.
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
            parser: BencodeTorrentParser::new(),
            config,
        }
    }

    /// Add a torrent by magnet link
    ///
    /// # Errors
    /// - `TorrentError::InvalidTorrentFile` - Magnet link parsing failed
    pub async fn add_magnet(&mut self, magnet_link: &str) -> Result<InfoHash, TorrentError> {
        let magnet = self.parser.parse_magnet_link(magnet_link).await?;
        let info_hash = magnet.info_hash;

        // Create session with minimal metadata from magnet link
        let session = TorrentSession {
            info_hash,
            piece_count: 0, // Unknown until we get metadata from peers
            piece_size: self.config.torrent.default_piece_size,
            completed_pieces: Vec::new(),
            progress: 0.0,
        };

        self.active_torrents.insert(info_hash, session);
        Ok(info_hash)
    }

    /// Add a torrent from .torrent file data
    ///
    /// # Errors
    /// - `TorrentError::InvalidTorrentFile` - Torrent file parsing failed
    pub async fn add_torrent_data(
        &mut self,
        torrent_data: &[u8],
    ) -> Result<InfoHash, TorrentError> {
        let metadata = self.parser.parse_torrent_data(torrent_data).await?;
        let info_hash = metadata.info_hash;

        // Create session with complete metadata from torrent file
        let session = TorrentSession {
            info_hash,
            piece_count: metadata.piece_hashes.len() as u32,
            piece_size: metadata.piece_length,
            completed_pieces: vec![false; metadata.piece_hashes.len()],
            progress: 0.0,
        };

        self.active_torrents.insert(info_hash, session);
        Ok(info_hash)
    }

    /// Start downloading a torrent with peer discovery
    ///
    /// # Errors
    /// - `TorrentError::NoPeersAvailable` - No torrent session found
    pub async fn start_download(&mut self, info_hash: InfoHash) -> Result<(), TorrentError> {
        let session = self
            .active_torrents
            .get_mut(&info_hash)
            .ok_or(TorrentError::NoPeersAvailable)?;

        // For magnet links, we need to discover peers first to get metadata
        if session.piece_count == 0 {
            // Start metadata discovery phase
            // In a full implementation, this would:
            // 1. Connect to tracker to get peer list
            // 2. Connect to peers and request metadata
            // 3. Once metadata is received, initialize piece tracking

            // For now, simulate with placeholder values
            session.piece_count = 100; // Placeholder
            session.completed_pieces = vec![false; session.piece_count as usize];
        }

        Ok(())
    }

    /// Get torrent session information
    ///
    /// # Errors
    /// - `TorrentError::NoPeersAvailable` - No torrent session found
    pub fn get_session(&self, info_hash: InfoHash) -> Result<&TorrentSession, TorrentError> {
        self.active_torrents
            .get(&info_hash)
            .ok_or(TorrentError::NoPeersAvailable)
    }

    /// Add discovered peer to torrent
    ///
    /// # Errors
    /// - `TorrentError::ConnectionLimitExceeded` - Too many active connections
    /// - `TorrentError::ConnectionFailed` - Failed to establish connection
    pub async fn add_peer(
        &mut self,
        info_hash: InfoHash,
        address: std::net::SocketAddr,
    ) -> Result<(), TorrentError> {
        self.peer_manager.add_peer(info_hash, address).await
    }

    /// Get download statistics for all active torrents
    pub async fn get_download_stats(&self) -> EngineStats {
        let peer_stats = self.peer_manager.get_stats().await;

        let total_progress: f32 = self
            .active_torrents
            .values()
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

/// Statistical information about the torrent engine.
///
/// Provides aggregate metrics across all active torrent downloads
/// including peer counts, transfer amounts, and progress indicators.
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
    use std::net::{IpAddr, Ipv4Addr, SocketAddr};

    use super::*;
    use crate::torrent::test_data::create_test_info_hash;

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
        let address = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 8080);

        engine.add_peer(info_hash, address).await.unwrap();

        let stats = engine.get_download_stats().await;
        assert_eq!(stats.total_peers, 1);
    }

    #[tokio::test]
    async fn test_add_magnet_link() {
        let mut engine = TorrentEngine::default();
        let magnet_url = "magnet:?xt=urn:btih:0123456789abcdef0123456789abcdef01234567&dn=Test%20Torrent&tr=http://tracker.example.com/announce";

        let info_hash = engine.add_magnet(magnet_url).await.unwrap();

        // Verify session was created
        let session = engine.get_session(info_hash).unwrap();
        assert_eq!(session.info_hash, info_hash);
        assert_eq!(session.piece_count, 0); // Unknown from magnet link
        assert_eq!(session.progress, 0.0);

        let stats = engine.get_download_stats().await;
        assert_eq!(stats.active_torrents, 1);
    }

    #[tokio::test]
    async fn test_add_torrent_data() {
        let mut engine = TorrentEngine::default();
        let torrent_data = b"d8:announce9:test:80804:infod6:lengthi1000e4:name8:test.txt12:piece lengthi32768e6:pieces20:12345678901234567890ee";

        let info_hash = engine.add_torrent_data(torrent_data).await.unwrap();

        // Verify session was created with metadata
        let session = engine.get_session(info_hash).unwrap();
        assert_eq!(session.info_hash, info_hash);
        assert_eq!(session.piece_count, 1); // One piece from torrent data
        assert_eq!(session.piece_size, 32768);
        assert_eq!(session.completed_pieces.len(), 1);
        assert_eq!(session.progress, 0.0);
    }

    #[tokio::test]
    async fn test_start_download() {
        let mut engine = TorrentEngine::default();
        let magnet_url =
            "magnet:?xt=urn:btih:0123456789abcdef0123456789abcdef01234567&dn=Test%20Torrent";

        let info_hash = engine.add_magnet(magnet_url).await.unwrap();

        // Start download should initialize piece tracking for magnet links
        engine.start_download(info_hash).await.unwrap();

        let session = engine.get_session(info_hash).unwrap();
        assert_eq!(session.piece_count, 100); // Placeholder value
        assert_eq!(session.completed_pieces.len(), 100);
    }

    #[tokio::test]
    async fn test_invalid_magnet_link() {
        let mut engine = TorrentEngine::default();
        let invalid_magnet = "invalid://not-a-magnet";

        let result = engine.add_magnet(invalid_magnet).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_get_nonexistent_session() {
        let engine = TorrentEngine::default();
        let info_hash = create_test_info_hash();

        let result = engine.get_session(info_hash);
        assert!(result.is_err());
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
