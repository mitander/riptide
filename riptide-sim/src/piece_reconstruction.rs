//! Piece reconstruction pipeline for streaming simulation
//!
//! Reassembles downloaded pieces back into streamable content for UI integration.
//! Enables end-to-end content distribution simulation from file splitting to reconstruction.

use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

use riptide_core::torrent::{InfoHash, PieceIndex, TorrentError, TorrentMetadata};
use sha1::{Digest, Sha1};
use tokio::sync::RwLock;

/// Represents a piece that has been downloaded and verified
#[derive(Debug, Clone)]
pub struct VerifiedPiece {
    pub piece_index: PieceIndex,
    pub data: Vec<u8>,
    pub verified_at: std::time::Instant,
}

/// Status of piece reconstruction for a torrent
#[derive(Debug, Clone)]
pub struct ReconstructionProgress {
    pub info_hash: InfoHash,
    pub pieces_downloaded: u32,
    pub pieces_total: u32,
    pub bytes_reconstructed: u64,
    pub bytes_total: u64,
    pub completion_percentage: f32,
    pub is_streamable: bool, // True if enough sequential pieces available for streaming
}

/// Piece reconstruction service for simulation environments
///
/// Takes downloaded pieces and reassembles them into streamable content.
/// Maintains piece order and verifies hashes for data integrity.
pub struct PieceReconstructionService {
    /// Map from info_hash to torrent metadata
    torrent_metadata: Arc<RwLock<HashMap<InfoHash, TorrentMetadata>>>,
    /// Map from info_hash to downloaded pieces (ordered by piece index)
    downloaded_pieces: Arc<RwLock<HashMap<InfoHash, BTreeMap<u32, VerifiedPiece>>>>,
    /// Map from info_hash to reconstructed file segments
    reconstructed_segments: Arc<RwLock<HashMap<InfoHash, Vec<u8>>>>,
}

