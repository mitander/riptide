//! Unified data source abstraction for streaming operations
//!
//! Provides a consistent interface for accessing file data from various sources
//! including torrent pieces, local files, and cached data. Consolidates the
//! functionality previously scattered across multiple file assembler implementations.

pub mod cache;
pub mod local;
pub mod piece;
pub mod range;

use std::ops::Range;
use std::sync::Arc;

use async_trait::async_trait;
// Re-export implementations
pub use cache::{
    CacheEntry, CacheKey, CacheStatistics, StorageCache, StorageCacheConfig, StorageCacheError,
};
pub use local::{LocalDataSource, create_local_data_source};
pub use piece::PieceDataSource;
pub use range::{
    BufferInfo, PiecePriority, PieceRange, RangeCalculator, TorrentLayout,
    create_range_calculator_for_info_hash,
};

use crate::torrent::InfoHash;

/// Unified error type for all data source operations
#[derive(Debug, thiserror::Error)]
pub enum DataError {
    /// Byte range is invalid (start >= end)
    #[error("Invalid range: start {start} >= end {end}")]
    InvalidRange {
        /// Start byte position of the invalid range
        start: u64,
        /// End byte position of the invalid range
        end: u64,
    },

    /// Requested range extends beyond file boundaries
    #[error("Range {start}..{end} exceeds file size {file_size}")]
    RangeExceedsFile {
        /// Start byte position of the range
        start: u64,
        /// End byte position of the range
        end: u64,
        /// Total size of the file
        file_size: u64,
    },

    /// Required data pieces are not available
    #[error("Insufficient data: missing {missing_count} pieces for range {start}..{end}")]
    InsufficientData {
        /// Start byte position of the requested range
        start: u64,
        /// End byte position of the requested range
        end: u64,
        /// Number of missing pieces needed for the range
        missing_count: usize,
    },

    /// Generic storage system error
    #[error("Storage error: {reason}")]
    Storage {
        /// Description of the storage error
        reason: String,
    },

    /// Cache-related operation failed
    #[error("Cache error: {reason}")]
    Cache {
        /// Description of the cache error
        reason: String,
    },

    /// Requested file could not be found
    #[error("File not found: {info_hash}")]
    FileNotFound {
        /// Info hash of the file that was not found
        info_hash: InfoHash,
    },

    /// Underlying torrent operation failed
    #[error("Torrent error: {source}")]
    Torrent {
        /// The underlying torrent error
        #[from]
        source: crate::torrent::TorrentError,
    },

    /// Underlying I/O operation failed
    #[error("I/O error: {source}")]
    Io {
        /// The underlying I/O error
        #[from]
        source: std::io::Error,
    },
}

/// Result type for data source operations
pub type DataResult<T> = Result<T, DataError>;

/// Key for caching byte ranges across data sources
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct RangeKey {
    /// Info hash identifying the file
    pub info_hash: InfoHash,
    /// Start byte position of the range
    pub start: u64,
    /// End byte position of the range
    pub end: u64,
}

impl RangeKey {
    /// Create new range key from info hash and byte range
    pub fn new(info_hash: InfoHash, range: Range<u64>) -> Self {
        Self {
            info_hash,
            start: range.start,
            end: range.end,
        }
    }

    /// Get the byte range represented by this key
    pub fn range(&self) -> Range<u64> {
        self.start..self.end
    }

    /// Calculate the size of this range in bytes
    pub fn size(&self) -> u64 {
        self.end - self.start
    }
}

/// Information about data availability for a specific range
#[derive(Debug, Clone)]
pub struct RangeAvailability {
    /// Whether the complete range is available
    pub available: bool,
    /// List of piece indices that are missing for this range
    pub missing_pieces: Vec<u32>,
    /// Whether this range was found in cache
    pub cache_hit: bool,
}

/// Unified trait for accessing file data from various sources
///
/// This trait consolidates functionality from FileAssembler, PieceReader,
/// and other data access patterns into a single, consistent interface.
#[async_trait]
pub trait DataSource: Send + Sync {
    /// Read arbitrary byte range from the data source
    ///
    /// # Arguments
    /// * `info_hash` - Identifier for the file/torrent
    /// * `range` - Byte range to read (start..end)
    ///
    /// # Returns
    /// Vector containing the requested bytes
    ///
    /// # Errors
    /// - `DataError::InvalidRange` - Invalid range parameters
    /// - `DataError::RangeExceedsFile` - Range beyond file size
    /// - `DataError::InsufficientData` - Missing required data
    /// - `DataError::Storage` - Storage access error
    async fn read_range(&self, info_hash: InfoHash, range: Range<u64>) -> DataResult<Vec<u8>>;

    /// Get total file size without downloading entire file
    ///
    /// # Arguments
    /// * `info_hash` - Identifier for the file/torrent
    ///
    /// # Returns
    /// Total file size in bytes
    ///
    /// # Errors
    /// - `DataError::FileNotFound` - File not found
    /// - `DataError::Storage` - Storage access error
    async fn file_size(&self, info_hash: InfoHash) -> DataResult<u64>;

