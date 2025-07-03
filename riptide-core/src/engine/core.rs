//! Core torrent engine implementation for the actor model.

use std::collections::HashMap;
use std::sync::Arc;

use rand;
use tokio::sync::{RwLock, mpsc};
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
    /// Channel for internal piece completion notifications
    piece_completion_sender: mpsc::UnboundedSender<super::commands::TorrentEngineCommand>,
}

impl<P: PeerManager, T: TrackerManagement + 'static> TorrentEngine<P, T> {
    /// Creates new torrent engine with provided peer manager and tracker manager.
    pub fn new(
        config: RiptideConfig,
        peer_manager: P,
        tracker_manager: T,
        piece_completion_sender: mpsc::UnboundedSender<super::commands::TorrentEngineCommand>,
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
            self.get_fallback_trackers()
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
        let piece_count = session.piece_count;
        self.spawn_download_loop(info_hash, piece_count).await;

        Ok(())
    }

    /// Spawns the download loop for a specific torrent.
    ///
    /// Creates background tasks to handle piece downloading, peer communication,
    /// and progress tracking. The download loop runs until the torrent is
    /// complete or stopped. Uses realistic BitTorrent protocol simulation
    /// including peer discovery, piece requests, bandwidth limiting, and peer churn.
    async fn spawn_download_loop(&self, info_hash: InfoHash, piece_count: u32) {
        let config = self.config.clone();
        let piece_sender = self.piece_completion_sender.clone();
        let _peer_manager = self.peer_manager.clone();

        tokio::spawn(async move {
            tracing::info!(
                "Starting realistic BitTorrent download simulation for torrent {} ({} pieces)",
                info_hash,
                piece_count
            );

            // Simulation parameters from config
            let download_speed_bytes_per_sec = config.simulation.simulated_download_speed;
            let piece_size = config.torrent.default_piece_size;
            let max_peers = config.simulation.max_simulated_peers;
            let packet_loss_rate = config.simulation.packet_loss_rate;
            let network_latency_ms = config.simulation.network_latency_ms;

            // BitTorrent protocol simulation state
            let mut connected_peers = 0usize;
            let mut piece_requests = Vec::new();
            let mut completed_pieces = std::collections::HashSet::new();
            let mut peer_connection_times: HashMap<std::net::SocketAddr, std::time::Instant> =
                HashMap::new();
            let mut piece_rarity = vec![max_peers; piece_count as usize]; // How many peers have each piece

            // Optimized timing for testing while maintaining realistic behavior ratios
            let base_piece_time =
                (piece_size as f64 / download_speed_bytes_per_sec as f64).max(0.05); // Min 50ms per piece
            let update_interval = Duration::from_millis(25); // Fast updates for testing
            let peer_discovery_interval = Duration::from_millis(500); // Fast peer discovery for testing
            let mut last_peer_discovery = std::time::Instant::now();

            tracing::info!(
                "BitTorrent simulation: {} pieces, {} max peers, {:.1} MB/s target speed",
                piece_count,
                max_peers,
                download_speed_bytes_per_sec as f64 / 1_048_576.0
            );

            // Phase 1: Peer discovery simulation
            tracing::info!("Phase 1: Discovering peers for torrent {}", info_hash);
            for i in 0..std::cmp::min(max_peers, 10) {
                let peer_addr: std::net::SocketAddr =
                    format!("192.168.1.{}:6881", 100 + i).parse().unwrap();
                let connect_delay = Duration::from_millis(network_latency_ms + (i as u64 * 10)); // Faster connection delays

                sleep(connect_delay).await;

                // Simulate peer connection
                if rand::random::<f64>() > packet_loss_rate {
                    connected_peers += 1;
                    peer_connection_times.insert(peer_addr, std::time::Instant::now());

                    tracing::debug!(
                        "Connected to peer {} for torrent {} ({}/{} peers)",
                        peer_addr,
                        info_hash,
                        connected_peers,
                        max_peers
                    );
                } else {
                    tracing::debug!("Failed to connect to peer {} (packet loss)", peer_addr);
                }
            }

            // Phase 2: Piece downloading with realistic BitTorrent behavior
            tracing::info!("Phase 2: Downloading pieces using BitTorrent protocol simulation");

            while completed_pieces.len() < piece_count as usize {
                sleep(update_interval).await;

                // Periodic peer discovery
                if last_peer_discovery.elapsed() >= peer_discovery_interval
                    && connected_peers < max_peers
                {
                    if rand::random::<f64>() > 0.3 {
                        // 70% chance to find new peer
                        let new_peer_addr: std::net::SocketAddr =
                            format!("192.168.1.{}:6881", 200 + connected_peers)
                                .parse()
                                .unwrap();
                        connected_peers += 1;
                        peer_connection_times.insert(new_peer_addr, std::time::Instant::now());
                        tracing::debug!(
                            "Discovered new peer {} for torrent {}",
                            new_peer_addr,
                            info_hash
                        );
                    }
                    last_peer_discovery = std::time::Instant::now();
                }

                // Simulate peer churn (disconnections)
                if rand::random::<f64>() < 0.01 && connected_peers > 2 {
                    // 1% chance per update to lose a peer
                    connected_peers = connected_peers.saturating_sub(1);
                    tracing::debug!(
                        "Lost peer connection for torrent {} ({} peers remaining)",
                        info_hash,
                        connected_peers
                    );
                }

                // Find rarest pieces first (realistic BitTorrent strategy)
                let mut available_pieces: Vec<u32> = (0..piece_count)
                    .filter(|&i| !completed_pieces.contains(&i))
                    .collect();

                available_pieces.sort_by_key(|&i| piece_rarity[i as usize]);

                // Request pieces from peers
                for &piece_index in available_pieces.iter().take(connected_peers) {
                    if piece_requests.iter().any(|(idx, _)| *idx == piece_index) {
                        continue; // Already requesting this piece
                    }

                    // Calculate realistic download time for this piece
                    let rarity_factor = 1.0
                        + (max_peers - piece_rarity[piece_index as usize]) as f64
                            / max_peers as f64;
                    let latency_factor = 1.0 + network_latency_ms as f64 / 10000.0; // Reduced latency impact
                    let peer_load_factor = 1.0 + connected_peers as f64 / (max_peers as f64 * 4.0); // Reduced load impact

                    let piece_download_time =
                        (base_piece_time * rarity_factor * latency_factor * peer_load_factor)
                            .max(0.02); // Min 20ms
                    let completion_time =
                        std::time::Instant::now() + Duration::from_secs_f64(piece_download_time);

                    piece_requests.push((piece_index, completion_time));

                    tracing::debug!(
                        "Requesting piece {} from peers (ETA: {:.1}s, rarity: {}/{} peers)",
                        piece_index,
                        piece_download_time,
                        piece_rarity[piece_index as usize],
                        max_peers
                    );
                }

                // Process completed piece requests
                let now = std::time::Instant::now();
                let mut completed_requests = Vec::new();

                for (i, (piece_index, completion_time)) in piece_requests.iter().enumerate() {
                    if now >= *completion_time {
                        // Simulate piece verification and occasional hash failures
                        if rand::random::<f64>() > 0.02 {
                            // 98% success rate
                            completed_pieces.insert(*piece_index);
                            completed_requests.push(i);

                            // Update piece rarity (other peers now have this piece)
                            for rarity in piece_rarity.iter_mut() {
                                if rand::random::<f64>() < 0.1 {
                                    // 10% chance other peers also got it
                                    *rarity = (*rarity + 1).min(max_peers);
                                }
                            }

                            let progress = completed_pieces.len() as f32 / piece_count as f32;

                            tracing::debug!(
                                "Torrent {} verified piece {} ({:.1}% complete, {} peers)",
                                info_hash,
                                piece_index,
                                progress * 100.0,
                                connected_peers
                            );

                            // Send piece completion back to engine actor
                            let cmd = super::commands::TorrentEngineCommand::PieceCompleted {
                                info_hash,
                                piece_index: *piece_index,
                            };

                            if let Err(e) = piece_sender.send(cmd) {
                                tracing::error!("Failed to send piece completion: {}", e);
                                return;
                            }
                        } else {
                            completed_requests.push(i);
                            tracing::debug!(
                                "Piece {} failed verification, will retry",
                                piece_index
                            );
                        }
                    }
                }

                // Remove completed requests
                for &i in completed_requests.iter().rev() {
                    piece_requests.remove(i);
                }

                // Simulate bandwidth throttling during peak usage
                if completed_pieces.len() > piece_count as usize / 2 {
                    sleep(Duration::from_millis(10)).await; // Slight throttling during endgame
                }
            }

            tracing::info!(
                "BitTorrent download simulation completed for torrent {} (downloaded {} pieces from {} peers)",
                info_hash,
                piece_count,
                connected_peers
            );

            // Simulate post-download seeding behavior
            if config.simulation.enabled {
                tracing::info!("Entering seeding mode for torrent {}", info_hash);
                sleep(Duration::from_millis(50)).await; // Brief seeding simulation for testing
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
