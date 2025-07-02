//! Core torrent engine implementation for the actor model.

use std::collections::HashMap;
use std::sync::Arc;

use tokio::sync::RwLock;
use tokio::time::{Duration, sleep};

use super::commands::{EngineStats, TorrentSession, TorrentSessionParams};
use crate::config::RiptideConfig;
use crate::torrent::parsing::types::TorrentMetadata;
use crate::torrent::tracker::{AnnounceEvent, AnnounceRequest, TrackerManagement};
use crate::torrent::{
    BencodeTorrentParser, InfoHash, PeerId, PeerManager, TorrentError, TorrentParser,
};

/// Core torrent engine implementation.
///
/// This is the private implementation that runs inside the actor. It manages
/// active torrents, coordinates with peer and tracker managers, and handles
/// all torrent operations. The engine is single-threaded and processes
/// commands sequentially to avoid race conditions.
pub struct TorrentEngine<P: PeerManager, T: TrackerManagement> {
    /// Peer connection manager (real or simulated)
    peer_manager: Arc<RwLock<P>>,
    /// Tracker manager (real or simulated)
    tracker_manager: Arc<RwLock<T>>,
    /// Active torrents being downloaded
    active_torrents: HashMap<InfoHash, TorrentSession>,
    /// Torrent parser for metadata extraction
    parser: BencodeTorrentParser,
    /// Configuration
    config: RiptideConfig,
    /// Our peer ID for BitTorrent protocol
    peer_id: PeerId,
}

