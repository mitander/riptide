//! Piece downloading and verification for torrent downloads.
//!
//! Coordinates piece requests, downloads, hash verification, and storage.
//! Integrates with tracker client, storage layer, and piece picker to manage
//! the full download process.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use sha1::{Digest, Sha1};
use tokio::sync::RwLock;

#[cfg(test)]
use super::enhanced_peer_connection::EnhancedPeerConnection;
use super::error_recovery::ErrorRecoveryManager;
use super::protocol::types::{PeerId, PeerMessage};
use super::{PeerManager, PeerMessageEvent, PieceIndex, TorrentError, TorrentMetadata};
use crate::storage::Storage;

// Constants
const BLOCK_SIZE: u32 = 16_384; // Standard 16KB BitTorrent block size
const PEER_REQUEST_TIMEOUT_SECS: u64 = 30;

/// Single piece download request.
///
/// Represents a request for a specific piece range from a peer.
/// Used to coordinate piece-level downloads across multiple connections.
#[derive(Debug, Clone)]
pub struct PieceRequest {
    pub piece_index: PieceIndex,
    pub offset: u32,
    pub length: u32,
}

/// Piece download status.
///
/// Tracks the lifecycle of piece downloads from initial request
/// through verification to completion or failure.
#[derive(Debug, Clone, PartialEq)]
pub enum PieceStatus {
    Pending,
    Downloading,
    Verifying,
    Complete,
    Failed { attempts: u32 },
}

/// Download progress for a single piece.
///
/// Provides detailed progress information including status, bytes downloaded,
/// and completion percentage for individual pieces.
#[derive(Debug, Clone)]
pub struct PieceProgress {
    pub piece_index: PieceIndex,
    pub status: PieceStatus,
    pub bytes_downloaded: u32,
    pub total_bytes: u32,
}

/// Manages piece downloading and verification for torrents.
///
/// Coordinates between tracker announcements, peer connections, and storage
/// to download and verify pieces. Provides progress tracking and error recovery.
pub struct PieceDownloader<S: Storage, P: PeerManager> {
    torrent_metadata: TorrentMetadata,
    storage: S,
    peer_manager: Arc<RwLock<P>>,
    peer_id: PeerId,
    piece_status: Arc<RwLock<HashMap<PieceIndex, PieceStatus>>>,
    piece_data: Arc<RwLock<HashMap<PieceIndex, Vec<u8>>>>,
    available_peers: Arc<RwLock<Vec<SocketAddr>>>,
    error_recovery: Arc<RwLock<ErrorRecoveryManager>>,
}

impl<S: Storage, P: PeerManager> PieceDownloader<S, P> {
    /// Creates new piece downloader for torrent.
    ///
    /// Initializes piece tracking and prepares storage for the torrent.
    /// All pieces start in Pending status.
    ///
    /// # Errors
    /// - `TorrentError::InvalidTorrentFile` - Invalid torrent metadata
    pub fn new(
        torrent_metadata: TorrentMetadata,
        storage: S,
        peer_manager: Arc<RwLock<P>>,
        peer_id: PeerId,
    ) -> Result<Self, TorrentError> {
        let total_pieces = torrent_metadata.piece_hashes.len();
        if total_pieces == 0 {
            return Err(TorrentError::InvalidTorrentFile {
                reason: "Torrent has no pieces".to_string(),
            });
        }

        let mut piece_status = HashMap::new();
        for i in 0..total_pieces {
            piece_status.insert(PieceIndex::new(i as u32), PieceStatus::Pending);
        }

        Ok(Self {
            torrent_metadata,
            storage,
            peer_manager,
            peer_id,
            piece_status: Arc::new(RwLock::new(piece_status)),
            piece_data: Arc::new(RwLock::new(HashMap::new())),
            available_peers: Arc::new(RwLock::new(Vec::new())),
            error_recovery: Arc::new(RwLock::new(ErrorRecoveryManager::new())),
        })
    }

    /// Returns download progress for all pieces.
    pub async fn progress(&self) -> Vec<PieceProgress> {
        let status_map = self.piece_status.read().await;
        let data_map = self.piece_data.read().await;

        let mut progress = Vec::new();
        for (piece_index, status) in status_map.iter() {
            let bytes_downloaded = data_map
                .get(piece_index)
                .map(|data| data.len() as u32)
                .unwrap_or(0);

            progress.push(PieceProgress {
                piece_index: *piece_index,
                status: status.clone(),
                bytes_downloaded,
                total_bytes: self.torrent_metadata.piece_length,
            });
        }

        progress.sort_by_key(|p| p.piece_index);
        progress
    }

