//! Core torrent download engine

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;

use super::tracker::{AnnounceEvent, AnnounceRequest, TrackerManagement};
use super::{BencodeTorrentParser, InfoHash, PeerId, PeerManager, TorrentError, TorrentParser};
use crate::config::RiptideConfig;

/// Main orchestrator for torrent downloads with unified interface support.
///
/// Uses trait-based peer and tracker management enabling both real and simulated
/// implementations for production use and comprehensive testing/fuzzing.
pub struct TorrentEngine<P: PeerManager, T: TrackerManagement> {
    /// Peer connection manager (real or simulated)
    peer_manager: Arc<tokio::sync::RwLock<P>>,
    /// Tracker manager (real or simulated)
    tracker_manager: Arc<tokio::sync::RwLock<T>>,
    /// Active torrents being downloaded
    active_torrents: HashMap<InfoHash, TorrentSession>,
    /// Torrent parser for metadata extraction
    parser: BencodeTorrentParser,
    /// Configuration
    config: RiptideConfig,
    /// Our peer ID for BitTorrent protocol
    peer_id: PeerId,
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
    /// When download started (for simulation)
    pub started_at: Instant,
    /// Whether download is actively running
    pub is_downloading: bool,
    /// Tracker URLs for this torrent
    pub tracker_urls: Vec<String>,
}

impl<P: PeerManager, T: TrackerManagement> TorrentEngine<P, T> {
    /// Creates new torrent engine with provided peer manager and tracker manager.
    ///
    /// Uses dependency injection pattern to support both real and simulated implementations.
    /// This enables the same engine logic to work with real BitTorrent operations or
    /// deterministic simulation for testing and fuzzing.
    pub fn new(config: RiptideConfig, peer_manager: P, tracker_manager: T) -> Self {
        Self {
            peer_manager: Arc::new(tokio::sync::RwLock::new(peer_manager)),
            tracker_manager: Arc::new(tokio::sync::RwLock::new(tracker_manager)),
            active_torrents: HashMap::new(),
            parser: BencodeTorrentParser::new(),
            config,
            peer_id: PeerId::generate(),
        }
    }

    /// Add a torrent by magnet link
    ///
    /// # Errors
    /// - `TorrentError::InvalidTorrentFile` - Magnet link parsing failed
    pub async fn add_magnet(&mut self, magnet_link: &str) -> Result<InfoHash, TorrentError> {
        let magnet = self.parser.parse_magnet_link(magnet_link).await?;
        let info_hash = magnet.info_hash;

        tracing::debug!(
            "Parsed magnet link: info_hash={info_hash}, display_name={:?}, trackers={:?}",
            magnet.display_name,
            magnet.trackers
        );

        // Filter out UDP trackers (we only support HTTP for now) and use fallback if none remain
        let http_trackers: Vec<String> = magnet
            .trackers
            .into_iter()
            .filter(|url| url.starts_with("http://") || url.starts_with("https://"))
            .collect();

        let tracker_urls = if http_trackers.is_empty() {
            tracing::info!("No HTTP trackers in magnet link, using fallback trackers");
            self.get_fallback_trackers()
        } else {
            tracing::info!(
                "Using {} HTTP trackers from magnet link",
                http_trackers.len()
            );
            http_trackers
        };

        // Create session with minimal metadata from magnet link
        let session = TorrentSession {
            info_hash,
            piece_count: 0, // Unknown until we get metadata from peers
            piece_size: self.config.torrent.default_piece_size,
            completed_pieces: Vec::new(),
            progress: 0.0,
            started_at: Instant::now(),
            is_downloading: false,
            tracker_urls,
        };

        self.active_torrents.insert(info_hash, session);
        Ok(info_hash)
    }

    /// Get fallback tracker URLs when magnet link doesn't contain trackers.
    ///
    /// Returns a minimal list of fast-responding public trackers. Using fewer trackers
    /// that respond quickly rather than many slow/dead ones for better user experience.
    fn get_fallback_trackers(&self) -> Vec<String> {
        vec![
            // Fast responding core trackers only
            "http://tracker.opentrackr.org:1337/announce".to_string(),
            "udp://tracker.opentrackr.org:1337/announce".to_string(),
        ]
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
            started_at: Instant::now(),
            is_downloading: false,
            tracker_urls: metadata.announce_urls,
        };