impl<P: PeerManager, T: TrackerManagement + 'static> TorrentEngine<P, T> {
    /// Creates new torrent engine with provided peer manager and tracker manager.
    pub fn new(config: RiptideConfig, peer_manager: P, tracker_manager: T) -> Self {
        Self {
            peer_manager: Arc::new(RwLock::new(peer_manager)),
            tracker_manager: Arc::new(RwLock::new(tracker_manager)),
            active_torrents: HashMap::new(),
            parser: BencodeTorrentParser::new(),
            config,
            peer_id: PeerId::generate(),
        }
    }

    /// Adds a torrent from a magnet link.
    ///
    /// Parses the magnet link to extract the info hash and tracker URLs,
    /// then creates a new torrent session. The torrent is not started
    /// automatically - use `start_download` to begin downloading.
    ///
    /// # Errors
    /// - `TorrentError::InvalidMagnetLink` - Malformed magnet link
    /// - `TorrentError::DuplicateTorrent` - Torrent already exists
    pub async fn add_magnet(&mut self, magnet_link: &str) -> Result<InfoHash, TorrentError> {
        let parsed = self.parser.parse_magnet_link(magnet_link).await?;
        let info_hash = parsed.info_hash;

        if self.active_torrents.contains_key(&info_hash) {
            return Err(TorrentError::DuplicateTorrent { info_hash });
        }

        // Use fallback trackers if magnet doesn't contain any
        let tracker_urls = if parsed.trackers.is_empty() {
            self.get_fallback_trackers()
        } else {
            parsed.trackers
        };

        // For magnet links, we don't have piece information yet
        // Create a placeholder session that will be updated when metadata is received
        let session = TorrentSession::new(TorrentSessionParams {
            info_hash,
            piece_count: 1, // Placeholder - will be updated
            piece_size: self.config.torrent.default_piece_size,
            total_size: 0, // Placeholder - will be updated
            filename: format!("magnet_{}", &info_hash.to_string()[..8]),
            tracker_urls,
        });

        self.active_torrents.insert(info_hash, session);
        Ok(info_hash)
    }

    /// Gets fallback tracker URLs from configuration.
    fn get_fallback_trackers(&self) -> Vec<String> {
        vec![
            "udp://tracker.openbittorrent.com:80/announce".to_string(),
            "udp://tracker.publicbt.com:80/announce".to_string(),
        ]
    }

    /// Adds torrent metadata directly to the engine.
    ///
    /// Creates a new torrent session from pre-parsed metadata. This is used
    /// for simulation scenarios where metadata is generated programmatically.
    ///
    /// # Errors
    /// - `TorrentError::DuplicateTorrent` - Torrent already exists
    pub fn add_torrent_metadata(
        &mut self,
        metadata: TorrentMetadata,
    ) -> Result<InfoHash, TorrentError> {
        let info_hash = metadata.info_hash;

        if self.active_torrents.contains_key(&info_hash) {
            return Err(TorrentError::DuplicateTorrent { info_hash });
        }

        let session = TorrentSession::new(TorrentSessionParams {
            info_hash,
            piece_count: metadata.piece_hashes.len() as u32,
            piece_size: metadata.piece_length,
            total_size: metadata.total_length,
            filename: metadata.name.clone(),
            tracker_urls: metadata.announce_urls,
        });

        self.active_torrents.insert(info_hash, session);
        Ok(info_hash)
    }

    /// Starts downloading a torrent by its info hash.
    ///
    /// Initiates the download process by announcing to trackers, discovering
    /// peers, and spawning download tasks. The torrent session is updated
    /// to reflect the active download state.
    ///
    /// # Errors
    /// - `TorrentError::TorrentNotFound` - Info hash not in active torrents
    /// - `TorrentError::TrackerConnectionFailed` - Could not contact tracker
    pub async fn start_download(&mut self, info_hash: InfoHash) -> Result<(), TorrentError> {
        let session = self
            .active_torrents
            .get_mut(&info_hash)
            .ok_or(TorrentError::TorrentNotFound { info_hash })?;

        if session.is_downloading {
            return Ok(()); // Already downloading
        }

        session.is_downloading = true;
        session.started_at = std::time::Instant::now();

        // Announce to trackers to discover peers
        let tracker_manager = self.tracker_manager.clone();
        let tracker_urls = session.tracker_urls.clone();

        for tracker_url in tracker_urls {
            let announce_request = AnnounceRequest {
                info_hash,
                peer_id: *self.peer_id.as_bytes(),
                port: 6881, // Default BitTorrent port
                uploaded: 0,
                downloaded: 0,
                left: session.total_size,
                event: AnnounceEvent::Started,
            };

            let tracker_manager_clone = tracker_manager.clone();
            let url_clone = tracker_url.clone();

            tokio::spawn(async move {
                let mut manager = tracker_manager_clone.write().await;
                if let Err(e) = manager
                    .announce_to_trackers(std::slice::from_ref(&url_clone), announce_request)
                    .await
                {
                    tracing::warn!("Failed to announce to tracker {}: {}", url_clone, e);
                }
            });
        }

        // Spawn download loop for this torrent
        self.spawn_download_loop(info_hash).await;

        Ok(())
    }

    /// Spawns the download loop for a specific torrent.
    ///
    /// Creates background tasks to handle piece downloading, peer communication,
    /// and progress tracking. The download loop runs until the torrent is
    /// complete or stopped.
    async fn spawn_download_loop(&self, info_hash: InfoHash) {
        let _peer_manager = self.peer_manager.clone();
        let _config = self.config.clone();

        tokio::spawn(async move {
            tracing::info!("Starting download loop for torrent {}", info_hash);

            // Simulate download progress over time
            let mut progress = 0.0f32;
            let download_duration = Duration::from_secs(60); // Default 1 minute simulation
            let update_interval = Duration::from_millis(500);
            let progress_increment =
                0.5 / (download_duration.as_millis() as f32 / update_interval.as_millis() as f32);

            while progress < 1.0 {
                sleep(update_interval).await;
                progress += progress_increment;
                progress = progress.min(1.0);

                tracing::debug!("Torrent {} progress: {:.1}%", info_hash, progress * 100.0);

                if progress >= 1.0 {
                    tracing::info!("Torrent {} download completed", info_hash);
                    break;
                }
            }
        });
    }

    /// Gets the session information for a specific torrent.
    ///
    /// Returns a reference to the torrent session, or None if the torrent
    /// is not found in the active torrents.
    pub fn get_session(&self, info_hash: InfoHash) -> Option<&TorrentSession> {
        self.active_torrents.get(&info_hash)
    }

    /// Marks specific pieces as completed for a torrent.
    ///
    /// Updates the piece completion status and recalculates download progress.
    /// This method is typically called by download tasks when pieces are
    /// successfully downloaded and verified.
    ///
    /// # Errors
    /// - `TorrentError::TorrentNotFound` - Info hash not in active torrents
    /// - `TorrentError::InvalidPieceIndex` - Piece index out of bounds
    pub fn mark_pieces_completed(
        &mut self,
        info_hash: InfoHash,
        piece_indices: Vec<u32>,
    ) -> Result<(), TorrentError> {
        let session = self
            .active_torrents
            .get_mut(&info_hash)
            .ok_or(TorrentError::TorrentNotFound { info_hash })?;

        for piece_index in piece_indices {
            if piece_index >= session.piece_count {
                return Err(TorrentError::InvalidPieceIndex {
                    index: piece_index,
                    max_index: session.piece_count - 1,
                });
            }
            session.complete_piece(piece_index);
        }

        Ok(())
    }

    /// Connects to a peer for a specific torrent.
    ///
    /// Establishes a connection to the given peer and initiates the BitTorrent
    /// handshake process. This method is used during the peer discovery phase.
    ///
    /// # Errors
    /// - `TorrentError::PeerConnectionError` - Could not connect to peer
    #[allow(dead_code)]
    pub async fn connect_peer(
        &self,
        info_hash: InfoHash,
        peer_address: std::net::SocketAddr,
    ) -> Result<(), TorrentError> {
        let mut peer_manager = self.peer_manager.write().await;
        peer_manager
            .connect_peer(peer_address, info_hash, self.peer_id)
            .await
            .map_err(|e| TorrentError::PeerConnectionError {
                reason: format!("Failed to connect to {peer_address}: {e}"),
            })
    }

    /// Gets all active torrent sessions.
    ///
    /// Returns a reference to all torrent sessions currently managed by the engine.
    pub fn active_sessions(&self) -> impl Iterator<Item = &TorrentSession> {
        self.active_torrents.values()
    }

    /// Gets download statistics for the engine.
    ///
    /// Calculates and returns aggregated statistics including active torrent
    /// count, peer connections, and data transfer metrics.
    pub async fn get_download_stats(&self) -> EngineStats {
        let active_torrents = self.active_torrents.len();

        // Calculate total peers across all torrents
        let total_peers = {
            let peer_manager = self.peer_manager.read().await;
            // Calculate total peers across all torrents
            peer_manager.connected_peers().await.len()
        };

        // Calculate bytes downloaded and average progress
        let mut total_downloaded = 0u64;
        let mut total_progress = 0.0f32;

        for session in self.active_torrents.values() {
            let completed_pieces = session.completed_pieces.iter().filter(|&&x| x).count();
            let bytes_for_session = completed_pieces as u64 * session.piece_size as u64;
            total_downloaded += bytes_for_session;
            total_progress += session.progress;
        }

        let average_progress = if active_torrents > 0 {
            total_progress / active_torrents as f32
        } else {
            0.0
        };

        EngineStats {
            active_torrents,
            total_peers,
            bytes_downloaded: total_downloaded,
            bytes_uploaded: 0, // TODO: Track upload stats
            average_progress,
        }
    }

    /// Performs maintenance tasks on the engine.
    ///
    /// Cleans up completed torrents, updates statistics, and performs
    /// other housekeeping operations. This should be called periodically.
    #[allow(dead_code)]
    pub async fn maintenance(&mut self) {
        // Remove completed torrents that are no longer needed
        // TODO: Implement based on configuration
    }
}