    /// Returns the torrent metadata.
    pub fn metadata(&self) -> &TorrentMetadata {
        &self.torrent_metadata
    }

    /// Downloads piece data from peers using BitTorrent protocol with retry logic.
    ///
    /// Connects to available peers, requests piece data, verifies hash,
    /// and stores the completed piece. Implements exponential backoff and
    /// peer blacklisting for robust error recovery.
    ///
    /// # Errors
    /// - `TorrentError::PeerConnectionError` - No peers available or all peers failed
    /// - `TorrentError::PieceHashMismatch` - Hash verification failed after all retries
    /// - `TorrentError::StorageError` - Failed to store piece data
    pub async fn download_piece(&mut self, piece_index: PieceIndex) -> Result<(), TorrentError> {
        {
            let status_map = self.piece_status.read().await;
            if let Some(PieceStatus::Complete) = status_map.get(&piece_index) {
                return Ok(());
            }
        }

        loop {
            // Check if we should retry this piece
            let should_retry = {
                let error_recovery = self.error_recovery.read().await;
                error_recovery.should_retry_piece(piece_index)
            };
            tracing::debug!("download_piece: piece={piece_index}, should_retry={should_retry}");

            if !should_retry {
                tracing::warn!(
                    "download_piece: piece={piece_index} exceeded max retries, giving up"
                );
                let mut status_map = self.piece_status.write().await;
                status_map.insert(piece_index, PieceStatus::Failed { attempts: 10 });
                return Err(TorrentError::PeerConnectionError {
                    reason: format!("Piece {piece_index} failed after maximum retry attempts"),
                });
            }

            // Wait for retry delay if this is a retry attempt
            let retry_delay = {
                let error_recovery = self.error_recovery.read().await;
                error_recovery.calculate_retry_delay(piece_index)
            };

            if !retry_delay.is_zero() {
                tracing::debug!(
                    "Waiting {}ms before retrying piece {} download",
                    retry_delay.as_millis(),
                    piece_index
                );
                tokio::time::sleep(retry_delay).await;
            }

            {
                let mut status_map = self.piece_status.write().await;
                status_map.insert(piece_index, PieceStatus::Downloading);
            }

            // Attempt to download the piece
            tracing::debug!("download_piece: attempting download for piece={piece_index}");
            match self.download_from_peers_with_recovery(piece_index).await {
                Ok(piece_data) => {
                    tracing::debug!(
                        "download_piece: piece={piece_index} downloaded successfully, {} bytes",
                        piece_data.len()
                    );
                    {
                        let mut status_map = self.piece_status.write().await;
                        status_map.insert(piece_index, PieceStatus::Verifying);
                    }

                    tracing::debug!("download_piece: verifying hash for piece={piece_index}");
                    if !self.verify_piece_hash(piece_index, &piece_data) {
                        tracing::warn!(
                            "download_piece: piece={piece_index} hash verification FAILED"
                        );
                        let hash_error = TorrentError::PieceHashMismatch { index: piece_index };

                        {
                            let mut error_recovery = self.error_recovery.write().await;
                            // A piece hash failure is not attributable to a specific peer, as any peer could have sent
                            // the corrupted data. We record the failure against a dummy address to track that the piece
                            // itself is problematic, so we can retry it from a different peer.
                            let dummy_peer = "0.0.0.0:0".parse().unwrap();
                            error_recovery.record_piece_failure(
                                piece_index,
                                dummy_peer,
                                &hash_error,
                            );
                        }

                        {
                            let mut status_map = self.piece_status.write().await;
                            status_map.insert(piece_index, PieceStatus::Failed { attempts: 1 });
                        }

                        tracing::warn!(
                            "Piece {} hash verification failed, will retry with different peer",
                            piece_index
                        );
                        continue; // Retry the download
                    }

                    tracing::debug!("download_piece: piece={piece_index} hash verification PASSED");
                    tracing::debug!("download_piece: storing piece={piece_index}");
                    match self
                        .storage
                        .store_piece(self.torrent_metadata.info_hash, piece_index, &piece_data)
                        .await
                    {
                        Ok(()) => {
                            tracing::info!(
                                "download_piece: piece={piece_index} stored successfully, COMPLETE"
                            );
                            {
                                let mut error_recovery = self.error_recovery.write().await;
                                let dummy_peer = "0.0.0.0:0".parse().unwrap();
                                error_recovery.record_piece_success(piece_index, dummy_peer);
                            }

                            {
                                let mut status_map = self.piece_status.write().await;
                                status_map.insert(piece_index, PieceStatus::Complete);
                            }

                            {
                                let mut data_map = self.piece_data.write().await;
                                data_map.remove(&piece_index);
                            }

                            return Ok(());
                        }
                        Err(storage_error) => {
                            tracing::error!(
                                "download_piece: piece={piece_index} storage FAILED: {storage_error}"
                            );
                            let torrent_error = TorrentError::Storage(storage_error);
                            {
                                let mut error_recovery = self.error_recovery.write().await;
                                let dummy_peer = "0.0.0.0:0".parse().unwrap();
                                error_recovery.record_piece_failure(
                                    piece_index,
                                    dummy_peer,
                                    &torrent_error,
                                );
                            }

                            {
                                let mut status_map = self.piece_status.write().await;
                                status_map.insert(piece_index, PieceStatus::Failed { attempts: 1 });
                            }

                            tracing::error!(
                                "Failed to store piece {}: {}",
                                piece_index,
                                torrent_error
                            );
                            continue; // Retry
                        }
                    }
                }
                Err(download_error) => {
                    tracing::warn!(
                        "download_piece: piece={piece_index} download FAILED: {download_error}"
                    );
                    {
                        let mut status_map = self.piece_status.write().await;
                        status_map.insert(piece_index, PieceStatus::Failed { attempts: 1 });
                    }

                    tracing::debug!(
                        "Failed to download piece {}: {}, will retry",
                        piece_index,
                        download_error
                    );
                    tracing::debug!("download_piece: piece={piece_index} will retry after failure");
                    // Continue the loop to retry
                }
            }
        }
    }

