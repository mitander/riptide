//! BitTorrent piece-based file reconstruction for streaming
//!
//! Provides efficient reading and reconstruction of file data from BitTorrent pieces,
//! enabling direct streaming without requiring complete file assembly.

use std::ops::Range;
use std::sync::Arc;

use crate::torrent::{InfoHash, PieceIndex, PieceStore, TorrentError};

/// Error types for piece-based streaming operations
#[derive(Debug, thiserror::Error)]
pub enum PieceReaderError {
    /// Torrent operation failed during piece reading.
    #[error("Torrent error: {0}")]
    Torrent(#[from] TorrentError),

    /// Range start position is greater than or equal to end position.
    #[error("Invalid range: start {start} >= end {end}")]
    InvalidRange {
        /// Starting byte position of the invalid range.
        start: u64,
        /// Ending byte position of the invalid range.
        end: u64,
    },

    /// Requested range extends beyond the actual file size.
    #[error("Range {start}-{end} exceeds file size {file_size}")]
    RangeExceedsFile {
        /// Starting byte position of the range.
        start: u64,
        /// Ending byte position of the range.
        end: u64,
        /// Actual size of the file in bytes.
        file_size: u64,
    },

    /// Piece data is smaller than required for the requested operation.
    #[error("Piece {piece_index} is smaller than expected: got {actual}, need {minimum}")]
    PieceTooSmall {
        /// Index of the piece that is too small.
        piece_index: u32,
        /// Actual size of the piece in bytes.
        actual: usize,
        /// Minimum required size in bytes.
        minimum: usize,
    },
}

/// Efficiently reads file data from BitTorrent pieces
pub struct PieceBasedStreamReader<P: PieceStore + ?Sized> {
    piece_store: Arc<P>,
    piece_size: u32,
}

impl<P: PieceStore + ?Sized> PieceBasedStreamReader<P> {
    /// Create new piece reader with the given piece store and piece size
    ///
    /// Since piece size is typically known from torrent metadata, it's provided
    /// at construction time for efficiency.
    pub fn new(piece_store: Arc<P>, piece_size: u32) -> Self {
        Self {
            piece_store,
            piece_size,
        }
    }

    /// Read a range of bytes from the reconstructed file
    ///
    /// Reads the specified byte range by fetching and assembling the necessary
    /// BitTorrent pieces. Handles partial piece reads and range boundaries efficiently.
    ///
    /// # Errors
    /// - `PieceReaderError::InvalidRange` - Start >= end or invalid range
    /// - `PieceReaderError::RangeExceedsFile` - Range extends beyond file size
    /// - `PieceReaderError::Torrent` - Failed to read pieces from store
    /// - `PieceReaderError::PieceTooSmall` - Piece smaller than expected for range
    pub async fn read_range(
        &self,
        info_hash: InfoHash,
        range: Range<u64>,
    ) -> Result<Vec<u8>, PieceReaderError> {
        if range.start >= range.end {
            return Err(PieceReaderError::InvalidRange {
                start: range.start,
                end: range.end,
            });
        }

        let piece_count = self
            .piece_store
            .piece_count(info_hash)
            .map_err(PieceReaderError::Torrent)?;
        let piece_size = self.piece_size as u64;

        // Calculate file size - last piece may be smaller
        let file_size =
            calculate_file_size(piece_count, piece_size, info_hash, &*self.piece_store).await?;

        if range.end > file_size {
            return Err(PieceReaderError::RangeExceedsFile {
                start: range.start,
                end: range.end,
                file_size,
            });
        }

        let range_length = range.end - range.start;
        let start_piece_index = (range.start / piece_size) as u32;
        let end_piece_index = ((range.end - 1) / piece_size) as u32;

        let mut result = Vec::with_capacity(range_length as usize);
        let mut bytes_remaining = range_length;
        let mut current_file_position = range.start;

        for piece_index in start_piece_index..=end_piece_index {
            let piece_data = self
                .piece_store
                .piece_data(info_hash, PieceIndex::new(piece_index))
                .await?;

            // Calculate which part of this piece we need
            let piece_start_position = piece_index as u64 * piece_size;
            let offset_in_piece = if current_file_position >= piece_start_position {
                (current_file_position - piece_start_position) as usize
            } else {
                0
            };

            // Validate piece has enough data for our offset
            if offset_in_piece >= piece_data.len() {
                return Err(PieceReaderError::PieceTooSmall {
                    piece_index,
                    actual: piece_data.len(),
                    minimum: offset_in_piece + 1,
                });
            }

            let available_in_piece = piece_data.len() - offset_in_piece;
            let bytes_to_copy = (bytes_remaining as usize).min(available_in_piece);

            if bytes_to_copy > 0 {
                let end_pos = offset_in_piece + bytes_to_copy;
                result.extend_from_slice(&piece_data[offset_in_piece..end_pos]);
                current_file_position += bytes_to_copy as u64;
                bytes_remaining -= bytes_to_copy as u64;
            }

            if bytes_remaining == 0 {
                break;
            }
        }

        Ok(result)
    }