impl PieceReconstructionService {
    /// Creates new piece reconstruction service
    pub fn new() -> Self {
        Self {
            torrent_metadata: Arc::new(RwLock::new(HashMap::new())),
            downloaded_pieces: Arc::new(RwLock::new(HashMap::new())),
            reconstructed_segments: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Registers torrent metadata for reconstruction
    ///
    /// Must be called before adding pieces to enable proper reconstruction
    /// and streaming status calculation.
    ///
    /// # Errors
    /// - `TorrentError::InvalidTorrentFile` - Invalid metadata
    pub async fn register_torrent(&self, metadata: TorrentMetadata) -> Result<(), TorrentError> {
        if metadata.piece_hashes.is_empty() {
            return Err(TorrentError::InvalidTorrentFile {
                reason: "Torrent metadata has no pieces".to_string(),
            });
        }

        let mut torrents = self.torrent_metadata.write().await;
        let mut pieces = self.downloaded_pieces.write().await;
        let mut segments = self.reconstructed_segments.write().await;

        let info_hash = metadata.info_hash;
        torrents.insert(info_hash, metadata);
        pieces.insert(info_hash, BTreeMap::new());
        segments.insert(info_hash, Vec::new());

        Ok(())
    }

    /// Adds downloaded piece and attempts reconstruction
    ///
    /// Verifies piece hash against metadata and adds to reconstruction queue.
    /// Automatically reconstructs streamable segments when sequential pieces available.
    ///
    /// # Errors
    /// - `TorrentError::TorrentNotFound` - Torrent not registered
    /// - `TorrentError::PieceHashMismatch` - Hash verification failed
    pub async fn add_piece(
        &self,
        info_hash: InfoHash,
        piece_index: PieceIndex,
        piece_data: Vec<u8>,
    ) -> Result<ReconstructionProgress, TorrentError> {
        // Verify we have metadata for this torrent
        let expected_hash = {
            let torrents = self.torrent_metadata.read().await;
            let metadata = torrents
                .get(&info_hash)
                .ok_or_else(|| TorrentError::TorrentNotFound { info_hash })?;

            let piece_idx = piece_index.as_u32() as usize;
            if piece_idx >= metadata.piece_hashes.len() {
                return Err(TorrentError::PieceHashMismatch { index: piece_index });
            }

            metadata.piece_hashes[piece_idx]
        };

        // Verify piece hash
        let mut hasher = Sha1::new();
        hasher.update(&piece_data);
        let actual_hash = hasher.finalize();

        if actual_hash.as_slice() != &expected_hash[..] {
            return Err(TorrentError::PieceHashMismatch { index: piece_index });
        }

        // Add verified piece
        let verified_piece = VerifiedPiece {
            piece_index,
            data: piece_data,
            verified_at: std::time::Instant::now(),
        };

        {
            let mut pieces = self.downloaded_pieces.write().await;
            let torrent_pieces = pieces
                .get_mut(&info_hash)
                .ok_or_else(|| TorrentError::TorrentNotFound { info_hash })?;
            torrent_pieces.insert(piece_index.as_u32(), verified_piece);
        }

        // Attempt reconstruction if we have sequential pieces
        self.reconstruct_sequential_data(info_hash).await?;

        // Calculate and return progress
        self.reconstruction_progress(info_hash).await
    }

    /// Returns current reconstruction progress for a torrent
    ///
    /// # Errors
    /// - `TorrentError::TorrentNotFound` - Torrent not registered
    pub async fn reconstruction_progress(
        &self,
        info_hash: InfoHash,
    ) -> Result<ReconstructionProgress, TorrentError> {
        let torrents = self.torrent_metadata.read().await;
        let pieces = self.downloaded_pieces.read().await;
        let segments = self.reconstructed_segments.read().await;

        let metadata = torrents
            .get(&info_hash)
            .ok_or_else(|| TorrentError::TorrentNotFound { info_hash })?;
        let torrent_pieces = pieces
            .get(&info_hash)
            .ok_or_else(|| TorrentError::TorrentNotFound { info_hash })?;
        let reconstructed = segments
            .get(&info_hash)
            .ok_or_else(|| TorrentError::TorrentNotFound { info_hash })?;

        let pieces_total = metadata.piece_hashes.len() as u32;
        let pieces_downloaded = torrent_pieces.len() as u32;
        let bytes_total = metadata.total_length;
        let bytes_reconstructed = reconstructed.len() as u64;

        let completion_percentage = if pieces_total > 0 {
            (pieces_downloaded as f32 / pieces_total as f32) * 100.0
        } else {
            0.0
        };

        // Check if enough sequential pieces available for streaming
        let is_streamable = self.has_sequential_pieces_for_streaming(torrent_pieces);

        Ok(ReconstructionProgress {
            info_hash,
            pieces_downloaded,
            pieces_total,
            bytes_reconstructed,
            bytes_total,
            completion_percentage,
            is_streamable,
        })
    }

    /// Reads reconstructed segment for streaming
    ///
    /// Returns requested byte range from reconstructed content for HTTP streaming.
    /// Supports range requests for video playback.
    ///
    /// # Errors
    /// - `TorrentError::TorrentNotFound` - Torrent not registered
    /// - `TorrentError::InvalidTorrentFile` - Invalid range request
    pub async fn read_reconstructed_segment(
        &self,
        info_hash: InfoHash,
        start_byte: u64,
        length: u64,
    ) -> Result<Vec<u8>, TorrentError> {
        let segments = self.reconstructed_segments.read().await;
        let reconstructed = segments
            .get(&info_hash)
            .ok_or_else(|| TorrentError::TorrentNotFound { info_hash })?;

        let start = start_byte as usize;
        let end = (start_byte + length) as usize;

        if start >= reconstructed.len() {
            return Err(TorrentError::InvalidTorrentFile {
                reason: format!(
                    "Start byte {} beyond reconstructed data length {}",
                    start,
                    reconstructed.len()
                ),
            });
        }

        let safe_end = end.min(reconstructed.len());
        Ok(reconstructed[start..safe_end].to_vec())
    }

    /// Returns size of reconstructed content available for streaming
    pub async fn reconstructed_size(&self, info_hash: InfoHash) -> Result<u64, TorrentError> {
        let segments = self.reconstructed_segments.read().await;
        let reconstructed = segments
            .get(&info_hash)
            .ok_or_else(|| TorrentError::TorrentNotFound { info_hash })?;
        Ok(reconstructed.len() as u64)
    }

    /// Removes torrent from reconstruction service
    pub async fn remove_torrent(&self, info_hash: InfoHash) {
        let mut torrents = self.torrent_metadata.write().await;
        let mut pieces = self.downloaded_pieces.write().await;
        let mut segments = self.reconstructed_segments.write().await;

        torrents.remove(&info_hash);
        pieces.remove(&info_hash);
        segments.remove(&info_hash);
    }

    /// Reconstructs sequential data from downloaded pieces
    async fn reconstruct_sequential_data(&self, info_hash: InfoHash) -> Result<(), TorrentError> {
        let pieces = self.downloaded_pieces.read().await;
        let torrent_pieces = pieces
            .get(&info_hash)
            .ok_or_else(|| TorrentError::TorrentNotFound { info_hash })?;

        // Find longest sequential range starting from piece 0
        let mut sequential_data = Vec::new();
        let mut current_piece_index = 0u32;

        while let Some(piece) = torrent_pieces.get(&current_piece_index) {
            sequential_data.extend_from_slice(&piece.data);
            current_piece_index += 1;
        }

        // Update reconstructed segments if we have new data
        if !sequential_data.is_empty() {
            let mut segments = self.reconstructed_segments.write().await;
            if let Some(current_segments) = segments.get_mut(&info_hash) {
                if sequential_data.len() > current_segments.len() {
                    *current_segments = sequential_data;
                }
            }
        }

        Ok(())
    }

    /// Checks if enough sequential pieces available for streaming
    fn has_sequential_pieces_for_streaming(&self, pieces: &BTreeMap<u32, VerifiedPiece>) -> bool {
        if pieces.is_empty() {
            return false;
        }

        // Check if we have pieces starting from 0
        if !pieces.contains_key(&0) {
            return false;
        }

        // Count sequential pieces from beginning
        let mut sequential_count = 0;
        for i in 0.. {
            if pieces.contains_key(&i) {
                sequential_count += 1;
            } else {
                break;
            }
        }

        // Consider streamable if we have at least 10 sequential pieces
        // This provides enough buffer for streaming to start
        sequential_count >= 10
    }
}

impl Default for PieceReconstructionService {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use riptide_core::torrent::{TorrentFile, TorrentMetadata};

    use super::*;

    fn create_test_metadata() -> TorrentMetadata {
        TorrentMetadata {
            info_hash: InfoHash::new([1u8; 20]),
            name: "test.txt".to_string(),
            piece_length: 32,
            piece_hashes: vec![[0u8; 20], [1u8; 20], [2u8; 20]], // 3 pieces
            total_length: 96,
            files: vec![TorrentFile {
                path: vec!["test.txt".to_string()],
                length: 96,
            }],
            announce_urls: vec!["http://tracker.example.com/announce".to_string()],
        }
    }

    #[tokio::test]
    async fn test_reconstruction_service_register_torrent() {
        let service = PieceReconstructionService::new();
        let metadata = create_test_metadata();
        let info_hash = metadata.info_hash;

        let result = service.register_torrent(metadata).await;
        assert!(result.is_ok());

        // Should be able to get progress for registered torrent
        let progress = service.reconstruction_progress(info_hash).await.unwrap();
        assert_eq!(progress.pieces_total, 3);
        assert_eq!(progress.pieces_downloaded, 0);
        assert!(!progress.is_streamable);
    }

    #[tokio::test]
    async fn test_reconstruction_service_add_piece() {
        let service = PieceReconstructionService::new();
        let metadata = create_test_metadata();
        let info_hash = metadata.info_hash;

        service.register_torrent(metadata).await.unwrap();

        // Add piece with correct hash
        let piece_data = vec![0u8; 32]; // All zeros should hash to [0u8; 20] for our test
        let mut hasher = Sha1::new();
        hasher.update(&piece_data);
        let actual_hash = hasher.finalize();

        // For this test, we'll update the metadata to match our piece hash
        {
            let mut torrents = service.torrent_metadata.write().await;
            let metadata = torrents.get_mut(&info_hash).unwrap();
            let mut hash_array = [0u8; 20];
            hash_array.copy_from_slice(&actual_hash[..20]);
            metadata.piece_hashes[0] = hash_array;
        }

        let progress = service
            .add_piece(info_hash, PieceIndex::new(0), piece_data)
            .await
            .unwrap();
        assert_eq!(progress.pieces_downloaded, 1);
        assert!(!progress.is_streamable); // Need more pieces for streaming
    }

    #[tokio::test]
    async fn test_reconstruction_service_sequential_streaming() {
        let service = PieceReconstructionService::new();
        let mut metadata = create_test_metadata();

        // Create enough pieces for streaming test (15 pieces)
        metadata.piece_hashes = (0..15).map(|i| [i as u8; 20]).collect();
        let info_hash = metadata.info_hash;

        service.register_torrent(metadata).await.unwrap();

        // Add first 12 sequential pieces
        for i in 0..12 {
            let piece_data = vec![i as u8; 32];
            let mut hasher = Sha1::new();
            hasher.update(&piece_data);
            let hash = hasher.finalize();

            // Update metadata hash to match our test data
            {
                let mut torrents = service.torrent_metadata.write().await;
                let metadata = torrents.get_mut(&info_hash).unwrap();
                let mut hash_array = [0u8; 20];
                hash_array.copy_from_slice(&hash[..20]);
                metadata.piece_hashes[i] = hash_array;
            }

            let progress = service
                .add_piece(info_hash, PieceIndex::new(i as u32), piece_data)
                .await
                .unwrap();

            if i >= 9 {
                // Should be streamable after 10 pieces
                assert!(progress.is_streamable);
            }
        }

        // Should have reconstructed data available
        let size = service.reconstructed_size(info_hash).await.unwrap();
        assert_eq!(size, 12 * 32); // 12 pieces * 32 bytes each

        // Should be able to read segments
        let segment = service
            .read_reconstructed_segment(info_hash, 0, 64)
            .await
            .unwrap();
        assert_eq!(segment.len(), 64);
    }

    #[tokio::test]
    async fn test_reconstruction_service_hash_mismatch() {
        let service = PieceReconstructionService::new();
        let metadata = create_test_metadata();
        let info_hash = metadata.info_hash;

        service.register_torrent(metadata).await.unwrap();

        // Try to add piece with incorrect hash
        let piece_data = vec![99u8; 32]; // This won't match the expected [0u8; 20] hash

        let result = service
            .add_piece(info_hash, PieceIndex::new(0), piece_data)
            .await;
        assert!(matches!(
            result,
            Err(TorrentError::PieceHashMismatch { .. })
        ));
    }

    #[tokio::test]
    async fn test_reconstruction_service_unknown_torrent() {
        let service = PieceReconstructionService::new();
        let unknown_hash = InfoHash::new([99u8; 20]);

        let result = service
            .add_piece(unknown_hash, PieceIndex::new(0), vec![0u8; 32])
            .await;
        assert!(matches!(result, Err(TorrentError::TorrentNotFound { .. })));
    }

    #[tokio::test]
    async fn test_reconstruction_service_remove_torrent() {
        let service = PieceReconstructionService::new();
        let metadata = create_test_metadata();
        let info_hash = metadata.info_hash;

        service.register_torrent(metadata).await.unwrap();

        // Should be able to get progress
        let progress = service.reconstruction_progress(info_hash).await;
        assert!(progress.is_ok());

        service.remove_torrent(info_hash).await;

        // Should no longer find torrent
        let progress = service.reconstruction_progress(info_hash).await;
        assert!(matches!(
            progress,
            Err(TorrentError::TorrentNotFound { .. })
        ));
    }
}