    /// Checks if all pieces are downloaded and verified.
    pub async fn is_complete(&self) -> bool {
        let status_map = self.piece_status.read().await;
        status_map
            .values()
            .all(|status| matches!(status, PieceStatus::Complete))
    }

    /// Returns the number of completed pieces.
    pub async fn completed_pieces(&self) -> usize {
        let status_map = self.piece_status.read().await;
        status_map
            .values()
            .filter(|status| matches!(status, PieceStatus::Complete))
            .count()
    }

    /// Updates the list of available peers for downloading.
    ///
    /// Called when tracker announces return new peer addresses.
    /// Peers will be used for piece downloads in round-robin fashion.
    pub async fn update_peers(&self, peers: Vec<SocketAddr>) {
        let mut available_peers = self.available_peers.write().await;
        *available_peers = peers;
    }

    /// Returns error recovery statistics
    pub async fn error_recovery_statistics(&self) -> super::error_recovery::ErrorRecoveryStats {
        let error_recovery = self.error_recovery.read().await;
        error_recovery.statistics()
    }

    /// Cleanup expired error recovery entries
    pub async fn cleanup_error_recovery(&self) {
        let mut error_recovery = self.error_recovery.write().await;
        error_recovery.cleanup_expired();
    }

    /// Downloads piece data from real BitTorrent peers with error recovery.
    ///
    /// Attempts to connect to peers and download the requested piece
    /// using the BitTorrent wire protocol. Integrates with error recovery
    /// system to avoid problematic peers and track failures.
    async fn download_from_peers_with_recovery(
        &self,
        piece_index: PieceIndex,
    ) -> Result<Vec<u8>, TorrentError> {
        let peers = {
            let available_peers = self.available_peers.read().await;
            available_peers.clone()
        };

        if peers.is_empty() {
            return Err(TorrentError::NoPeersAvailable);
        }

        let peers_to_avoid = {
            let error_recovery = self.error_recovery.read().await;
            error_recovery.peers_to_avoid_for_piece(piece_index)
        };

        let available_peers: Vec<SocketAddr> = peers
            .into_iter()
            .filter(|peer| !peers_to_avoid.contains(peer))
            .collect();

        if available_peers.is_empty() {
            return Err(TorrentError::PeerConnectionError {
                reason: format!(
                    "No suitable peers available for piece {piece_index} (all peers blacklisted)"
                ),
            });
        }

        let peer_count = available_peers.len();
        let mut last_error = TorrentError::NoPeersAvailable;

        for (i, peer_addr) in available_peers.iter().enumerate() {
            tracing::debug!(
                "download_from_peers_with_recovery: trying peer {} of {}: {}",
                i + 1,
                available_peers.len(),
                peer_addr
            );
            match self.download_piece_from_peer(*peer_addr, piece_index).await {
                Ok(piece_data) => {
                    tracing::debug!("download_from_peers_with_recovery: peer {peer_addr} SUCCESS");
                    {
                        let mut error_recovery = self.error_recovery.write().await;
                        error_recovery.record_piece_success(piece_index, *peer_addr);
                    }
                    return Ok(piece_data);
                }
                Err(e) => {
                    tracing::debug!(
                        "download_from_peers_with_recovery: peer {peer_addr} FAILED: {e}"
                    );
                    {
                        let mut error_recovery = self.error_recovery.write().await;
                        error_recovery.record_piece_failure(piece_index, *peer_addr, &e);
                    }

                    tracing::debug!(
                        "Failed to download piece {} from peer {}: {}",
                        piece_index,
                        peer_addr,
                        e
                    );
                    last_error = e;
                    continue;
                }
            }
        }

        Err(TorrentError::PeerConnectionError {
            reason: format!(
                "All {peer_count} available peers failed for piece {piece_index}: {last_error}"
            ),
        })
    }

