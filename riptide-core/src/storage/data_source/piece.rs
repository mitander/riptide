//! PieceDataSource implementation for torrent piece-based data access
//!
//! Provides unified data access for torrent pieces with intelligent caching
//! and range optimization. Consolidates functionality from FileAssembler
//! and PieceReader into a single, efficient implementation.

use std::collections::HashMap;
use std::ops::Range;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use async_trait::async_trait;
use lru::LruCache;
use tokio::sync::RwLock;
use tracing::{debug, info};

use super::range::{PieceRange, RangeCalculator, TorrentLayout};
use super::{
    CacheStats, CacheableDataSource, DataError, DataResult, DataSource, RangeAvailability,
    RangeKey, validate_range_bounds,
};
use crate::torrent::{InfoHash, PieceIndex, PieceStore};

/// Cached metadata for torrents to avoid repeated calculations
#[derive(Debug, Clone)]
struct TorrentMetadata {
    layout: TorrentLayout,
    calculator: RangeCalculator,
}

/// DataSource implementation for BitTorrent pieces with intelligent caching
pub struct PieceDataSource {
    piece_store: Arc<dyn PieceStore>,
    range_cache: Arc<RwLock<LruCache<RangeKey, Vec<u8>>>>,
    metadata_cache: Arc<RwLock<HashMap<InfoHash, TorrentMetadata>>>,
    cache_hits: Arc<AtomicUsize>,
    cache_misses: Arc<AtomicUsize>,
    cache_evictions: Arc<AtomicUsize>,
}

impl PieceDataSource {
    /// Create new piece data source with specified cache size
    pub fn new(piece_store: Arc<dyn PieceStore>, max_cache_entries: Option<usize>) -> Self {
        let cache_size = max_cache_entries.unwrap_or(100);

        Self {
            piece_store,
            range_cache: Arc::new(RwLock::new(LruCache::new(
                std::num::NonZeroUsize::new(cache_size).unwrap(),
            ))),
            metadata_cache: Arc::new(RwLock::new(HashMap::new())),
            cache_hits: Arc::new(AtomicUsize::new(0)),
            cache_misses: Arc::new(AtomicUsize::new(0)),
            cache_evictions: Arc::new(AtomicUsize::new(0)),
        }
    }

    /// Get or create torrent metadata for efficient range calculations
    async fn get_metadata(&self, info_hash: InfoHash) -> DataResult<TorrentMetadata> {
        // Check cache first
        {
            let metadata_cache = self.metadata_cache.read().await;
            if let Some(metadata) = metadata_cache.get(&info_hash) {
                return Ok(metadata.clone());
            }
        }

        // Create new metadata
        let piece_count =
            self.piece_store
                .piece_count(info_hash)
                .map_err(|e| DataError::Storage {
                    reason: format!("Failed to get piece count: {e}"),
                })?;

        // Infer piece size from the first piece
        let piece_size = if piece_count > 0 {
            let first_piece = self
                .piece_store
                .piece_data(info_hash, PieceIndex::new(0))
                .await
                .map_err(|e| DataError::Storage {
                    reason: format!("Failed to get first piece for size calculation: {e}"),
                })?;
            first_piece.len() as u32
        } else {
            return Err(DataError::Storage {
                reason: "Cannot determine piece size: no pieces available".to_string(),
            });
        };

        // Calculate total size - handle last piece potentially being smaller
        let mut total_size = 0u64;
        for i in 0..piece_count {
            let piece_data = self
                .piece_store
                .piece_data(info_hash, PieceIndex::new(i))
                .await
                .map_err(|e| DataError::Storage {
                    reason: format!("Failed to get piece {i} for size calculation: {e}"),
                })?;
            total_size += piece_data.len() as u64;
        }

        let layout = TorrentLayout::new(piece_size, piece_count, total_size);
        let calculator = RangeCalculator::new(layout.clone());

        let metadata = TorrentMetadata { layout, calculator };

        // Cache the metadata
        {
            let mut metadata_cache = self.metadata_cache.write().await;
            metadata_cache.insert(info_hash, metadata.clone());
        }

        debug!(
            "Created metadata for {}: {} pieces, {} bytes",
            info_hash, piece_count, total_size
        );

        Ok(metadata)
    }