    /// Get total file size by calculating from pieces
    ///
    /// # Errors
    /// - `PieceReaderError::Torrent` - Failed to access piece store
    pub async fn file_size(&self, info_hash: InfoHash) -> Result<u64, PieceReaderError> {
        let piece_count = self
            .piece_store
            .piece_count(info_hash)
            .map_err(PieceReaderError::Torrent)?;
        let piece_size = self.piece_size as u64;

        calculate_file_size(piece_count, piece_size, info_hash, &*self.piece_store)
            .await
            .map_err(PieceReaderError::Torrent)
    }
}

/// Creates a piece reader from a trait object for dynamic dispatch
pub fn create_piece_reader_from_trait_object(
    piece_store: Arc<dyn PieceStore>,
    piece_size: u32,
) -> PieceBasedStreamReader<dyn PieceStore> {
    PieceBasedStreamReader {
        piece_store,
        piece_size,
    }
}

/// Calculate the actual file size by examining the last piece
async fn calculate_file_size<P: PieceStore + ?Sized>(
    piece_count: u32,
    piece_size: u64,
    info_hash: InfoHash,
    piece_store: &P,
) -> Result<u64, TorrentError> {
    if piece_count == 0 {
        return Ok(0);
    }

    if piece_count == 1 {
        // Single piece - file size is the piece size
        let piece_data = piece_store
            .piece_data(info_hash, PieceIndex::new(0))
            .await?;
        return Ok(piece_data.len() as u64);
    }

    // Multiple pieces - last piece determines actual size
    let last_piece_index = piece_count - 1;
    let last_piece_data = piece_store
        .piece_data(info_hash, PieceIndex::new(last_piece_index))
        .await?;
    let last_piece_size = last_piece_data.len() as u64;

    // All pieces except last are full size
    let full_pieces_size = (piece_count - 1) as u64 * piece_size;
    Ok(full_pieces_size + last_piece_size)
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;

    // Type alias for complex piece collection map
    type PieceCollectionMap = HashMap<InfoHash, HashMap<u32, Vec<u8>>>;

    // Mock piece store for testing
    struct MockPieceStore {
        pieces: PieceCollectionMap,
    }

    impl MockPieceStore {
        fn new() -> Self {
            Self {
                pieces: HashMap::new(),
            }
        }

        fn add_piece(&mut self, info_hash: InfoHash, piece_index: u32, data: Vec<u8>) {
            self.pieces
                .entry(info_hash)
                .or_default()
                .insert(piece_index, data);
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
                .get(&info_hash)
                .and_then(|pieces| pieces.get(&piece_index.as_u32()))
                .cloned()
                .ok_or(TorrentError::TorrentNotFound { info_hash })
        }

        fn has_piece(&self, info_hash: InfoHash, piece_index: PieceIndex) -> bool {
            self.pieces
                .get(&info_hash)
                .map(|pieces| pieces.contains_key(&piece_index.as_u32()))
                .unwrap_or(false)
        }

        fn piece_count(&self, info_hash: InfoHash) -> Result<u32, TorrentError> {
            Ok(self
                .pieces
                .get(&info_hash)
                .map(|pieces| pieces.len() as u32)
                .unwrap_or(0))
        }
    }

    fn create_test_info_hash() -> InfoHash {
        InfoHash::new([1u8; 20])
    }

    #[tokio::test]
    async fn test_read_full_file_single_piece() {
        let mut store = MockPieceStore::new();
        let info_hash = create_test_info_hash();
        let test_data = b"Hello, World! This is a test file.".to_vec();

        store.add_piece(info_hash, 0, test_data.clone());

        let reader = PieceBasedStreamReader::new(Arc::new(store), 1024);
        let result = reader
            .read_range(info_hash, 0..test_data.len() as u64)
            .await
            .unwrap();

        assert_eq!(result, test_data);
    }

    #[tokio::test]
    async fn test_read_partial_range_single_piece() {
        let mut store = MockPieceStore::new();
        let info_hash = create_test_info_hash();
        let test_data = b"Hello, World! This is a test file.".to_vec();

        store.add_piece(info_hash, 0, test_data.clone());

        let reader = PieceBasedStreamReader::new(Arc::new(store), 1024);
        let result = reader.read_range(info_hash, 7..12).await.unwrap();

        assert_eq!(result, b"World");
    }

    #[tokio::test]
    async fn test_read_across_multiple_pieces() {
        let mut store = MockPieceStore::new(); // Small pieces for testing
        let info_hash = create_test_info_hash();

        // Create test data spanning 3 pieces
        store.add_piece(info_hash, 0, b"0123456789".to_vec()); // Full piece
        store.add_piece(info_hash, 1, b"abcdefghij".to_vec()); // Full piece
        store.add_piece(info_hash, 2, b"ABCDE".to_vec()); // Partial last piece

        let reader = PieceBasedStreamReader::new(Arc::new(store), 10);

        // Read across piece boundaries
        let result = reader.read_range(info_hash, 8..22).await.unwrap();
        assert_eq!(result, b"89abcdefghijAB");
    }

    #[tokio::test]
    async fn test_invalid_range() {
        let store = MockPieceStore::new();
        let reader = PieceBasedStreamReader::new(Arc::new(store), 1024);
        let info_hash = create_test_info_hash();

        #[allow(clippy::reversed_empty_ranges)]
        let result = reader.read_range(info_hash, 10..5).await;
        assert!(matches!(result, Err(PieceReaderError::InvalidRange { .. })));
    }

    #[tokio::test]
    async fn test_range_exceeds_file() {
        let mut store = MockPieceStore::new();
        let info_hash = create_test_info_hash();
        let test_data = b"Small file".to_vec();

        store.add_piece(info_hash, 0, test_data);

        let reader = PieceBasedStreamReader::new(Arc::new(store), 1024);
        let result = reader.read_range(info_hash, 0..100).await;

        assert!(matches!(
            result,
            Err(PieceReaderError::RangeExceedsFile { .. })
        ));
    }

    #[tokio::test]
    async fn test_file_size_calculation() {
        let mut store = MockPieceStore::new();
        let info_hash = create_test_info_hash();

        store.add_piece(info_hash, 0, b"0123456789".to_vec()); // Full piece
        store.add_piece(info_hash, 1, b"ABCDE".to_vec()); // Partial last piece

        let reader = PieceBasedStreamReader::new(Arc::new(store), 10);
        let file_size = reader.file_size(info_hash).await.unwrap();

        assert_eq!(file_size, 15); // 10 + 5
    }
}