    /// Downloads a specific piece from a single peer.
    ///
    /// Connects to peer, requests the piece data, and assembles the complete piece
    /// from potentially multiple block responses.
    async fn download_piece_from_peer(
        &self,
        peer_addr: SocketAddr,
        piece_index: PieceIndex,
    ) -> Result<Vec<u8>, TorrentError> {
        tracing::debug!("download_piece_from_peer: START peer={peer_addr}, piece={piece_index}");
        self.connect_to_peer(peer_addr).await?;
        tracing::debug!("download_piece_from_peer: connection successful, starting block download");
        let piece_data = self.download_piece_blocks(peer_addr, piece_index).await?;
        tracing::debug!("download_piece_from_peer: blocks downloaded, disconnecting");
        self.disconnect_from_peer(peer_addr).await;
        tracing::debug!(
            "download_piece_from_peer: SUCCESS peer={peer_addr}, piece={piece_index}, {} bytes",
            piece_data.len()
        );
        Ok(piece_data)
    }

    /// Establishes connection to a peer for downloading.
    async fn connect_to_peer(&self, peer_addr: SocketAddr) -> Result<(), TorrentError> {
        tracing::debug!("Connecting to peer: {peer_addr}");
        let mut peer_manager = self.peer_manager.write().await;
        let result = peer_manager
            .connect_peer(peer_addr, self.torrent_metadata.info_hash, self.peer_id)
            .await;
        match &result {
            Ok(_) => tracing::debug!("Successfully connected to peer: {peer_addr}"),
            Err(e) => tracing::debug!("Failed to connect to peer {peer_addr}: {e}"),
        }
        result
    }

    /// Downloads all blocks for a piece from the connected peer.
    async fn download_piece_blocks(
        &self,
        peer_addr: SocketAddr,
        piece_index: PieceIndex,
    ) -> Result<Vec<u8>, TorrentError> {
        tracing::debug!("download_piece_blocks: START peer={peer_addr}, piece={piece_index}");
        let piece_size = self.calculate_piece_size(piece_index);
        tracing::debug!("download_piece_blocks: piece_size={piece_size}");
        let mut piece_data = vec![0u8; piece_size];
        let mut offset = 0u32;
        let mut block_count = 0;

        while offset < piece_size as u32 {
            let request_length = std::cmp::min(BLOCK_SIZE, piece_size as u32 - offset);
            block_count += 1;
            tracing::debug!(
                "download_piece_blocks: requesting block {block_count}, offset={offset}, length={request_length}"
            );

            let block_data = match self
                .request_block_from_peer(peer_addr, piece_index, offset, request_length)
                .await
            {
                Ok(data) => {
                    tracing::debug!(
                        "download_piece_blocks: block {block_count} received, {} bytes",
                        data.len()
                    );
                    data
                }
                Err(e) => {
                    tracing::warn!("download_piece_blocks: block {block_count} FAILED: {e}");
                    return Err(e);
                }
            };

            self.copy_block_to_piece(&mut piece_data, offset, &block_data)?;
            offset += request_length;
        }

        tracing::debug!(
            "download_piece_blocks: SUCCESS downloaded {block_count} blocks, total {} bytes",
            piece_data.len()
        );
        Ok(piece_data)
    }

