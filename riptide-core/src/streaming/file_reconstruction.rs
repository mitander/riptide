//! File reconstruction from BitTorrent pieces for remuxing operations

use std::io::Write;
use std::path::Path;
use std::sync::Arc;

use super::strategy::{StreamingError, StreamingResult};
use crate::torrent::{InfoHash, PieceIndex, PieceStore};

/// Reconstructs complete files from BitTorrent pieces for remuxing
pub struct FileReconstructor<P: PieceStore> {
    piece_store: Arc<P>,
}

impl<P: PieceStore> FileReconstructor<P> {
    /// Create new file reconstructor with piece store
    pub fn new(piece_store: Arc<P>) -> Self {
        Self { piece_store }
    }

    /// Reconstruct complete file from pieces and write to output path
    ///
    /// Downloads all pieces sequentially and writes them to the output file.
    /// Verifies all pieces are available before starting reconstruction.
    ///
    /// # Errors
    /// - `StreamingError::MissingPieces` - Not all pieces are available
    /// - `StreamingError::IoError` - Failed to write output file
    /// - `StreamingError::PieceStorageError` - Failed to retrieve piece data
    pub async fn reconstruct_file(
        &self,
        info_hash: InfoHash,
        output_path: &Path,
    ) -> StreamingResult<u64> {
        // Get total piece count
        let piece_count = self.piece_store.piece_count(info_hash).map_err(|e| {
            StreamingError::PieceStorageError {
                reason: e.to_string(),
            }
        })?;

        // Verify all pieces are available
        for piece_index in 0..piece_count {
            let piece_idx = PieceIndex::new(piece_index);
            if !self.piece_store.has_piece(info_hash, piece_idx) {
                return Err(StreamingError::MissingPieces {
                    missing: vec![piece_index],
                    total: piece_count,
                });
            }
        }

        // Create output file
        let mut output_file =
            std::fs::File::create(output_path).map_err(|e| StreamingError::IoError {
                operation: "create output file".to_string(),
                path: output_path.to_string_lossy().to_string(),
                source: e,
            })?;

        let mut total_bytes_written = 0u64;

        // Reconstruct file piece by piece
        for piece_index in 0..piece_count {
            let piece_idx = PieceIndex::new(piece_index);

            tracing::debug!(
                "Reconstructing piece {}/{} for torrent {}",
                piece_index + 1,
                piece_count,
                info_hash
            );

            // Get piece data
            let piece_data = self
                .piece_store
                .piece_data(info_hash, piece_idx)
                .await
                .map_err(|e| StreamingError::PieceStorageError {
                    reason: e.to_string(),
                })?;

            // Write piece data to output file
            output_file
                .write_all(&piece_data)
                .map_err(|e| StreamingError::IoError {
                    operation: "write piece data".to_string(),
                    path: output_path.to_string_lossy().to_string(),
                    source: e,
                })?;

            total_bytes_written += piece_data.len() as u64;
        }

        // Ensure all data is written to disk
        output_file
            .sync_all()
            .map_err(|e| StreamingError::IoError {
                operation: "sync output file".to_string(),
                path: output_path.to_string_lossy().to_string(),
                source: e,
            })?;

        tracing::debug!(
            "Successfully reconstructed {} bytes for torrent {} to {}",
            total_bytes_written,
            info_hash,
            output_path.display()
        );

        Ok(total_bytes_written)
    }

    /// Check if all pieces are available for reconstruction
    pub fn can_reconstruct(&self, info_hash: InfoHash) -> StreamingResult<bool> {
        let piece_count = self.piece_store.piece_count(info_hash).map_err(|e| {
            StreamingError::PieceStorageError {
                reason: e.to_string(),
            }
        })?;

        for piece_index in 0..piece_count {
            let piece_idx = PieceIndex::new(piece_index);
            if !self.piece_store.has_piece(info_hash, piece_idx) {
                return Ok(false);
            }
        }

        Ok(true)
    }

    /// Get list of missing pieces
    pub fn missing_pieces(&self, info_hash: InfoHash) -> StreamingResult<Vec<u32>> {
        let piece_count = self.piece_store.piece_count(info_hash).map_err(|e| {
            StreamingError::PieceStorageError {
                reason: e.to_string(),
            }
        })?;

        let mut missing = Vec::new();
        for piece_index in 0..piece_count {
            let piece_idx = PieceIndex::new(piece_index);
            if !self.piece_store.has_piece(info_hash, piece_idx) {
                missing.push(piece_index);
            }
        }

        Ok(missing)
    }