        self.active_torrents.insert(info_hash, session);
        Ok(info_hash)
    }

    /// Start downloading a torrent with real BitTorrent peer discovery.
    ///
    /// Implements the complete BitTorrent workflow:
    /// 1. Announce to tracker to get peer list
    /// 2. Connect to discovered peers
    /// 3. Begin piece exchange protocol
    ///
    /// # Errors
    /// - `TorrentError::TorrentNotFound` - No torrent session found for this info hash
    /// - `TorrentError::TrackerConnectionFailed` - Could not reach tracker
    /// - `TorrentError::NoPeersAvailable` - Tracker returned no peers
    pub async fn start_download(&mut self, info_hash: InfoHash) -> Result<(), TorrentError> {
        // Get tracker URLs and prepare announce request
        let (announce_request, tracker_urls) = {
            let session = self
                .active_torrents
                .get_mut(&info_hash)
                .ok_or(TorrentError::TorrentNotFound { info_hash })?;

            // For magnet links, we need to discover peers first to get metadata
            if session.piece_count == 0 {
                session.piece_count = 100; // Placeholder until we get real metadata
                session.completed_pieces = vec![false; session.piece_count as usize];
            }

            let announce_request = AnnounceRequest {
                info_hash,
                peer_id: *self.peer_id.as_bytes(),
                port: 6881, // TODO: Use configurable port
                uploaded: 0,
                downloaded: 0,
                left: session.piece_count as u64 * session.piece_size as u64,
                event: AnnounceEvent::Started,
            };

            (announce_request, session.tracker_urls.clone())
        };

        // If no tracker URLs available, return error
        if tracker_urls.is_empty() {
            return Err(TorrentError::TrackerConnectionFailed {
                url: "No tracker URLs available for this torrent".to_string(),
            });
        }

        tracing::info!("Attempting to announce to {} trackers", tracker_urls.len());
        for url in &tracker_urls {
            tracing::debug!("Using tracker: {}", url);
        }

        // Use tracker manager to announce to best available tracker
        let tracker_response = {
            let mut tracker_manager = self.tracker_manager.write().await;
            match tracker_manager
                .announce_to_trackers(&tracker_urls, announce_request)
                .await
            {
                Ok(response) => response,
                Err(e) => {
                    tracing::error!("All trackers failed. This could be due to:");
                    tracing::error!("1. Network connectivity issues");
                    tracing::error!("2. Tracker servers being offline");
                    tracing::error!("3. Firewall blocking tracker connections");
                    tracing::error!(
                        "Consider using torrents with DHT support or different trackers"
                    );

                    // For now, we'll fail fast rather than fall back to simulation
                    // In a production client, this is where DHT/PEX would be used
                    return Err(e);
                }
            }
        };

        // Connect to discovered peers
        let mut connected_count = 0;
        for peer_addr in tracker_response
            .peers
            .iter()
            .take(self.config.network.max_peer_connections)
        {
            match self.connect_peer(info_hash, *peer_addr).await {
                Ok(()) => {
                    connected_count += 1;
                    println!("Connected to peer: {peer_addr}");
                }
                Err(e) => {
                    eprintln!("Failed to connect to peer {peer_addr}: {e}");
                }
            }
        }

        if connected_count == 0 {
            tracing::warn!(
                "No peer connections established. Tracker announced successfully but peer connections failed."
            );
            tracing::info!(
                "This indicates the BitTorrent wire protocol implementation needs completion."
            );
            tracing::info!(
                "Production BitTorrent clients would use DHT and PEX for additional peer discovery."
            );

            return Err(TorrentError::NoPeersAvailable);
        }

        // Update session to start downloading
        let session = self
            .active_torrents
            .get_mut(&info_hash)
            .ok_or(TorrentError::TorrentNotFound { info_hash })?;
        session.is_downloading = true;
        session.started_at = Instant::now();

        println!("Started download for {info_hash} with {connected_count} peers");
        Ok(())
    }

    /// Simulate downloading progress for demo purposes.
    ///
    /// In production, this would be replaced by real BitTorrent protocol implementation.
    /// Downloads pieces sequentially at ~2 pieces per second for streaming optimization.
    pub fn simulate_download_progress(&mut self) {
        for session in self.active_torrents.values_mut() {
            if !session.is_downloading || session.progress >= 1.0 {
                continue;
            }

            let elapsed = session.started_at.elapsed();
            // Download ~2 pieces per second for streaming
            let pieces_to_complete = (elapsed.as_secs_f32() * 2.0) as usize;
            let target_pieces = pieces_to_complete.min(session.piece_count as usize);

            // Complete pieces sequentially (better for streaming)
            for i in 0..target_pieces {
                if i < session.completed_pieces.len() {
                    session.completed_pieces[i] = true;
                }
            }

            // Update progress
            let completed_count = session.completed_pieces.iter().filter(|&&x| x).count();
            session.progress = completed_count as f32 / session.piece_count as f32;

            // Stop downloading when complete
            if session.progress >= 1.0 {
                session.is_downloading = false;
            }
        }
    }

    /// Get torrent session information
    ///
    /// # Errors
    /// - `TorrentError::TorrentNotFound` - No torrent session found for this info hash
    pub fn get_session(&self, info_hash: InfoHash) -> Result<&TorrentSession, TorrentError> {
        self.active_torrents
            .get(&info_hash)
            .ok_or(TorrentError::TorrentNotFound { info_hash })
    }

    /// Connect to discovered peer for torrent
    ///
    /// # Errors
    /// - `TorrentError::PeerConnectionError` - Failed to establish connection
    pub async fn connect_peer(
        &mut self,
        info_hash: InfoHash,
        address: std::net::SocketAddr,
    ) -> Result<(), TorrentError> {
        let mut peer_manager = self.peer_manager.write().await;
        peer_manager
            .connect_peer(address, info_hash, self.peer_id)
            .await
    }

    /// All active torrent sessions
    pub fn active_sessions(&self) -> Vec<&TorrentSession> {
        self.active_torrents.values().collect()
    }

    /// Get download statistics for all active torrents
    pub async fn get_download_stats(&self) -> EngineStats {
        let peer_manager = self.peer_manager.read().await;
        let total_peers = peer_manager.connection_count().await;
        let connected_peers = peer_manager.connected_peers().await;

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

        // Calculate bytes from peer info (simplified for now)
        let bytes_downloaded = connected_peers
            .iter()
            .map(|peer| peer.bytes_downloaded)
            .sum();
        let bytes_uploaded = connected_peers.iter().map(|peer| peer.bytes_uploaded).sum();

        EngineStats {
            active_torrents: self.active_torrents.len(),
            total_peers,
            bytes_downloaded,
            bytes_uploaded,
            average_progress,
        }
    }

    /// Cleanup stale connections and update statistics
    pub async fn maintenance(&mut self) {
        // In the new interface, cleanup is handled internally by the peer manager
        // This can be extended to include torrent-specific maintenance tasks
    }
}

