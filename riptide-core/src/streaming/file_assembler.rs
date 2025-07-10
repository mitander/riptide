//! File assembly from BitTorrent pieces with intelligent caching
//!
//! Provides efficient byte range reconstruction from torrent pieces with LRU caching
//! for optimal streaming performance. Supports seeking to arbitrary positions and
//! partial downloads.

use std::collections::HashMap;
use std::ops::Range;
use std::sync::Arc;

use lru::LruCache;
use tokio::sync::RwLock;

use crate::torrent::{InfoHash, PieceIndex, PieceStore, TorrentError};

/// Errors that can occur during file assembly operations
#[derive(Debug, thiserror::Error)]
pub enum FileAssemblerError {
    #[error("Torrent error: {0}")]
    Torrent(#[from] TorrentError),

    #[error("Invalid range: start {start} >= end {end}")]
    InvalidRange { start: u64, end: u64 },

    #[error("Range {start}-{end} exceeds file size {file_size}")]
    RangeExceedsFile {
        start: u64,
        end: u64,
        file_size: u64,
    },

    #[error("Insufficient data: missing {missing_count} pieces for range {start}-{end}")]
    InsufficientData {
        start: u64,
        end: u64,
        missing_count: usize,
    },

    #[error("Cache error: {reason}")]
    CacheError { reason: String },
}

/// Key for caching assembled byte ranges
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct RangeKey {
    info_hash: InfoHash,
    start: u64,
    end: u64,
}

impl RangeKey {
    fn new(info_hash: InfoHash, range: Range<u64>) -> Self {
        Self {
            info_hash,
            start: range.start,
            end: range.end,
        }
    }
}

/// Torrent metadata cached for efficient range calculations
#[derive(Debug, Clone)]
struct TorrentMetadata {
    piece_size: u32,
    file_size: u64,
}

/// Trait for assembling files from torrent pieces with seeking support
#[async_trait::async_trait]
pub trait FileAssembler: Send + Sync {
    /// Read arbitrary byte range from reconstructed file
    ///
    /// # Errors
    /// - `FileAssemblerError::InvalidRange` - Invalid range parameters
    /// - `FileAssemblerError::RangeExceedsFile` - Range beyond file size
    /// - `FileAssemblerError::InsufficientData` - Missing required pieces
    async fn read_range(
        &self,
        info_hash: InfoHash,
        range: Range<u64>,
    ) -> Result<Vec<u8>, FileAssemblerError>;

    /// Get file size without downloading entire file
    ///
    /// # Errors
    /// - `FileAssemblerError::Torrent` - Failed to access torrent metadata
    async fn file_size(&self, info_hash: InfoHash) -> Result<u64, FileAssemblerError>;