    /// Estimate total file size from all pieces
    ///
    /// This provides an approximation by assuming all pieces are the same size
    /// as the first piece. The last piece might be smaller.
    pub async fn estimate_file_size(&self, info_hash: InfoHash) -> StreamingResult<u64> {
        let piece_count = self.piece_store.piece_count(info_hash).map_err(|e| {
            StreamingError::PieceStorageError {
                reason: e.to_string(),
            }
        })?;

        if piece_count == 0 {
            return Ok(0);
        }

        // Get size of first piece to estimate
        let first_piece = self
            .piece_store
            .piece_data(info_hash, PieceIndex::new(0))
            .await
            .map_err(|e| StreamingError::PieceStorageError {
                reason: e.to_string(),
            })?;

        let piece_size = first_piece.len() as u64;

        // Rough estimate: all pieces same size except possibly last
        Ok(piece_size * piece_count as u64)
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use tempfile::tempdir;

    use super::*;
    use crate::torrent::{InfoHash, PieceIndex, TorrentError, TorrentPiece};

    /// Mock piece store for testing
    struct MockPieceStore {
        pieces: HashMap<(InfoHash, u32), Vec<u8>>,
        piece_counts: HashMap<InfoHash, u32>,
    }

    impl MockPieceStore {
        fn new() -> Self {
            Self {
                pieces: HashMap::new(),
                piece_counts: HashMap::new(),
            }
        }

        fn add_pieces(&mut self, info_hash: InfoHash, pieces: Vec<TorrentPiece>) {
            let count = pieces.len() as u32;
            self.piece_counts.insert(info_hash, count);

            for piece in pieces {
                self.pieces.insert((info_hash, piece.index), piece.data);
            }
        }
    }

    #[async_trait::async_trait]
    impl PieceStore for MockPieceStore {
        async fn piece_data(
            &self,
            info_hash: InfoHash,
            piece_index: PieceIndex,
        ) -> Result<Vec<u8>, TorrentError> {
            self.pieces
                .get(&(info_hash, piece_index.as_u32()))
                .cloned()
                .ok_or(TorrentError::PieceHashMismatch { index: piece_index })
        }

        fn has_piece(&self, info_hash: InfoHash, piece_index: PieceIndex) -> bool {
            self.pieces.contains_key(&(info_hash, piece_index.as_u32()))
        }

        fn piece_count(&self, info_hash: InfoHash) -> Result<u32, TorrentError> {
            self.piece_counts
                .get(&info_hash)
                .copied()
                .ok_or(TorrentError::TorrentNotFound { info_hash })
        }
    }

    #[tokio::test]
    async fn test_file_reconstruction_success() {
        let info_hash = InfoHash::new([1u8; 20]);
        let temp_dir = tempdir().unwrap();
        let output_path = temp_dir.path().join("reconstructed.mkv");

        // Create test pieces
        let pieces = vec![
            TorrentPiece {
                index: 0,
                hash: [0u8; 20],
                data: b"piece0data".to_vec(),
            },
            TorrentPiece {
                index: 1,
                hash: [0u8; 20],
                data: b"piece1data".to_vec(),
            },
            TorrentPiece {
                index: 2,
                hash: [0u8; 20],
                data: b"piece2data".to_vec(),
            },
        ];

        let mut piece_store = MockPieceStore::new();
        piece_store.add_pieces(info_hash, pieces);

        let reconstructor = FileReconstructor::new(Arc::new(piece_store));

        // Test reconstruction
        let bytes_written = reconstructor
            .reconstruct_file(info_hash, &output_path)
            .await
            .unwrap();

        assert_eq!(bytes_written, 30); // 3 pieces * 10 bytes each

        // Verify output file content
        let reconstructed_data = std::fs::read(&output_path).unwrap();
        assert_eq!(reconstructed_data, b"piece0datapiece1datapiece2data");
    }

    #[tokio::test]
    async fn test_file_reconstruction_missing_pieces() {
        let info_hash = InfoHash::new([2u8; 20]);
        let temp_dir = tempdir().unwrap();
        let output_path = temp_dir.path().join("reconstructed.mkv");

        // Create incomplete pieces (missing piece 1)
        let pieces = vec![
            TorrentPiece {
                index: 0,
                hash: [0u8; 20],
                data: b"piece0data".to_vec(),
            },
            TorrentPiece {
                index: 2,
                hash: [0u8; 20],
                data: b"piece2data".to_vec(),
            },
        ];

        let mut piece_store = MockPieceStore::new();
        piece_store.add_pieces(info_hash, pieces);
        // Set piece count to 3 even though we only have 2 pieces
        piece_store.piece_counts.insert(info_hash, 3);

        let reconstructor = FileReconstructor::new(Arc::new(piece_store));

        // Test reconstruction should fail
        let result = reconstructor
            .reconstruct_file(info_hash, &output_path)
            .await;
        assert!(matches!(result, Err(StreamingError::MissingPieces { .. })));

        // Test can_reconstruct
        assert!(!reconstructor.can_reconstruct(info_hash).unwrap());

        // Test missing_pieces
        let missing = reconstructor.missing_pieces(info_hash).unwrap();
        assert_eq!(missing, vec![1]);
    }

    #[tokio::test]
    async fn test_file_size_estimation() {
        let info_hash = InfoHash::new([3u8; 20]);

        let pieces = vec![
            TorrentPiece {
                index: 0,
                hash: [0u8; 20],
                data: vec![0u8; 1024], // 1KB piece
            },
            TorrentPiece {
                index: 1,
                hash: [0u8; 20],
                data: vec![0u8; 1024], // 1KB piece
            },
        ];

        let mut piece_store = MockPieceStore::new();
        piece_store.add_pieces(info_hash, pieces);

        let reconstructor = FileReconstructor::new(Arc::new(piece_store));

        let estimated_size = reconstructor.estimate_file_size(info_hash).await.unwrap();
        assert_eq!(estimated_size, 2048); // 2 pieces * 1024 bytes
    }
}