    /// Check if byte range is available without downloading
    ///
    /// # Arguments
    /// * `info_hash` - Identifier for the file/torrent
    /// * `range` - Byte range to check
    ///
    /// # Returns
    /// Availability information including cache status
    async fn check_range_availability(
        &self,
        info_hash: InfoHash,
        range: Range<u64>,
    ) -> DataResult<RangeAvailability>;

    /// Get the name/identifier for this data source type
    fn source_type(&self) -> &'static str;

    /// Check if this data source can handle the given info hash
    async fn can_handle(&self, info_hash: InfoHash) -> bool;
}

/// Extension trait for data sources that support caching
#[async_trait]
pub trait CacheableDataSource: DataSource {
    /// Get cache statistics for this data source
    async fn cache_stats(&self) -> CacheStats;

    /// Clear cache for specific info hash
    async fn clear_cache(&self, info_hash: InfoHash) -> DataResult<()>;

    /// Clear all cached data
    async fn clear_all_cache(&self) -> DataResult<()>;
}

/// Statistics about cache performance
#[derive(Debug, Clone)]
pub struct CacheStats {
    /// Number of cache hits
    pub hits: u64,
    /// Number of cache misses
    pub misses: u64,
    /// Number of cache entries evicted
    pub evictions: u64,
    /// Total memory usage in bytes
    pub memory_usage: u64,
    /// Number of entries currently in cache
    pub entry_count: usize,
}

impl CacheStats {
    /// Calculate cache hit rate as percentage
    pub fn hit_rate(&self) -> f64 {
        let total = self.hits + self.misses;
        if total == 0 {
            0.0
        } else {
            (self.hits as f64 / total as f64) * 100.0
        }
    }
}

/// Validate that a byte range is well-formed
///
/// # Errors
/// - `DataError::InvalidRange` if start >= end
pub fn validate_range(range: &Range<u64>) -> DataResult<()> {
    if range.start >= range.end {
        return Err(DataError::InvalidRange {
            start: range.start,
            end: range.end,
        });
    }
    Ok(())
}

/// Validate that a byte range is within file bounds
///
/// # Errors
/// - `DataError::InvalidRange` if range is malformed
/// - `DataError::RangeExceedsFile` if range extends beyond file size
pub fn validate_range_bounds(range: &Range<u64>, file_size: u64) -> DataResult<()> {
    validate_range(range)?;

    if range.end > file_size {
        return Err(DataError::RangeExceedsFile {
            start: range.start,
            end: range.end,
            file_size,
        });
    }
    Ok(())
}

/// Create a boxed data source from a trait object
pub fn create_data_source_from_trait_object(source: Arc<dyn DataSource>) -> Box<dyn DataSource> {
    Box::new(TraitObjectWrapper { inner: source })
}

/// Wrapper to convert Arc<dyn DataSource> to Box<dyn DataSource>
struct TraitObjectWrapper {
    inner: Arc<dyn DataSource>,
}

#[async_trait]
impl DataSource for TraitObjectWrapper {
    async fn read_range(&self, info_hash: InfoHash, range: Range<u64>) -> DataResult<Vec<u8>> {
        self.inner.read_range(info_hash, range).await
    }

    async fn file_size(&self, info_hash: InfoHash) -> DataResult<u64> {
        self.inner.file_size(info_hash).await
    }

    async fn check_range_availability(
        &self,
        info_hash: InfoHash,
        range: Range<u64>,
    ) -> DataResult<RangeAvailability> {
        self.inner.check_range_availability(info_hash, range).await
    }

    fn source_type(&self) -> &'static str {
        self.inner.source_type()
    }

    async fn can_handle(&self, info_hash: InfoHash) -> bool {
        self.inner.can_handle(info_hash).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_range_key_creation() {
        let info_hash = InfoHash::new([1u8; 20]);
        let range = 100..200;
        let key = RangeKey::new(info_hash, range.clone());

        assert_eq!(key.info_hash, info_hash);
        assert_eq!(key.start, 100);
        assert_eq!(key.end, 200);
        assert_eq!(key.range(), range);
        assert_eq!(key.size(), 100);
    }

    #[test]
    fn test_validate_range() {
        // Valid range
        assert!(validate_range(&(100..200)).is_ok());

        // Invalid range (start >= end)
        let start = 200;
        let end = 100;
        let invalid_range = start..end;
        assert!(validate_range(&invalid_range).is_err());
        assert!(validate_range(&(100..100)).is_err());
    }

    #[test]
    fn test_validate_range_bounds() {
        let file_size = 1000;

        // Valid range within bounds
        assert!(validate_range_bounds(&(100..200), file_size).is_ok());

        // Range exceeds file size
        assert!(validate_range_bounds(&(100..1100), file_size).is_err());

        // Range at exact file boundary
        assert!(validate_range_bounds(&(100..1000), file_size).is_ok());
    }

    #[test]
    fn test_cache_stats_hit_rate() {
        let stats = CacheStats {
            hits: 80,
            misses: 20,
            evictions: 5,
            memory_usage: 1024,
            entry_count: 10,
        };

        assert_eq!(stats.hit_rate(), 80.0);

        // Test zero division
        let empty_stats = CacheStats {
            hits: 0,
            misses: 0,
            evictions: 0,
            memory_usage: 0,
            entry_count: 0,
        };

        assert_eq!(empty_stats.hit_rate(), 0.0);
    }
}