// Default implementation removed - requires dependency injection

impl TorrentSession {
    /// Creates new torrent session
    pub fn new(info_hash: InfoHash, piece_count: u32, piece_size: u32) -> Self {
        Self {
            info_hash,
            piece_count,
            piece_size,
            completed_pieces: vec![false; piece_count as usize],
            progress: 0.0,
            started_at: Instant::now(),
            is_downloading: false,
            tracker_urls: Vec::new(),
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
        use super::super::{TcpPeerManager, TrackerManager};

        let config = RiptideConfig::default();
        let peer_manager = TcpPeerManager::new_default();
        let tracker_manager = TrackerManager::new(config.network.clone());
        let engine = TorrentEngine::new(config, peer_manager, tracker_manager);

        let stats = engine.get_download_stats().await;
        assert_eq!(stats.active_torrents, 0);
        assert_eq!(stats.total_peers, 0);
    }

    #[tokio::test]
    async fn test_connect_peer_to_torrent() {
        // This test should verify the engine's connect_peer logic without network I/O
        // Since we can't easily mock TcpPeerManager, we'll test the basic state management
        use super::super::{TcpPeerManager, TrackerManager};

        let config = RiptideConfig::default();
        let peer_manager = TcpPeerManager::new_default();
        let tracker_manager = TrackerManager::new(config.network.clone());
        let mut engine = TorrentEngine::new(config, peer_manager, tracker_manager);
        let info_hash = create_test_info_hash();

        // Test that engine accepts the peer connection call (but network will fail)
        // This tests the engine logic, not the network layer
        let address = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 8080);
        let result = engine.connect_peer(info_hash, address).await;
        
        // We expect this to fail due to no server running, but that's expected
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), TorrentError::PeerConnectionError { .. }));
    }

    #[tokio::test]
    async fn test_add_magnet_link() {
        use super::super::{TcpPeerManager, TrackerManager};

        let config = RiptideConfig::default();
        let peer_manager = TcpPeerManager::new_default();
        let tracker_manager = TrackerManager::new(config.network.clone());
        let mut engine = TorrentEngine::new(config, peer_manager, tracker_manager);
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
        use super::super::{TcpPeerManager, TrackerManager};

        let config = RiptideConfig::default();
        let peer_manager = TcpPeerManager::new_default();
        let tracker_manager = TrackerManager::new(config.network.clone());
        let mut engine = TorrentEngine::new(config, peer_manager, tracker_manager);
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
        // This test verifies magnet link parsing and session creation without network I/O
        use super::super::{TcpPeerManager, TrackerManager};

        let config = RiptideConfig::default();
        let peer_manager = TcpPeerManager::new_default();
        let tracker_manager = TrackerManager::new(config.network.clone());
        let mut engine = TorrentEngine::new(config, peer_manager, tracker_manager);
        let magnet_url = "magnet:?xt=urn:btih:0123456789abcdef0123456789abcdef01234567&dn=Test%20Torrent&tr=http://tracker.example.com/announce";

        let info_hash = engine.add_magnet(magnet_url).await.unwrap();

        // Test that start_download will fail on tracker connection (expected for unit test)
        let result = engine.start_download(info_hash).await;
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), TorrentError::TrackerConnectionFailed { .. }));
        
        // Test that the session was created even though download failed
        let session = engine.get_session(info_hash).unwrap();
        assert_eq!(session.piece_count, 100); // Placeholder value from magnet link
        assert_eq!(session.completed_pieces.len(), 100);
    }

    #[tokio::test]
    async fn test_invalid_magnet_link() {
        use super::super::{TcpPeerManager, TrackerManager};

        let config = RiptideConfig::default();
        let peer_manager = TcpPeerManager::new_default();
        let tracker_manager = TrackerManager::new(config.network.clone());
        let mut engine = TorrentEngine::new(config, peer_manager, tracker_manager);
        let invalid_magnet = "invalid://not-a-magnet";

        let result = engine.add_magnet(invalid_magnet).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_get_nonexistent_session() {
        use super::super::{TcpPeerManager, TrackerManager};

        let config = RiptideConfig::default();
        let peer_manager = TcpPeerManager::new_default();
        let tracker_manager = TrackerManager::new(config.network.clone());
        let engine = TorrentEngine::new(config, peer_manager, tracker_manager);
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
        use super::super::{TcpPeerManager, TrackerManager};

        let config = RiptideConfig::default();
        let peer_manager = TcpPeerManager::new_default();
        let tracker_manager = TrackerManager::new(config.network.clone());
        let mut engine = TorrentEngine::new(config, peer_manager, tracker_manager);

        // Maintenance should not panic on empty engine
        engine.maintenance().await;

        let stats = engine.get_download_stats().await;
        assert_eq!(stats.active_torrents, 0);
    }
}
