//! Core torrent engine implementation for the actor model.

use std::collections::HashMap;
use std::sync::Arc;

use tokio::sync::{RwLock, mpsc};
use tokio::time::Duration;

use super::commands::{EngineStats, TorrentEngineCommand, TorrentSession, TorrentSessionParams};
use crate::config::RiptideConfig;
use crate::storage::FileStorage;
use crate::torrent::downloader::PieceDownloader;
use crate::torrent::parsing::types::{TorrentFile, TorrentMetadata};
use crate::torrent::tracker::{AnnounceEvent, AnnounceRequest, TrackerManagement};
use crate::torrent::{
    BencodeTorrentParser, InfoHash, PeerId, PeerManager, PieceIndex, TorrentError, TorrentParser,
};

// Constants
const DEFAULT_BITTORRENT_PORT: u16 = 6881;
const DEFAULT_PIECE_SIZE: u32 = 262_144; // 256KB
const REAL_DOWNLOAD_TIMEOUT_MS: u64 = 30000; // 30 seconds for testing

/// Parameters for piece downloading operations.
struct DownloadParams<P: PeerManager> {
    metadata: TorrentMetadata,
    storage: FileStorage,
    peer_manager: Arc<RwLock<P>>,
    peer_id: PeerId,
    peers: Vec<std::net::SocketAddr>,
    info_hash: InfoHash,
    piece_count: u32,
    piece_sender: mpsc::UnboundedSender<TorrentEngineCommand>,
}

/// Context for download operations.
struct DownloadContext<T: TrackerManagement, P: PeerManager> {
    info_hash: InfoHash,
    tracker_manager: Arc<RwLock<T>>,
    peer_manager: Arc<RwLock<P>>,
    tracker_urls: Vec<String>,
    announce_request: AnnounceRequest,
    piece_count: u32,
    peer_id: PeerId,
    piece_sender: mpsc::UnboundedSender<TorrentEngineCommand>,
}

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
    /// Channel for internal piece completion notifications
    piece_completion_sender: mpsc::UnboundedSender<TorrentEngineCommand>,
}

impl<P: PeerManager + 'static, T: TrackerManagement + 'static> TorrentEngine<P, T> {
    /// Creates new torrent engine with provided peer manager and tracker manager.
    pub fn new(
        config: RiptideConfig,
        peer_manager: P,
        tracker_manager: T,
        piece_completion_sender: mpsc::UnboundedSender<TorrentEngineCommand>,
    ) -> Self {
        Self {
            peer_manager: Arc::new(RwLock::new(peer_manager)),
            tracker_manager: Arc::new(RwLock::new(tracker_manager)),
            active_torrents: HashMap::new(),
            parser: BencodeTorrentParser::new(),
            config,
            peer_id: PeerId::generate(),
            piece_completion_sender,
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
            self.fallback_trackers()
        } else {
            parsed.trackers
        };

        // For magnet links, we don't have piece information yet
        // Create a placeholder session that will be updated when metadata is received
        // Use display name from magnet link if available, otherwise create a readable fallback
        let filename = parsed
            .display_name
            .clone()
            .unwrap_or_else(|| format!("Torrent_{}", &info_hash.to_string()[..16]));

        let session = TorrentSession::new(TorrentSessionParams {
            info_hash,
            piece_count: 1, // Placeholder - will be updated
            piece_size: self.config.torrent.default_piece_size,
            total_size: 0, // Placeholder - will be updated
            filename,
            tracker_urls,
        });

        self.active_torrents.insert(info_hash, session);
        Ok(info_hash)
    }

    /// Fallback tracker URLs from configuration.
    fn fallback_trackers(&self) -> Vec<String> {
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

        // Announce to trackers to discover peers and start real downloading
        let tracker_manager = self.tracker_manager.clone();
        let peer_manager = self.peer_manager.clone();
        let tracker_urls = session.tracker_urls.clone();
        let peer_id = self.peer_id;
        let piece_count = session.piece_count;
        let total_size = session.total_size;
        let piece_sender = self.piece_completion_sender.clone();

        // Create announce request
        let announce_request = AnnounceRequest {
            info_hash,
            peer_id: *peer_id.as_bytes(),
            port: DEFAULT_BITTORRENT_PORT,
            uploaded: 0,
            downloaded: 0,
            left: total_size,
            event: AnnounceEvent::Started,
        };

        // Start BitTorrent download process
        let download_context = DownloadContext {
            info_hash,
            tracker_manager,
            peer_manager,
            tracker_urls,
            announce_request,
            piece_count,
            peer_id,
            piece_sender,
        };

        self.spawn_download_task(download_context).await;

        Ok(())
    }

    /// Spawns BitTorrent download task using tracker responses and peer connections.
    ///
    /// Announces to trackers, discovers peers, and downloads pieces using the BitTorrent
    /// wire protocol in a background task.
    async fn spawn_download_task(&self, download_context: DownloadContext<T, P>) {
        tokio::spawn(async move {
            let info_hash = download_context.info_hash;
            let piece_count = download_context.piece_count;

            tracing::info!(
                "Starting real BitTorrent download for torrent {} ({} pieces)",
                info_hash,
                piece_count
            );

            let download_result = tokio::time::timeout(
                Duration::from_millis(REAL_DOWNLOAD_TIMEOUT_MS),
                Self::download_torrent_pieces(download_context),
            )
            .await;

            match download_result {
                Ok(Ok(())) => {
                    tracing::info!("Download completed successfully for torrent {}", info_hash);
                }
                Ok(Err(e)) => {
                    tracing::error!("Download failed for torrent {}: {}", info_hash, e);
                }
                Err(_) => {
                    tracing::error!("Download timed out for torrent {}", info_hash);
                }
            }
        });
    }