    /// Check if byte range is available without downloading
    fn is_range_available(&self, info_hash: InfoHash, range: Range<u64>) -> bool;
}

/// Implementation of FileAssembler using torrent pieces with LRU cache
pub struct PieceFileAssembler {
    piece_store: Arc<dyn PieceStore>,
    range_cache: Arc<RwLock<LruCache<RangeKey, Vec<u8>>>>,
    metadata_cache: Arc<RwLock<HashMap<InfoHash, TorrentMetadata>>>,
}

impl PieceFileAssembler {
    /// Create new piece file assembler with specified cache size
    ///
    /// # Arguments
    /// * `piece_store` - Storage backend for torrent pieces
    /// * `cache_size` - Maximum number of byte ranges to cache (default: 64)
    pub fn new(piece_store: Arc<dyn PieceStore>, cache_size: Option<usize>) -> Self {
        let cache_size = cache_size.unwrap_or(64);
        Self {
            piece_store,
            range_cache: Arc::new(RwLock::new(LruCache::new(
                std::num::NonZeroUsize::new(cache_size).unwrap(),
            ))),
            metadata_cache: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Get or compute torrent metadata for efficient range calculations
    async fn get_metadata(
        &self,
        info_hash: InfoHash,
    ) -> Result<TorrentMetadata, FileAssemblerError> {
        // Check cache first
        {
            let cache = self.metadata_cache.read().await;
            if let Some(metadata) = cache.get(&info_hash) {
                return Ok(metadata.clone());
            }
        }

        // Compute metadata
        let piece_count = self.piece_store.piece_count(info_hash)?;
        if piece_count == 0 {
            return Ok(TorrentMetadata {
                piece_size: 0,
                file_size: 0,
            });
        }

        // Find any available piece to determine standard piece size
        let mut piece_size = 0u32;
        for piece_index in 0..piece_count {
            if self
                .piece_store
                .has_piece(info_hash, PieceIndex::new(piece_index))
            {
                let piece = self
                    .piece_store
                    .piece_data(info_hash, PieceIndex::new(piece_index))
                    .await?;
                piece_size = piece.len() as u32;
                break;
            }
        }

        // If no pieces available, we can't determine metadata
        if piece_size == 0 {
            return Err(FileAssemblerError::InsufficientData {
                start: 0,
                end: 0,
                missing_count: piece_count as usize,
            });
        }

        // Calculate file size - try to get last piece, otherwise estimate
        let file_size = if piece_count == 1 {
            piece_size as u64
        } else {
            // Try to get the actual last piece size
            if self
                .piece_store
                .has_piece(info_hash, PieceIndex::new(piece_count - 1))
            {
                let last_piece = self
                    .piece_store
                    .piece_data(info_hash, PieceIndex::new(piece_count - 1))
                    .await?;
                ((piece_count - 1) as u64 * piece_size as u64) + last_piece.len() as u64
            } else {
                // Estimate file size assuming last piece is full size
                // This is an approximation when last piece is missing
                piece_count as u64 * piece_size as u64
            }
        };

        let metadata = TorrentMetadata {
            piece_size,
            file_size,
        };

        // Cache the metadata
        {
            let mut cache = self.metadata_cache.write().await;
            cache.insert(info_hash, metadata.clone());
        }

        Ok(metadata)
    }

    /// Map byte range to required piece indices
    fn range_to_pieces(&self, range: Range<u64>, metadata: &TorrentMetadata) -> Range<u32> {
        let piece_size = metadata.piece_size as u64;
        let start_piece = (range.start / piece_size) as u32;
        let end_piece = ((range.end.saturating_sub(1)) / piece_size) as u32;
        start_piece..end_piece + 1
    }

    /// Assemble byte range from pieces without caching
    async fn assemble_range_from_pieces(
        &self,
        info_hash: InfoHash,
        range: Range<u64>,
        metadata: &TorrentMetadata,
    ) -> Result<Vec<u8>, FileAssemblerError> {
        let piece_range = self.range_to_pieces(range.clone(), metadata);
        let mut missing_pieces = Vec::new();

        // Check which pieces are missing
        for piece_index in piece_range.clone() {
            if !self
                .piece_store
                .has_piece(info_hash, PieceIndex::new(piece_index))
            {
                missing_pieces.push(piece_index);
            }
        }

        // Return error if pieces are missing
        if !missing_pieces.is_empty() {
            return Err(FileAssemblerError::InsufficientData {
                start: range.start,
                end: range.end,
                missing_count: missing_pieces.len(),
            });
        }

        // Assemble data from available pieces
        let mut result = Vec::with_capacity((range.end - range.start) as usize);
        let mut current_position = range.start;
        let piece_size = metadata.piece_size as u64;

        for piece_index in piece_range {
            let piece_data = self
                .piece_store
                .piece_data(info_hash, PieceIndex::new(piece_index))
                .await?;

            // Calculate offset within this piece
            let piece_start = piece_index as u64 * piece_size;
            let offset_in_piece = if current_position >= piece_start {
                (current_position - piece_start) as usize
            } else {
                0
            };

            // Calculate how much data to copy from this piece
            let remaining_in_range = range.end - current_position;
            let available_in_piece = (piece_data.len() - offset_in_piece) as u64;
            let bytes_to_copy = remaining_in_range.min(available_in_piece) as usize;

            if bytes_to_copy > 0 {
                let end_offset = offset_in_piece + bytes_to_copy;
                result.extend_from_slice(&piece_data[offset_in_piece..end_offset]);
                current_position += bytes_to_copy as u64;
            }

            if current_position >= range.end {
                break;
            }
        }

        Ok(result)
    }

    /// Clear cache entries for specific torrent
    pub async fn clear_cache(&self, info_hash: InfoHash) {
        let mut cache = self.range_cache.write().await;

        // Collect keys to remove (LruCache doesn't have retain method)
        let keys_to_remove: Vec<RangeKey> = cache
            .iter()
            .filter(|(key, _)| key.info_hash == info_hash)
            .map(|(key, _)| key.clone())
            .collect();

        // Remove the keys
        for key in keys_to_remove {
            cache.pop(&key);
        }

        let mut metadata_cache = self.metadata_cache.write().await;
        metadata_cache.remove(&info_hash);
    }

    /// Get cache statistics for monitoring
    pub async fn cache_stats(&self) -> CacheStats {
        let cache = self.range_cache.read().await;
        CacheStats {
            size: cache.len(),
            capacity: cache.cap().get(),
            hit_rate: 0.0, // TODO: Implement hit rate tracking
        }
    }
}

#[async_trait::async_trait]
impl FileAssembler for PieceFileAssembler {
    async fn read_range(
        &self,
        info_hash: InfoHash,
        range: Range<u64>,
    ) -> Result<Vec<u8>, FileAssemblerError> {
        // Validate range
        if range.start >= range.end {
            return Err(FileAssemblerError::InvalidRange {
                start: range.start,
                end: range.end,
            });
        }

        // Get metadata for range validation
        let metadata = self.get_metadata(info_hash).await?;

        if range.end > metadata.file_size {
            return Err(FileAssemblerError::RangeExceedsFile {
                start: range.start,
                end: range.end,
                file_size: metadata.file_size,
            });
        }

        // Check cache first
        let cache_key = RangeKey::new(info_hash, range.clone());
        {
            let mut cache = self.range_cache.write().await;
            if let Some(cached_data) = cache.get(&cache_key) {
                tracing::debug!(
                    "Cache hit for range {}-{} of torrent {}",
                    range.start,
                    range.end,
                    info_hash
                );
                return Ok(cached_data.clone());
            }
        }

        // Cache miss - assemble from pieces
        tracing::debug!(
            "Cache miss for range {}-{} of torrent {}, assembling from pieces",
            range.start,
            range.end,
            info_hash
        );

        let data = self
            .assemble_range_from_pieces(info_hash, range.clone(), &metadata)
            .await?;

        // Cache the result if it's reasonable size (< 1MB)
        if data.len() <= 1024 * 1024 {
            let mut cache = self.range_cache.write().await;
            cache.put(cache_key, data.clone());
        }

        Ok(data)
    }

    async fn file_size(&self, info_hash: InfoHash) -> Result<u64, FileAssemblerError> {
        let metadata = self.get_metadata(info_hash).await?;
        Ok(metadata.file_size)
    }

    fn is_range_available(&self, info_hash: InfoHash, range: Range<u64>) -> bool {
        // Try to get cached metadata for accurate piece size calculation
        let piece_size = if let Ok(metadata_cache) = self.metadata_cache.try_read() {
            metadata_cache
                .get(&info_hash)
                .map(|metadata| metadata.piece_size as u64)
                .unwrap_or(262_144) // 256KB default (common torrent piece size)
        } else {
            262_144 // 256KB default if cache is locked
        };

        let start_piece = (range.start / piece_size) as u32;
        let end_piece = ((range.end.saturating_sub(1)) / piece_size) as u32;

        tracing::debug!(
            "Checking range availability for {} range {}-{}, piece_size={}, checking pieces {}-{}",
            info_hash,
            range.start,
            range.end,
            piece_size,
            start_piece,
            end_piece
        );

        for piece_index in start_piece..=end_piece {
            let has_piece = self
                .piece_store
                .has_piece(info_hash, PieceIndex::new(piece_index));

            if !has_piece {
                tracing::debug!(
                    "Range {}-{} not available for {}: missing piece {}",
                    range.start,
                    range.end,
                    info_hash,
                    piece_index
                );
                return false;
            }
        }

        tracing::debug!(
            "Range {}-{} fully available for {} (pieces {}-{})",
            range.start,
            range.end,
            info_hash,
            start_piece,
            end_piece
        );
        true
    }
}

/// Cache statistics for monitoring
#[derive(Debug, Clone)]
pub struct CacheStats {
    pub size: usize,
    pub capacity: usize,
    pub hit_rate: f64,
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;

    // Mock piece store for testing
    struct MockPieceStore {
        pieces: HashMap<InfoHash, HashMap<u32, Vec<u8>>>,
        piece_counts: HashMap<InfoHash, u32>,
    }

    impl MockPieceStore {
        fn new() -> Self {
            Self {
                pieces: HashMap::new(),
                piece_counts: HashMap::new(),
            }
        }

        fn add_piece(&mut self, info_hash: InfoHash, piece_index: u32, data: Vec<u8>) {
            self.pieces
                .entry(info_hash)
                .or_default()
                .insert(piece_index, data);
        }

        fn set_piece_count(&mut self, info_hash: InfoHash, count: u32) {
            self.piece_counts.insert(info_hash, count);
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
                .piece_counts
                .get(&info_hash)
                .copied()
                .unwrap_or_else(|| {
                    self.pieces
                        .get(&info_hash)
                        .map(|pieces| pieces.len() as u32)
                        .unwrap_or(0)
                }))
        }
    }

    fn create_test_info_hash() -> InfoHash {
        InfoHash::new([1u8; 20])
    }

    #[tokio::test]
    async fn test_file_assembler_single_piece() {
        let info_hash = create_test_info_hash();
        let mut store = MockPieceStore::new();
        let test_data = b"Hello, World! This is test data.".to_vec();
        store.add_piece(info_hash, 0, test_data.clone());

        let assembler = PieceFileAssembler::new(Arc::new(store), Some(32));

        // Test full range
        let result = assembler
            .read_range(info_hash, 0..test_data.len() as u64)
            .await
            .unwrap();
        assert_eq!(result, test_data);

        // Test partial range
        let result = assembler.read_range(info_hash, 7..12).await.unwrap();
        assert_eq!(result, b"World");
    }

    #[tokio::test]
    async fn test_file_assembler_multiple_pieces() {
        let info_hash = create_test_info_hash();
        let mut store = MockPieceStore::new();

        // Add pieces with 10 bytes each
        store.add_piece(info_hash, 0, b"0123456789".to_vec());
        store.add_piece(info_hash, 1, b"abcdefghij".to_vec());
        store.add_piece(info_hash, 2, b"ABCDE".to_vec());

        let assembler = PieceFileAssembler::new(Arc::new(store), Some(32));

        // Test reading across piece boundaries
        let result = assembler.read_range(info_hash, 8..22).await.unwrap();
        assert_eq!(result, b"89abcdefghijAB");

        // Test file size
        let file_size = assembler.file_size(info_hash).await.unwrap();
        assert_eq!(file_size, 25); // 10 + 10 + 5
    }

    #[tokio::test]
    async fn test_file_assembler_caching() {
        let info_hash = create_test_info_hash();
        let mut store = MockPieceStore::new();
        store.add_piece(info_hash, 0, b"0123456789".to_vec());

        let assembler = PieceFileAssembler::new(Arc::new(store), Some(32));

        // First read should cache the result
        let result1 = assembler.read_range(info_hash, 2..8).await.unwrap();
        assert_eq!(result1, b"234567");

        // Second read should hit cache
        let result2 = assembler.read_range(info_hash, 2..8).await.unwrap();
        assert_eq!(result2, b"234567");

        // Verify cache stats
        let stats = assembler.cache_stats().await;
        assert_eq!(stats.size, 1);
        assert!(stats.capacity > 0);
    }

    #[tokio::test]
    async fn test_file_assembler_missing_pieces() {
        let info_hash = create_test_info_hash();
        let mut store = MockPieceStore::new();
        store.add_piece(info_hash, 0, b"0123456789".to_vec());
        // Missing piece 1, but set total piece count to 2
        store.set_piece_count(info_hash, 2);

        let assembler = PieceFileAssembler::new(Arc::new(store), Some(32));

        // Should fail when trying to read from missing piece
        let result = assembler.read_range(info_hash, 8..15).await;
        assert!(matches!(
            result,
            Err(FileAssemblerError::InsufficientData { .. })
        ));
    }

    #[tokio::test]
    async fn test_file_assembler_invalid_range() {
        let info_hash = create_test_info_hash();
        let store = MockPieceStore::new();
        let assembler = PieceFileAssembler::new(Arc::new(store), Some(32));

        // Invalid range where start >= end
        #[allow(clippy::reversed_empty_ranges)]
        let result = assembler.read_range(info_hash, 10..5).await;
        assert!(matches!(
            result,
            Err(FileAssemblerError::InvalidRange { .. })
        ));
    }

    #[tokio::test]
    async fn test_file_assembler_range_exceeds_file() {
        let info_hash = create_test_info_hash();
        let mut store = MockPieceStore::new();
        store.add_piece(info_hash, 0, b"small".to_vec());

        let assembler = PieceFileAssembler::new(Arc::new(store), Some(32));

        // Range extends beyond file size
        let result = assembler.read_range(info_hash, 0..100).await;
        assert!(matches!(
            result,
            Err(FileAssemblerError::RangeExceedsFile { .. })
        ));
    }

    #[tokio::test]
    async fn test_is_range_available() {
        let info_hash = create_test_info_hash();
        let mut store = MockPieceStore::new();
        store.add_piece(info_hash, 0, vec![0u8; 1024 * 1024]); // 1MB piece

        let assembler = PieceFileAssembler::new(Arc::new(store), Some(32));

        // Should be available for first piece
        assert!(assembler.is_range_available(info_hash, 0..1024));

        // Should not be available for second piece (missing)
        assert!(!assembler.is_range_available(info_hash, 1024 * 1024..1024 * 1024 + 1024));
    }

    #[tokio::test]
    async fn test_cache_clear() {
        let info_hash = create_test_info_hash();
        let mut store = MockPieceStore::new();
        store.add_piece(info_hash, 0, b"test data".to_vec());

        let assembler = PieceFileAssembler::new(Arc::new(store), Some(32));

        // Add something to cache
        let _result = assembler.read_range(info_hash, 0..5).await.unwrap();

        // Verify cache has data
        let stats = assembler.cache_stats().await;
        assert_eq!(stats.size, 1);

        // Clear cache
        assembler.clear_cache(info_hash).await;

        // Verify cache is empty
        let stats = assembler.cache_stats().await;
        assert_eq!(stats.size, 0);
    }
}