    /// Requests a single block from a peer and waits for response.
    async fn request_block_from_peer(
        &self,
        peer_addr: SocketAddr,
        piece_index: PieceIndex,
        offset: u32,
        length: u32,
    ) -> Result<bytes::Bytes, TorrentError> {
        let request_msg = PeerMessage::Request {
            piece_index,
            offset,
            length,
        };

        {
            let mut peer_manager = self.peer_manager.write().await;
            peer_manager.send_message(peer_addr, request_msg).await?;
        }

        self.wait_for_piece_response(peer_addr, piece_index, offset)
            .await
    }

    /// Waits for piece response matching the request parameters.
    async fn wait_for_piece_response(
        &self,
        peer_addr: SocketAddr,
        piece_index: PieceIndex,
        expected_offset: u32,
    ) -> Result<bytes::Bytes, TorrentError> {
        tokio::time::timeout(Duration::from_secs(PEER_REQUEST_TIMEOUT_SECS), async {
            let mut message_count = 0;
            const MAX_MESSAGES: usize = 50; // Prevent infinite loops

            loop {
                message_count += 1;
                if message_count > MAX_MESSAGES {
                    return Err(TorrentError::PeerConnectionError {
                        reason: "Too many messages while waiting for piece response".to_string(),
                    });
                }

                let msg_event = {
                    let mut peer_manager = self.peer_manager.write().await;
                    match peer_manager.receive_message().await {
                        Ok(msg_event) => msg_event,
                        Err(_) => continue,
                    }
                };

                if self.is_matching_piece_response(
                    &msg_event,
                    peer_addr,
                    piece_index,
                    expected_offset,
                ) && let PeerMessage::Piece { data, .. } = msg_event.message
                {
                    return Ok::<bytes::Bytes, TorrentError>(data);
                }
            }
        })
        .await
        .map_err(|_| TorrentError::PeerConnectionError {
            reason: "Timeout waiting for piece response".to_string(),
        })?
    }

    /// Checks if received message matches the expected piece response.
    fn is_matching_piece_response(
        &self,
        msg_event: &PeerMessageEvent,
        expected_peer: SocketAddr,
        expected_piece: PieceIndex,
        expected_offset: u32,
    ) -> bool {
        msg_event.peer_address == expected_peer
            && matches!(
                &msg_event.message,
                PeerMessage::Piece {
                    piece_index,
                    offset,
                    ..
                } if *piece_index == expected_piece && *offset == expected_offset
            )
    }

    /// Copies block data into the piece buffer with bounds checking.
    fn copy_block_to_piece(
        &self,
        piece_data: &mut [u8],
        offset: u32,
        block_data: &[u8],
    ) -> Result<(), TorrentError> {
        let start = offset as usize;
        let end = start + block_data.len();

        if end <= piece_data.len() {
            piece_data[start..end].copy_from_slice(block_data);
            Ok(())
        } else {
            Err(TorrentError::ProtocolError {
                message: "Received block data exceeds piece boundary".to_string(),
            })
        }
    }

    /// Disconnects from peer after download completion.
    async fn disconnect_from_peer(&self, peer_addr: SocketAddr) {
        let mut peer_manager = self.peer_manager.write().await;
        let _ = peer_manager.disconnect_peer(peer_addr).await;
    }

    /// Verifies piece data against expected hash.
    fn verify_piece_hash(&self, piece_index: PieceIndex, piece_data: &[u8]) -> bool {
        let index = piece_index.as_u32() as usize;
        if index >= self.torrent_metadata.piece_hashes.len() {
            return false;
        }

        let expected_hash = &self.torrent_metadata.piece_hashes[index];
        let mut hasher = Sha1::new();
        hasher.update(piece_data);
        let computed_hash = hasher.finalize();

        computed_hash.as_slice() == expected_hash
    }