    /// Assemble data from multiple pieces efficiently
    async fn assemble_from_pieces(
        &self,
        info_hash: InfoHash,
        pieces: &[PieceRange],
    ) -> DataResult<Vec<u8>> {
        let mut result = Vec::new();

        for piece_range in pieces {
            let piece_index = PieceIndex::new(piece_range.piece_index);

            // Check if piece is available
            if !self.piece_store.has_piece(info_hash, piece_index) {
                return Err(DataError::InsufficientData {
                    start: 0, // Will be filled in by caller
                    end: 0,   // Will be filled in by caller
                    missing_count: 1,
                });
            }

            // Get piece data
            let piece_data = self
                .piece_store
                .piece_data(info_hash, piece_index)
                .await
                .map_err(|e| DataError::Storage {
                    reason: format!("Failed to get piece {}: {}", piece_range.piece_index, e),
                })?;

            // Extract the required range from the piece
            let start = piece_range.start_offset as usize;
            let end = (piece_range.end_offset + 1) as usize; // Convert inclusive to exclusive

            if end > piece_data.len() {
                return Err(DataError::Storage {
                    reason: format!(
                        "Piece {} data too short: need {} bytes, got {}",
                        piece_range.piece_index,
                        end,
                        piece_data.len()
                    ),
                });
            }

            result.extend_from_slice(&piece_data[start..end]);

            debug!(
                "Assembled {} bytes from piece {} (offset {}..{})",
                end - start,
                piece_range.piece_index,
                start,
                end
            );
        }

        Ok(result)
    }

    /// Check if all required pieces are available
    async fn check_pieces_available(
        &self,
        info_hash: InfoHash,
        pieces: &[PieceRange],
    ) -> (bool, Vec<u32>) {
        let mut available = true;
        let mut missing_pieces = Vec::new();

        for piece_range in pieces {
            let piece_index = PieceIndex::new(piece_range.piece_index);
            if !self.piece_store.has_piece(info_hash, piece_index) {
                available = false;
                missing_pieces.push(piece_range.piece_index);
            }
        }

        (available, missing_pieces)
    }
}

#[async_trait]
impl DataSource for PieceDataSource {
    async fn read_range(&self, info_hash: InfoHash, range: Range<u64>) -> DataResult<Vec<u8>> {
        let range_key = RangeKey::new(info_hash, range.clone());

        // Check cache first
        {
            let mut cache = self.range_cache.write().await;
            if let Some(cached_data) = cache.get(&range_key) {
                self.cache_hits.fetch_add(1, Ordering::Relaxed);
                debug!(
                    "Cache hit for range {}..{} ({})",
                    range.start, range.end, info_hash
                );
                return Ok(cached_data.clone());
            }
        }

        self.cache_misses.fetch_add(1, Ordering::Relaxed);
        debug!(
            "Cache miss for range {}..{} ({})",
            range.start, range.end, info_hash
        );

        // Get metadata and validate range
        let metadata = self.get_metadata(info_hash).await?;
        validate_range_bounds(&range, metadata.layout.total_size)?;

        // Calculate required pieces
        let pieces = metadata.calculator.pieces_for_range(range.clone())?;

        // Check if all pieces are available
        let (available, missing_pieces) = self.check_pieces_available(info_hash, &pieces).await;

        if !available {
            return Err(DataError::InsufficientData {
                start: range.start,
                end: range.end,
                missing_count: missing_pieces.len(),
            });
        }

        // Assemble data from pieces
        let data = self.assemble_from_pieces(info_hash, &pieces).await?;

        // Cache the result
        {
            let mut cache = self.range_cache.write().await;
            if let Some(evicted_data) = cache.put(range_key, data.clone()) {
                self.cache_evictions.fetch_add(1, Ordering::Relaxed);
                debug!("Evicted cached data ({} bytes)", evicted_data.len());
            }
        }

        info!(
            "Successfully read range {}..{} ({} bytes) from {} pieces",
            range.start,
            range.end,
            data.len(),
            pieces.len()
        );

        Ok(data)
    }

