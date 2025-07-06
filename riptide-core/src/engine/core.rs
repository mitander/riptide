//! Core torrent engine implementation for the actor model.

use std::collections::HashMap;
use std::env::temp_dir;
use std::error::Error;
use std::fs::create_dir_all;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Instant;

use tokio::sync::oneshot::channel;
use tokio::sync::{RwLock, mpsc};
use tokio::time::Duration;

use super::commands::{
    DownloadStats, EngineStats, TorrentEngineCommand, TorrentSession, TorrentSessionParams,
};
use crate::config::RiptideConfig;
use crate::storage::FileStorage;
use crate::torrent::downloader::PieceDownloader;
use crate::torrent::parsing::types::TorrentMetadata;
use crate::torrent::tracker::{AnnounceEvent, AnnounceRequest, TrackerManagement};
use crate::torrent::{
    AdaptivePiecePicker, AdaptiveStreamingPiecePicker, BencodeTorrentParser, BufferStatus,
    InfoHash, PeerId, PeerManager, PiecePicker, TorrentError, TorrentParser,
};

// Constants
const DEFAULT_BITTORRENT_PORT: u16 = 6881;
const DOWNLOAD_TIMEOUT_MS: u64 = 600000; // 10 minutes for development

/// Parameters for piece downloading operations.
struct DownloadParams<P: PeerManager> {
    metadata: TorrentMetadata,
    storage: FileStorage,
    peer_manager: Arc<RwLock<P>>,
    peer_id: PeerId,
    peers: Vec<SocketAddr>,
    info_hash: InfoHash,
    piece_count: u32,
    piece_sender: mpsc::UnboundedSender<TorrentEngineCommand>,
    piece_picker: Arc<RwLock<AdaptiveStreamingPiecePicker>>,
}

/// Context for download operations.
struct DownloadContext<T: TrackerManagement, P: PeerManager> {
    info_hash: InfoHash,
    metadata: TorrentMetadata,
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
    /// Stored torrent metadata
    torrent_metadata: HashMap<InfoHash, TorrentMetadata>,
    /// Torrent parser for metadata extraction
    parser: BencodeTorrentParser,
    /// Configuration
    config: RiptideConfig,
    /// Our peer ID for BitTorrent protocol
    peer_id: PeerId,
    /// Channel for internal piece completion notifications
    piece_completion_sender: mpsc::UnboundedSender<TorrentEngineCommand>,
    /// Active piece pickers for streaming-aware piece selection
    piece_pickers: HashMap<InfoHash, Arc<RwLock<AdaptiveStreamingPiecePicker>>>,
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
            torrent_metadata: HashMap::new(),
            parser: BencodeTorrentParser::new(),
            config,
            peer_id: PeerId::generate(),
            piece_completion_sender,
            piece_pickers: HashMap::new(),
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
        let info_hash = metadata.info_hash;

        let session = TorrentSession::new(TorrentSessionParams {
            info_hash,
            piece_count: metadata.piece_hashes.len() as u32,
            piece_size: metadata.piece_length,
            total_size: metadata.total_length,
            filename: metadata.name.clone(),
            tracker_urls: metadata.announce_urls.clone(),
        });

        self.active_torrents.insert(info_hash, session);
        self.torrent_metadata.insert(info_hash, metadata);
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
        session.started_at = Instant::now();

        // Announce to trackers to discover peers and start downloading
        // In development mode, this will use simulated components transparently
        let tracker_manager = self.tracker_manager.clone();
        let peer_manager = self.peer_manager.clone();
        let tracker_urls = session.tracker_urls.clone();
        let peer_id = self.peer_id;
        let piece_count = session.piece_count;
        let _piece_size = session.piece_size;
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

        // Get the stored metadata for this torrent
        let metadata = self
            .torrent_metadata
            .get(&info_hash)
            .ok_or(TorrentError::TorrentNotFound { info_hash })?
            .clone();

        // Create and store piece picker for this torrent
        let piece_picker = Arc::new(RwLock::new(AdaptiveStreamingPiecePicker::new(
            piece_count,
            metadata.piece_length,
        )));
        self.piece_pickers
            .insert(info_hash, Arc::clone(&piece_picker));

