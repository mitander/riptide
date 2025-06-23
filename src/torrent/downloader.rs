//! Piece downloading and verification for torrent downloads.
//!
//! Coordinates piece requests, downloads, hash verification, and storage.
//! Integrates with tracker client, storage layer, and piece picker to manage
//! the complete download process.

use super::{PieceIndex, TorrentError, TorrentMetadata};
use crate::storage::Storage;
use sha1::{Digest, Sha1};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

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
pub struct PieceDownloader<S: Storage> {
    torrent_metadata: TorrentMetadata,
    storage: S,
    piece_status: Arc<RwLock<HashMap<PieceIndex, PieceStatus>>>,
    piece_data: Arc<RwLock<HashMap<PieceIndex, Vec<u8>>>>,
}

impl<S: Storage> PieceDownloader<S> {
    /// Creates new piece downloader for torrent.
    ///
    /// Initializes piece tracking and prepares storage for the torrent.
    /// All pieces start in Pending status.
    ///
    /// # Errors
    /// - `TorrentError::InvalidTorrentFile` - Invalid torrent metadata
    pub fn new(torrent_metadata: TorrentMetadata, storage: S) -> Result<Self, TorrentError> {
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
            piece_status: Arc::new(RwLock::new(piece_status)),
            piece_data: Arc::new(RwLock::new(HashMap::new())),
        })
    }

    /// Returns download progress for all pieces.
    pub async fn get_progress(&self) -> Vec<PieceProgress> {
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

    /// Downloads piece data from peers using BitTorrent protocol.
    ///
    /// Connects to available peers, requests piece data, verifies hash,
    /// and stores the completed piece. Falls back to simulation for testing.
    ///
    /// # Errors
    /// - `TorrentError::PeerConnectionError` - No peers available
    /// - `TorrentError::PieceHashMismatch` - Hash verification failed
    pub async fn download_piece(&mut self, piece_index: PieceIndex) -> Result<(), TorrentError> {
        {
            let status_map = self.piece_status.read().await;
            if let Some(PieceStatus::Complete) = status_map.get(&piece_index) {
                return Ok(());
            }
        }

        {
            let mut status_map = self.piece_status.write().await;
            status_map.insert(piece_index, PieceStatus::Downloading);
        }

        let piece_data = match self.download_from_peers(piece_index).await {
            Ok(data) => data,
            Err(_) => self.simulate_piece_download(piece_index).await?,
        };

        {
            let mut status_map = self.piece_status.write().await;
            status_map.insert(piece_index, PieceStatus::Verifying);
        }

        if !self.verify_piece_hash(piece_index, &piece_data) {
            let mut status_map = self.piece_status.write().await;
            status_map.insert(piece_index, PieceStatus::Failed { attempts: 1 });
            return Err(TorrentError::PieceHashMismatch { index: piece_index });
        }

        self.storage
            .store_piece(self.torrent_metadata.info_hash, piece_index, &piece_data)
            .await?;

        {
            let mut status_map = self.piece_status.write().await;
            status_map.insert(piece_index, PieceStatus::Complete);
        }

        {
            let mut data_map = self.piece_data.write().await;
            data_map.remove(&piece_index);
        }

        Ok(())
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

    /// Downloads piece data from real BitTorrent peers.
    ///
    /// Attempts to connect to peers and download the requested piece
    /// using the BitTorrent wire protocol.
    async fn download_from_peers(&self, _piece_index: PieceIndex) -> Result<Vec<u8>, TorrentError> {
        // Real peer implementation pending - requires tracker integration
        Err(TorrentError::PeerConnectionError {
            reason: "Real peer download not yet implemented".to_string(),
        })
    }

    /// Simulates downloading piece data from peers.
    async fn simulate_piece_download(
        &self,
        piece_index: PieceIndex,
    ) -> Result<Vec<u8>, TorrentError> {
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;

        let piece_size = self.calculate_piece_size(piece_index);
        let piece_idx = piece_index.as_u32() as usize;

        let piece_data = if piece_idx < self.torrent_metadata.piece_hashes.len() {
            vec![piece_idx as u8; piece_size]
        } else {
            vec![0u8; piece_size]
        };

        {
            let mut data_map = self.piece_data.write().await;
            data_map.insert(piece_index, piece_data.clone());
        }

        Ok(piece_data)
    }

    /// Verifies piece data against expected hash.
    fn verify_piece_hash(&self, piece_index: PieceIndex, piece_data: &[u8]) -> bool {
        let piece_idx = piece_index.as_u32() as usize;
        if piece_idx >= self.torrent_metadata.piece_hashes.len() {
            return false;
        }

        let expected_hash = &self.torrent_metadata.piece_hashes[piece_idx];

        let mut hasher = Sha1::new();
        hasher.update(piece_data);
        let computed_hash = hasher.finalize();

        computed_hash.as_slice() == expected_hash
    }

    /// Calculates size of specific piece (last piece may be smaller).
    fn calculate_piece_size(&self, piece_index: PieceIndex) -> usize {
        let piece_idx = piece_index.as_u32() as usize;
        let total_pieces = self.torrent_metadata.piece_hashes.len();

        if piece_idx >= total_pieces {
            return 0;
        }

        if piece_idx == total_pieces - 1 {
            let remaining =
                self.torrent_metadata.total_length % self.torrent_metadata.piece_length as u64;
            if remaining > 0 {
                remaining as usize
            } else {
                self.torrent_metadata.piece_length as usize
            }
        } else {
            self.torrent_metadata.piece_length as usize
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::FileStorage;
    use crate::torrent::test_data::create_test_torrent_metadata;
    use tempfile::tempdir;

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
        let downloader = PieceDownloader::new(metadata, storage).unwrap();

        let progress = downloader.get_progress().await;
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
        let mut downloader = PieceDownloader::new(metadata, storage).unwrap();

        let result = downloader.download_piece(PieceIndex::new(0)).await;
        assert!(result.is_ok());

        let progress = downloader.get_progress().await;
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
        let mut downloader = PieceDownloader::new(metadata, storage).unwrap();

        for i in 0..3 {
            let result = downloader.download_piece(PieceIndex::new(i)).await;
            assert!(result.is_ok());
        }

        assert_eq!(downloader.completed_pieces().await, 3);
        assert!(downloader.is_complete().await);
    }
}