    async fn file_size(&self, info_hash: InfoHash) -> DataResult<u64> {
        let metadata = self.get_metadata(info_hash).await?;
        Ok(metadata.layout.total_size)
    }

    async fn check_range_availability(
        &self,
        info_hash: InfoHash,
        range: Range<u64>,
    ) -> DataResult<RangeAvailability> {
        let range_key = RangeKey::new(info_hash, range.clone());

        // Check cache first
        let cache_hit = {
            let cache = self.range_cache.read().await;
            cache.contains(&range_key)
        };

        if cache_hit {
            return Ok(RangeAvailability {
                available: true,
                missing_pieces: Vec::new(),
                cache_hit: true,
            });
        }

        // Get metadata and validate range
        let metadata = self.get_metadata(info_hash).await?;
        validate_range_bounds(&range, metadata.layout.total_size)?;

        // Calculate required pieces
        let pieces = metadata.calculator.pieces_for_range(range)?;

        // Check piece availability
        let (available, missing_pieces) = self.check_pieces_available(info_hash, &pieces).await;

        Ok(RangeAvailability {
            available,
            missing_pieces,
            cache_hit: false,
        })
    }

    fn source_type(&self) -> &'static str {
        "piece_data_source"
    }

    async fn can_handle(&self, info_hash: InfoHash) -> bool {
        // Check if we have any pieces for this torrent
        self.piece_store.piece_count(info_hash).is_ok()
    }
}

#[async_trait]
impl CacheableDataSource for PieceDataSource {
    async fn cache_stats(&self) -> CacheStats {
        let cache = self.range_cache.read().await;

        CacheStats {
            hits: self.cache_hits.load(Ordering::Relaxed) as u64,
            misses: self.cache_misses.load(Ordering::Relaxed) as u64,
            evictions: self.cache_evictions.load(Ordering::Relaxed) as u64,
            memory_usage: 0,
            entry_count: cache.len(),
        }
    }

    async fn clear_cache(&self, info_hash: InfoHash) -> DataResult<()> {
        let mut cache = self.range_cache.write().await;

        // Remove all entries for this info hash
        let keys_to_remove: Vec<_> = cache
            .iter()
            .filter(|(key, _)| key.info_hash == info_hash)
            .map(|(key, _)| key.clone())
            .collect();

        for key in keys_to_remove {
            cache.pop(&key);
        }

        // Also clear metadata cache
        {
            let mut metadata_cache = self.metadata_cache.write().await;
            metadata_cache.remove(&info_hash);
        }

        debug!("Cleared cache for torrent {}", info_hash);
        Ok(())
    }

    async fn clear_all_cache(&self) -> DataResult<()> {
        {
            let mut cache = self.range_cache.write().await;
            cache.clear();
        }

        {
            let mut metadata_cache = self.metadata_cache.write().await;
            metadata_cache.clear();
        }

        info!("Cleared all caches");
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use async_trait::async_trait;

    use super::*;
    use crate::torrent::{PieceIndex, PieceStore, TorrentError};

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

        fn add_piece(&mut self, info_hash: InfoHash, index: u32, data: Vec<u8>) {
            self.pieces.insert((info_hash, index), data);
            *self.piece_counts.entry(info_hash).or_insert(0) =
                (*self.piece_counts.get(&info_hash).unwrap_or(&0)).max(index + 1);
        }
    }

    #[async_trait]
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