        // Configure upload manager for streaming if using TcpPeerManager
        self.configure_upload_manager_for_streaming(info_hash, metadata.piece_length as u64)
            .await;

        // Start BitTorrent download process
        let download_context = DownloadContext {
            info_hash,
            metadata,
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
        let info_hash = download_context.info_hash;
        let piece_picker = self.piece_pickers.get(&info_hash).cloned();

        tokio::spawn(async move {
            let _piece_count = download_context.piece_count;

            if let Some(piece_picker) = piece_picker {
                let download_result = tokio::time::timeout(
                    Duration::from_millis(DOWNLOAD_TIMEOUT_MS),
                    Self::download_torrent_pieces(download_context, piece_picker),
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
            } else {
                tracing::error!("No piece picker found for torrent {}", info_hash);
            }
        });
    }

    async fn download_torrent_pieces(
        download_context: DownloadContext<T, P>,
        piece_picker: Arc<RwLock<AdaptiveStreamingPiecePicker>>,
    ) -> Result<(), Box<dyn Error + Send + Sync>> {
        let peers = Self::discover_peers(
            download_context.tracker_manager,
            &download_context.tracker_urls,
            download_context.announce_request,
        )
        .await?;
        let metadata = download_context.metadata;
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
            piece_picker,
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
    ) -> Result<Vec<SocketAddr>, Box<dyn Error + Send + Sync>> {
        let mut manager = tracker_manager.write().await;
        let response = manager
            .announce_to_trackers(tracker_urls, announce_request)
            .await
            .map_err(|e| format!("Failed to announce to trackers: {e}"))?;

        tracing::info!(
            "Tracker response received - interval: {}, complete: {}, incomplete: {}",
            response.interval,
            response.complete,
            response.incomplete
        );

        if response.peers.is_empty() {
            return Err("No peers available for download".into());
        }

        tracing::info!("Tracker responded with {} peers", response.peers.len());
        for (i, peer) in response.peers.iter().enumerate() {
            tracing::debug!("Peer {i}: {peer}");
        }
        Ok(response.peers)
    }

    fn create_temp_storage(
        info_hash: &InfoHash,
    ) -> Result<FileStorage, Box<dyn Error + Send + Sync>> {
        let temp_dir_path = temp_dir().join(format!("riptide_{info_hash}"));
        create_dir_all(&temp_dir_path)
            .map_err(|e| format!("Failed to create storage directory: {e}"))?;

        Ok(FileStorage::new(
            temp_dir_path.join("downloads"),
            temp_dir_path.join("library"),
        ))
    }

    async fn download_pieces(
        params: DownloadParams<P>,
    ) -> Result<(), Box<dyn Error + Send + Sync>> {
        tracing::info!(
            "Starting download of {} pieces for torrent {} with {} peers",
            params.piece_count,
            params.info_hash,
            params.peers.len()
        );

        let peer_manager_for_stats = Arc::clone(&params.peer_manager);

        let mut piece_downloader = PieceDownloader::new(
            params.metadata,
            params.storage,
            params.peer_manager,
            params.peer_id,
        )
        .map_err(|e| format!("Failed to create piece downloader: {e}"))?;

        tracing::debug!("Created piece downloader successfully");

        piece_downloader.update_peers(params.peers.clone()).await;
        tracing::debug!("Updated piece downloader with {} peers", params.peers.len());
        tracing::debug!("Peer addresses: {:?}", params.peers);

        let start_time = Instant::now();
        let mut last_speed_update = start_time;
        let mut bytes_downloaded = 0u64;
        let mut pieces_downloaded = 0u32;

        // Use adaptive piece picker for priority-based piece selection
        while pieces_downloaded < params.piece_count {
            let piece_idx = {
                let mut picker = params.piece_picker.write().await;
                if let Some(next_piece) = picker.next_piece() {
                    next_piece
                } else {
                    // No more pieces to download, break
                    break;
                }
            };

            let piece_start = Instant::now();

            match piece_downloader.download_piece(piece_idx).await {
                Ok(()) => {
                    // Mark piece as completed in picker
                    {
                        let mut picker = params.piece_picker.write().await;
                        picker.mark_completed(piece_idx);
                    }

                    pieces_downloaded += 1;

                    // Calculate download statistics
                    let piece_duration = piece_start.elapsed();
                    bytes_downloaded += piece_downloader.metadata().piece_length as u64;

                    // Update speed statistics every 2 seconds or on completion
                    let now = Instant::now();
                    if now.duration_since(last_speed_update) >= Duration::from_secs(2)
                        || pieces_downloaded == params.piece_count
                    {
                        let total_duration = now.duration_since(start_time);
                        let download_speed_bps = if total_duration.as_secs() > 0 {
                            bytes_downloaded / total_duration.as_secs()
                        } else {
                            0
                        };

                        // Get upload statistics from peer manager
                        let (bytes_uploaded, upload_speed_bps) = {
                            let peer_manager = peer_manager_for_stats.read().await;
                            peer_manager.upload_stats().await
                        };

                        // Send download stats update command to engine
                        Self::update_download_stats_notification(
                            params.info_hash,
                            DownloadStats {
                                download_speed_bps,
                                upload_speed_bps,
                                bytes_downloaded,
                                bytes_uploaded,
                            },
                            &params.piece_sender,
                        );

                        last_speed_update = now;
                    }

                    tracing::debug!(
                        "Downloaded piece {} in {:?} ({} bytes) - priority-based selection",
                        piece_idx.as_u32(),
                        piece_duration,
                        piece_downloader.metadata().piece_length
                    );

                    Self::notify_piece_completed(
                        params.info_hash,
                        piece_idx.as_u32(),
                        &params.piece_sender,
                    );
                }
                Err(e) => {
                    tracing::warn!(
                        "Failed to download piece {}, will retry: {}",
                        piece_idx.as_u32(),
                        e
                    );
                    // Don't break on individual piece failure - piece picker will retry
                    continue;
                }
            }
        }

        Ok(())
    }

    fn notify_piece_completed(
        info_hash: InfoHash,
        piece_index: u32,
        piece_sender: &mpsc::UnboundedSender<TorrentEngineCommand>,
    ) {
        let (responder, _receiver) = channel();
        let cmd = TorrentEngineCommand::MarkPiecesCompleted {
            info_hash,
            piece_indices: vec![piece_index],
            responder,
        };

        if let Err(e) = piece_sender.send(cmd) {
            tracing::error!("Failed to notify engine of piece completion: {}", e);
        }
    }

    fn update_download_stats_notification(
        info_hash: InfoHash,
        stats: DownloadStats,
        piece_sender: &mpsc::UnboundedSender<TorrentEngineCommand>,
    ) {
        let cmd = TorrentEngineCommand::UpdateDownloadStats { info_hash, stats };

        if let Err(e) = piece_sender.send(cmd) {
            tracing::error!("Failed to notify engine of download stats update: {}", e);
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

    /// Updates download statistics for a torrent.
    ///
    /// Updates the download/upload speeds and total bytes transferred for
    /// the specified torrent. This method is called periodically during
    /// active downloads to provide real-time statistics.
    ///
    /// # Errors
    /// - `TorrentError::TorrentNotFound` - Info hash not in active torrents
    pub fn update_download_stats(
        &mut self,
        info_hash: InfoHash,
        stats: DownloadStats,
    ) -> Result<(), TorrentError> {
        let session = self
            .active_torrents
            .get_mut(&info_hash)
            .ok_or(TorrentError::TorrentNotFound { info_hash })?;

        session.update_speed_stats(stats.clone());

        tracing::debug!(
            "Download stats updated for {}: speed={}B/s ({}B/s up), total={}B down ({}B up)",
            info_hash,
            stats.download_speed_bps,
            stats.upload_speed_bps,
            stats.bytes_downloaded,
            stats.bytes_uploaded
        );

        Ok(())
    }

    /// Requests prioritization of pieces around a seek position for streaming.
    ///
    /// This method signals the torrent engine to prioritize downloading pieces
    /// around the specified byte position to enable smooth seeking in video playback.
    /// The method updates the piece picker priorities synchronously to avoid
    /// blocking the actor loop.
    ///
    /// # Errors
    /// - `TorrentError::TorrentNotFound` - Info hash not in active torrents
    pub fn seek_to_position(
        &mut self,
        info_hash: InfoHash,
        byte_position: u64,
        buffer_size: u64,
    ) -> Result<(), TorrentError> {
        let session = self
            .active_torrents
            .get(&info_hash)
            .ok_or(TorrentError::TorrentNotFound { info_hash })?;

        // Calculate which pieces contain the seek position
        let piece_size = session.piece_size as u64;
        let seek_piece = byte_position / piece_size;
        let buffer_pieces = (buffer_size / piece_size) + 1; // Round up

        tracing::info!(
            "Seek request for torrent {}: position={}B (piece {}), buffer={}B ({} pieces)",
            info_hash,
            byte_position,
            seek_piece,
            buffer_size,
            buffer_pieces
        );

        // Update piece picker priorities synchronously
        if let Some(piece_picker) = self.piece_pickers.get(&info_hash) {
            // Use try_write to avoid blocking the actor
            if let Ok(mut picker) = piece_picker.try_write() {
                picker.request_seek_position(byte_position, buffer_size);
                tracing::info!(
                    "Successfully updated piece picker priorities for seek request to torrent {}",
                    info_hash
                );
            } else {
                tracing::warn!(
                    "Could not acquire write lock on piece picker for torrent {} - seek request deferred",
                    info_hash
                );
                // Could implement a deferred seek queue here if needed
            }
        } else {
            tracing::warn!(
                "No active piece picker found for torrent {} - seek request ignored",
                info_hash
            );
        }

        Ok(())
    }

    /// Updates buffer strategy for adaptive piece picking.
    ///
    /// Adjusts the buffer parameters for the specified torrent based on
    /// current playback conditions to optimize buffering behavior.
    ///
    /// # Errors
    /// - `TorrentError::TorrentNotFound` - Info hash not in active torrents
    pub fn update_buffer_strategy(
        &mut self,
        info_hash: InfoHash,
        playback_speed: f64,
        available_bandwidth: u64,
    ) -> Result<(), TorrentError> {
        // Validate torrent exists
        self.active_torrents
            .get(&info_hash)
            .ok_or(TorrentError::TorrentNotFound { info_hash })?;

        // Update piece picker buffer strategy if available
        if let Some(piece_picker) = self.piece_pickers.get(&info_hash) {
            if let Ok(mut picker) = piece_picker.try_write() {
                picker.update_buffer_strategy(playback_speed, available_bandwidth);
                tracing::info!(
                    "Updated buffer strategy for torrent {}: speed={:.1}x, bandwidth={}MB/s",
                    info_hash,
                    playback_speed,
                    available_bandwidth / 1_000_000
                );
            } else {
                tracing::warn!(
                    "Could not acquire write lock on piece picker for torrent {} - buffer update deferred",
                    info_hash
                );
            }
        } else {
            tracing::warn!(
                "No active piece picker found for torrent {} - buffer update ignored",
                info_hash
            );
        }

        Ok(())
    }

    /// Gets current buffer status for a torrent.
    ///
    /// Returns detailed information about the buffering state around the
    /// current playback position.
    ///
    /// # Errors
    /// - `TorrentError::TorrentNotFound` - Info hash not in active torrents
    pub fn buffer_status(&self, info_hash: InfoHash) -> Result<BufferStatus, TorrentError> {
        // Validate torrent exists
        self.active_torrents
            .get(&info_hash)
            .ok_or(TorrentError::TorrentNotFound { info_hash })?;

        // Get buffer status from piece picker if available
        if let Some(piece_picker) = self.piece_pickers.get(&info_hash) {
            if let Ok(picker) = piece_picker.try_read() {
                Ok(picker.buffer_status())
            } else {
                tracing::warn!(
                    "Could not acquire read lock on piece picker for torrent {}",
                    info_hash
                );
                // Return default buffer status
                Ok(BufferStatus {
                    current_position: 0,
                    bytes_ahead: 0,
                    bytes_behind: 0,
                    buffer_health: 0.0,
                    pieces_in_buffer: 0,
                    buffer_duration: 0.0,
                })
            }
        } else {
            tracing::warn!("No active piece picker found for torrent {}", info_hash);
            // Return default buffer status
            Ok(BufferStatus {
                current_position: 0,
                bytes_ahead: 0,
                bytes_behind: 0,
                buffer_health: 0.0,
                pieces_in_buffer: 0,
                buffer_duration: 0.0,
            })
        }
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
        peer_address: SocketAddr,
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

        // Calculate total uploaded bytes from peer manager
        let total_uploaded = {
            let peer_manager = self.peer_manager.read().await;
            let (bytes_uploaded, _speed) = peer_manager.upload_stats().await;
            bytes_uploaded
        };

        let average_progress = if active_torrents > 0 {
            total_progress / active_torrents as f32
        } else {
            0.0
        };

        EngineStats {
            active_torrents,
            total_peers,
            bytes_downloaded: total_downloaded,
            bytes_uploaded: total_uploaded,
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

    /// Configures upload manager for streaming-optimized behavior.
    ///
    /// Uses the PeerManager trait interface to configure upload throttling
    /// without requiring downcasting to specific implementation types.
    pub async fn configure_upload_manager_for_streaming(
        &self,
        info_hash: InfoHash,
        piece_size: u64,
    ) {
        let mut peer_manager = self.peer_manager.write().await;

        let total_bandwidth = self.config.network.download_limit.unwrap_or(10_000_000); // 10MB/s default

        // Use trait method instead of downcasting
        if let Err(e) = peer_manager
            .configure_upload_manager(info_hash, piece_size, total_bandwidth)
            .await
        {
            tracing::warn!(
                "Failed to configure upload manager for torrent {}: {}",
                info_hash,
                e
            );
        } else {
            tracing::info!(
                "Configured streaming upload throttling for torrent {} with piece_size={}",
                info_hash,
                piece_size
            );
        }
    }

    /// Updates streaming position across piece picker and upload manager.
    ///
    /// Coordinates between piece picker and upload manager using trait
    /// interfaces to avoid implementation-specific coupling.
    pub async fn update_streaming_position_coordinated(
        &self,
        info_hash: InfoHash,
        byte_position: u64,
    ) -> Result<(), TorrentError> {
        // Update piece picker priorities
        if let Some(piece_picker) = self.piece_pickers.get(&info_hash)
            && let Ok(mut picker) = piece_picker.try_write()
        {
            picker.request_seek_position(byte_position, 10_000_000); // 10MB buffer
        }

        let mut peer_manager = self.peer_manager.write().await;
        if let Err(e) = peer_manager
            .update_streaming_position(info_hash, byte_position)
            .await
        {
            tracing::warn!(
                "Failed to update streaming position for torrent {}: {}",
                info_hash,
                e
            );
        }

        Ok(())
    }

    /// Stops downloading a torrent by its info hash.
    ///
    /// Stops the download process for the specified torrent, closes peer
    /// connections, and releases resources. The torrent remains in the
    /// engine's list but is marked as not downloading.
    ///
    /// # Errors
    /// - `TorrentError::TorrentNotFound` - Info hash not in active torrents
    pub fn stop_download(&mut self, info_hash: InfoHash) -> Result<(), TorrentError> {
        let session = self
            .active_torrents
            .get_mut(&info_hash)
            .ok_or(TorrentError::TorrentNotFound { info_hash })?;

        if !session.is_downloading {
            return Ok(()); // Already stopped
        }

        session.is_downloading = false;

        tracing::info!(
            "Stopped download for torrent {} ({})",
            info_hash,
            session.filename
        );

        // Clean up piece picker for this torrent
        self.piece_pickers.remove(&info_hash);

        // TODO: In a full implementation, we would also:
        // - Cancel any ongoing download tasks
        // - Close peer connections for this torrent
        // - Announce "stopped" event to trackers
        // For now, we just mark the session as stopped

        Ok(())
    }
}