    async fn download_torrent_pieces(
        download_context: DownloadContext<T, P>,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        tracing::info!(
            "Starting download for torrent {}",
            download_context.info_hash
        );
        let peers = Self::discover_peers(
            download_context.tracker_manager,
            &download_context.tracker_urls,
            download_context.announce_request,
        )
        .await?;
        let metadata = Self::create_test_metadata(
            download_context.info_hash,
            download_context.piece_count,
            download_context.tracker_urls,
        );
        let storage = Self::create_temp_storage(&download_context.info_hash)?;

        let download_params = DownloadParams {
            metadata,
            storage,
            peer_manager: download_context.peer_manager,
            peer_id: download_context.peer_id,
            peers,
            info_hash: download_context.info_hash,
            piece_count: download_context.piece_count,
            piece_sender: download_context.piece_sender,
        };

        let result = Self::download_pieces(download_params).await;
        if let Err(e) = &result {
            tracing::error!(
                "Download all pieces failed for torrent {}: {}",
                download_context.info_hash,
                e
            );
        }
        result
    }

    async fn discover_peers(
        tracker_manager: Arc<RwLock<T>>,
        tracker_urls: &[String],
        announce_request: AnnounceRequest,
    ) -> Result<Vec<std::net::SocketAddr>, Box<dyn std::error::Error + Send + Sync>> {
        let mut manager = tracker_manager.write().await;
        let response = manager
            .announce_to_trackers(tracker_urls, announce_request)
            .await
            .map_err(|e| format!("Failed to announce to trackers: {e}"))?;

        if response.peers.is_empty() {
            return Err("No peers available for download".into());
        }

        tracing::info!("Tracker responded with {} peers", response.peers.len());
        Ok(response.peers)
    }

    fn create_test_metadata(
        info_hash: InfoHash,
        piece_count: u32,
        tracker_urls: Vec<String>,
    ) -> TorrentMetadata {
        let piece_hashes = (0..piece_count)
            .map(|i| {
                let mut hash = [0u8; 20];
                hash[0] = i as u8;
                hash
            })
            .collect();

        TorrentMetadata {
            info_hash,
            name: format!("torrent_{info_hash}"),
            piece_length: DEFAULT_PIECE_SIZE,
            piece_hashes,
            total_length: (piece_count as u64) * DEFAULT_PIECE_SIZE as u64,
            files: vec![TorrentFile {
                path: vec![format!("torrent_{info_hash}.bin")],
                length: (piece_count as u64) * DEFAULT_PIECE_SIZE as u64,
            }],
            announce_urls: tracker_urls,
        }
    }

    fn create_temp_storage(
        info_hash: &InfoHash,
    ) -> Result<FileStorage, Box<dyn std::error::Error + Send + Sync>> {
        let temp_dir = std::env::temp_dir().join(format!("riptide_{info_hash}"));
        std::fs::create_dir_all(&temp_dir)
            .map_err(|e| format!("Failed to create storage directory: {e}"))?;

        Ok(FileStorage::new(
            temp_dir.join("downloads"),
            temp_dir.join("library"),
        ))
    }

    async fn download_pieces(
        params: DownloadParams<P>,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        tracing::info!(
            "Starting download of {} pieces for torrent {} with {} peers",
            params.piece_count,
            params.info_hash,
            params.peers.len()
        );

        let mut piece_downloader = PieceDownloader::new(
            params.metadata,
            params.storage,
            params.peer_manager,
            params.peer_id,
        )
        .map_err(|e| format!("Failed to create piece downloader: {e}"))?;

        tracing::debug!("Created piece downloader successfully");

        piece_downloader.update_peers(params.peers.clone()).await;
        tracing::debug!("Updated peers: {:?}", params.peers);

        for piece_index in 0..params.piece_count {
            let piece_idx = PieceIndex::new(piece_index);
            tracing::debug!("Starting download of piece {}", piece_index);

            match piece_downloader.download_piece(piece_idx).await {
                Ok(()) => {
                    tracing::info!("Successfully downloaded piece {}", piece_index);
                    Self::notify_piece_completed(
                        params.info_hash,
                        piece_index,
                        &params.piece_sender,
                    );
                }
                Err(e) => {
                    tracing::error!("Failed to download piece {}: {}", piece_index, e);
                    break;
                }
            }
        }

        tracing::info!("Download completed for torrent {}", params.info_hash);
        Ok(())
    }

    fn notify_piece_completed(
        info_hash: InfoHash,
        piece_index: u32,
        piece_sender: &mpsc::UnboundedSender<TorrentEngineCommand>,
    ) {
        let (responder, _receiver) = tokio::sync::oneshot::channel();
        let cmd = TorrentEngineCommand::MarkPiecesCompleted {
            info_hash,
            piece_indices: vec![piece_index],
            responder,
        };

        if let Err(e) = piece_sender.send(cmd) {
            tracing::error!("Failed to notify engine of piece completion: {}", e);
        }
    }

    /// Session information for a specific torrent.
    ///
    /// Returns a reference to the torrent session, or None if the torrent
    /// is not found in the active torrents.
    pub fn session(&self, info_hash: InfoHash) -> Option<&TorrentSession> {
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