    fn create_test_piece_store() -> Arc<MockPieceStore> {
        let mut store = MockPieceStore::new();
        let info_hash = InfoHash::new([1u8; 20]);

        // Add some test data for 3 pieces of 1024 bytes each
        for i in 0..3 {
            let piece_data = vec![i as u8; 1024];
            store.add_piece(info_hash, i, piece_data);
        }

        Arc::new(store)
    }

    #[tokio::test]
    async fn test_basic_range_reading() {
        let store = create_test_piece_store();
        let source = PieceDataSource::new(store, Some(10));
        let info_hash = InfoHash::new([1u8; 20]);

        // Read from first piece
        let data = source.read_range(info_hash, 0..512).await.unwrap();
        assert_eq!(data.len(), 512);
        assert_eq!(data[0], 0);

        // Read across piece boundary
        let data = source.read_range(info_hash, 1020..1028).await.unwrap();
        assert_eq!(data.len(), 8);
        assert_eq!(data[0], 0); // From piece 0
        assert_eq!(data[4], 1); // From piece 1
    }

    #[tokio::test]
    async fn test_cache_functionality() {
        let store = create_test_piece_store();
        let source = PieceDataSource::new(store, Some(10));
        let info_hash = InfoHash::new([1u8; 20]);

        // First read should be cache miss
        let data1 = source.read_range(info_hash, 0..512).await.unwrap();
        let stats1 = source.cache_stats().await;
        assert_eq!(stats1.misses, 1);
        assert_eq!(stats1.hits, 0);

        // Second read should be cache hit
        let data2 = source.read_range(info_hash, 0..512).await.unwrap();
        let stats2 = source.cache_stats().await;
        assert_eq!(stats2.misses, 1);
        assert_eq!(stats2.hits, 1);

        assert_eq!(data1, data2);
    }

    #[tokio::test]
    async fn test_range_availability() {
        let store = create_test_piece_store();
        let source = PieceDataSource::new(store, Some(10));
        let info_hash = InfoHash::new([1u8; 20]);

        // Check availability of valid range
        let availability = source
            .check_range_availability(info_hash, 0..512)
            .await
            .unwrap();
        assert!(availability.available);
        assert!(availability.missing_pieces.is_empty());
        assert!(!availability.cache_hit);
    }

    #[tokio::test]
    async fn test_invalid_range() {
        let store = create_test_piece_store();
        let source = PieceDataSource::new(store, Some(10));
        let info_hash = InfoHash::new([1u8; 20]);

        // Range exceeds file size
        let result = source.read_range(info_hash, 0..5000).await;
        assert!(matches!(result, Err(DataError::RangeExceedsFile { .. })));

        // Invalid range
        let start = 1000;
        let end = 500;
        let invalid_range = start..end;
        let result = source.read_range(info_hash, invalid_range).await;
        assert!(matches!(result, Err(DataError::InvalidRange { .. })));
    }

    #[tokio::test]
    async fn test_cache_clearing() {
        let store = create_test_piece_store();
        let source = PieceDataSource::new(store, Some(10));
        let info_hash = InfoHash::new([1u8; 20]);

        // Fill cache
        source.read_range(info_hash, 0..512).await.unwrap();
        let stats = source.cache_stats().await;
        assert_eq!(stats.entry_count, 1);

        // Clear cache for specific torrent
        source.clear_cache(info_hash).await.unwrap();
        let stats = source.cache_stats().await;
        assert_eq!(stats.entry_count, 0);
    }

    #[tokio::test]
    async fn test_source_type() {
        let store = create_test_piece_store();
        let source = PieceDataSource::new(store, Some(10));

        assert_eq!(source.source_type(), "piece_data_source");
    }

    #[tokio::test]
    async fn test_can_handle() {
        let store = create_test_piece_store();
        let source = PieceDataSource::new(store, Some(10));
        let info_hash = InfoHash::new([1u8; 20]);
        let unknown_hash = InfoHash::new([2u8; 20]);

        assert!(source.can_handle(info_hash).await);
        assert!(!source.can_handle(unknown_hash).await);
    }
}