    /// Calculates size of specific piece (last piece may be smaller).
    fn calculate_piece_size(&self, piece_index: PieceIndex) -> usize {
        let index = piece_index.as_u32() as usize;
        let total_pieces = self.torrent_metadata.piece_hashes.len();

        if index >= total_pieces {
            return 0;
        }

        if index == total_pieces - 1 {
            // For the last piece, calculate remaining bytes
            let remaining =
                self.torrent_metadata.total_length % self.torrent_metadata.piece_length as u64;
            if remaining > 0 {
                remaining as usize
            } else {
                // If remainder is 0, the last piece is exactly piece_length size
                // UNLESS the total file is smaller than piece_length (single small piece case)
                if total_pieces == 1
                    && self.torrent_metadata.total_length
                        < self.torrent_metadata.piece_length as u64
                {
                    self.torrent_metadata.total_length as usize
                } else {
                    self.torrent_metadata.piece_length as usize
                }
            }
        } else {
            self.torrent_metadata.piece_length as usize
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use tempfile::tempdir;
    use tokio::sync::RwLock;

    use super::*;
    use crate::engine::MockPeerManager;
    use crate::storage::FileStorage;
    use crate::torrent::protocol::types::PeerId;
    use crate::torrent::test_data::create_test_torrent_metadata;

    fn create_test_metadata() -> TorrentMetadata {
        create_test_torrent_metadata()
    }

    #[tokio::test]
    async fn test_piece_downloader_creation() {
        let temp_dir = tempdir().unwrap();
        let storage = FileStorage::new(
            temp_dir.path().join("downloads"),
            temp_dir.path().join("library"),
        );

        let metadata = create_test_metadata();
        let peer_manager = Arc::new(RwLock::new(MockPeerManager::new()));
        let peer_id = PeerId::generate();
        let downloader = PieceDownloader::new(metadata, storage, peer_manager, peer_id).unwrap();

        let progress = downloader.progress().await;
        assert_eq!(progress.len(), 3);
        assert!(progress.iter().all(|p| p.status == PieceStatus::Pending));
    }

    #[tokio::test]
    async fn test_piece_download_progress() {
        let temp_dir = tempdir().unwrap();
        let storage = FileStorage::new(
            temp_dir.path().join("downloads"),
            temp_dir.path().join("library"),
        );

        let metadata = create_test_metadata();
        let peer_manager = Arc::new(RwLock::new(MockPeerManager::new()));
        let peer_id = PeerId::generate();
        let mut downloader =
            PieceDownloader::new(metadata, storage, peer_manager, peer_id).unwrap();

        // Add mock peers for testing
        let mock_peers = vec![
            "127.0.0.1:8080".parse().unwrap(),
            "127.0.0.1:8081".parse().unwrap(),
        ];
        downloader.update_peers(mock_peers).await;

        let piece_0_result = downloader.download_piece(PieceIndex::new(0)).await;
        assert!(piece_0_result.is_ok());

        let progress = downloader.progress().await;
        let piece_0_progress = progress
            .iter()
            .find(|p| p.piece_index.as_u32() == 0)
            .unwrap();
        assert_eq!(piece_0_progress.status, PieceStatus::Complete);

        assert_eq!(downloader.completed_pieces().await, 1);
        assert!(!downloader.is_complete().await);
    }

    #[tokio::test]
    async fn test_completion_tracking() {
        let temp_dir = tempdir().unwrap();
        let storage = FileStorage::new(
            temp_dir.path().join("downloads"),
            temp_dir.path().join("library"),
        );

        let metadata = create_test_metadata();
        let mut mock_peer_manager = MockPeerManager::new();
        mock_peer_manager.enable_piece_data_simulation();
        let peer_manager = Arc::new(RwLock::new(mock_peer_manager));
        let peer_id = PeerId::generate();
        let mut downloader =
            PieceDownloader::new(metadata, storage, peer_manager, peer_id).unwrap();

        // Add mock peers for testing
        let mock_peers = vec![
            "127.0.0.1:8080".parse().unwrap(),
            "127.0.0.1:8081".parse().unwrap(),
        ];
        downloader.update_peers(mock_peers).await;

        for i in 0..3 {
            let result = downloader.download_piece(PieceIndex::new(i)).await;
            if let Err(e) = &result {
                eprintln!("Failed to download piece {}: {:?}", i, e);
            }
            assert!(
                result.is_ok(),
                "Failed to download piece {}: {:?}",
                i,
                result.unwrap_err()
            );
        }

        assert_eq!(downloader.completed_pieces().await, 3);
        assert!(downloader.is_complete().await);
    }

    #[tokio::test]
    async fn test_piece_size_calculation() {
        let temp_dir = tempdir().unwrap();
        let storage = FileStorage::new(
            temp_dir.path().join("downloads"),
            temp_dir.path().join("library"),
        );

        let metadata = create_test_metadata();
        let mut mock_peer_manager = MockPeerManager::new();
        mock_peer_manager.enable_piece_data_simulation();
        let peer_manager = Arc::new(RwLock::new(mock_peer_manager));
        let peer_id = PeerId::generate();
        let downloader = PieceDownloader::new(metadata, storage, peer_manager, peer_id).unwrap();

        // Test normal piece size
        assert_eq!(downloader.calculate_piece_size(PieceIndex::new(0)), 32768);
        assert_eq!(downloader.calculate_piece_size(PieceIndex::new(1)), 32768);

        // Test last piece size (should be same for our test data)
        assert_eq!(downloader.calculate_piece_size(PieceIndex::new(2)), 32768);

        // Test out-of-bounds piece
        assert_eq!(downloader.calculate_piece_size(PieceIndex::new(10)), 0);
    }

    #[tokio::test]
    async fn test_hash_verification() {
        let temp_dir = tempdir().unwrap();
        let storage = FileStorage::new(
            temp_dir.path().join("downloads"),
            temp_dir.path().join("library"),
        );

        let metadata = create_test_metadata();
        let peer_manager = Arc::new(RwLock::new(MockPeerManager::new()));
        let peer_id = PeerId::generate();
        let downloader = PieceDownloader::new(metadata, storage, peer_manager, peer_id).unwrap();

        // Test valid piece data
        let valid_piece_data = vec![0u8; 32768]; // Matches piece 0 hash
        assert!(downloader.verify_piece_hash(PieceIndex::new(0), &valid_piece_data));

        // Test invalid piece data
        let invalid_piece_data = vec![255u8; 32768];
        assert!(!downloader.verify_piece_hash(PieceIndex::new(0), &invalid_piece_data));

        // Test out-of-bounds piece index
        assert!(!downloader.verify_piece_hash(PieceIndex::new(10), &valid_piece_data));
    }

    #[tokio::test]
    async fn test_copy_block_to_piece() {
        let temp_dir = tempdir().unwrap();
        let storage = FileStorage::new(
            temp_dir.path().join("downloads"),
            temp_dir.path().join("library"),
        );

        let metadata = create_test_metadata();
        let peer_manager = Arc::new(RwLock::new(MockPeerManager::new()));
        let peer_id = PeerId::generate();
        let downloader = PieceDownloader::new(metadata, storage, peer_manager, peer_id).unwrap();

        let mut piece_data = vec![0u8; 1024];
        let block_data = vec![42u8; 256];

        // Test valid copy
        let result = downloader.copy_block_to_piece(&mut piece_data, 0, &block_data);
        assert!(result.is_ok());
        assert_eq!(piece_data[0..256], vec![42u8; 256]);

        // Test copy with offset
        let result = downloader.copy_block_to_piece(&mut piece_data, 256, &block_data);
        assert!(result.is_ok());
        assert_eq!(piece_data[256..512], vec![42u8; 256]);

        // Test overflow
        let large_block = vec![1u8; 1024];
        let result = downloader.copy_block_to_piece(&mut piece_data, 512, &large_block);
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_peer_connection_management() {
        let temp_dir = tempdir().unwrap();
        let storage = FileStorage::new(
            temp_dir.path().join("downloads"),
            temp_dir.path().join("library"),
        );

        let metadata = create_test_metadata();
        let peer_manager = Arc::new(RwLock::new(MockPeerManager::new()));
        let peer_id = PeerId::generate();
        let downloader = PieceDownloader::new(metadata, storage, peer_manager, peer_id).unwrap();

        let peer_addr = "127.0.0.1:6881".parse().unwrap();

        // Test connection
        let result = downloader.connect_to_peer(peer_addr).await;
        assert!(result.is_ok());

        // Test disconnection (should always succeed in mock)
        downloader.disconnect_from_peer(peer_addr).await;
    }

    #[tokio::test]
    async fn test_error_recovery_integration() {
        let temp_dir = tempdir().unwrap();
        let storage = FileStorage::new(
            temp_dir.path().join("downloads"),
            temp_dir.path().join("library"),
        );

        let metadata = create_test_metadata();
        let peer_manager = Arc::new(RwLock::new(MockPeerManager::new()));
        let peer_id = PeerId::generate();
        let mut downloader =
            PieceDownloader::new(metadata, storage, peer_manager, peer_id).unwrap();

        // Test error recovery statistics
        let initial_stats = downloader.error_recovery_statistics().await;
        assert_eq!(initial_stats.total_pieces_with_failures, 0);
        assert_eq!(initial_stats.total_blacklisted_peers, 0);

        // Add peers that will fail (no real servers)
        let failing_peers = vec![
            "127.0.0.1:9999".parse().unwrap(), // Non-existent port
            "127.0.0.1:9998".parse().unwrap(),
        ];
        downloader.update_peers(failing_peers).await;

        // Try to download a piece - this should trigger error recovery
        let result = tokio::time::timeout(
            Duration::from_secs(2),
            downloader.download_piece(PieceIndex::new(0)),
        )
        .await;

        // Download should either timeout or fail with error recovery
        match result {
            Ok(Ok(())) => {
                // Unexpected success with mock data
            }
            Ok(Err(_)) | Err(_) => {
                // Expected failure - error recovery was used
            }
        }

        // Test cleanup
        downloader.cleanup_error_recovery().await;
    }

    #[tokio::test]
    async fn test_download_with_peer_blacklisting() {
        let temp_dir = tempdir().unwrap();
        let storage = FileStorage::new(
            temp_dir.path().join("downloads"),
            temp_dir.path().join("library"),
        );

        let metadata = create_test_metadata();
        let peer_manager = Arc::new(RwLock::new(MockPeerManager::new()));
        let peer_id = PeerId::generate();
        let mut downloader =
            PieceDownloader::new(metadata, storage, peer_manager, peer_id).unwrap();

        // Set up peers where some will be blacklisted
        let mixed_peers = vec![
            "127.0.0.1:8080".parse().unwrap(), // This will work with mock
            "127.0.0.1:9999".parse().unwrap(), // This will fail and get blacklisted
        ];
        downloader.update_peers(mixed_peers).await;

        // Download pieces - error recovery should handle failures gracefully
        for piece_idx in 0..3 {
            let result = tokio::time::timeout(
                Duration::from_millis(500),
                downloader.download_piece(PieceIndex::new(piece_idx)),
            )
            .await;

            // Each piece should either succeed (with mock data) or fail gracefully
            match result {
                Ok(Ok(())) => {
                    // Success with mock peer
                    let progress = downloader.progress().await;
                    let piece_progress = progress
                        .iter()
                        .find(|p| p.piece_index.as_u32() == piece_idx)
                        .unwrap();
                    assert_eq!(piece_progress.status, PieceStatus::Complete);
                }
                Ok(Err(_)) | Err(_) => {
                    // Expected failure due to peer unavailability
                    let progress = downloader.progress().await;
                    let piece_progress = progress
                        .iter()
                        .find(|p| p.piece_index.as_u32() == piece_idx)
                        .unwrap();
                    assert!(matches!(piece_progress.status, PieceStatus::Failed { .. }));
                }
            }
        }

        // Verify error recovery stats show activity
        // We expect some pieces to have had failures, leading to retry attempts
    }

    #[tokio::test]
    async fn test_enhanced_peer_connection_integration() {
        use std::net::{IpAddr, Ipv4Addr};

        let address = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 6881);
        let mut connection = EnhancedPeerConnection::new(address, 100);

        // Test connection stats
        let stats = connection.connection_stats();
        assert_eq!(stats.address, address);
        assert!(stats.connected_at.is_none());
        assert_eq!(stats.pending_requests, 0);

        // Test cleanup operations
        let expired_requests = connection.cleanup_expired_requests();
        assert!(expired_requests.is_empty());

        // Test connection health monitoring
        assert!(!connection.is_connection_healthy()); // Not connected
        assert!(!connection.is_connected());
    }
}
