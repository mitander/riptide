//! LocalDataSource implementation for local file access
//!
//! Provides unified data access for files managed by FileLibraryManager,
//! enabling streaming of local files through the same interface as torrent pieces.
//! Consolidates functionality from LocalFileAssembler into the new DataSource architecture.

use std::ops::Range;
use std::sync::Arc;

use async_trait::async_trait;
use tokio::sync::RwLock;
use tracing::{debug, error, info, warn};

use super::{
    CacheStats, CacheableDataSource, DataError, DataResult, DataSource, RangeAvailability,
    validate_range_bounds,
};
use crate::storage::FileLibraryManager;
use crate::torrent::InfoHash;

/// Simple cache statistics for local data source
#[derive(Debug, Clone)]
struct LocalCacheMetrics {
    hits: u64,
    misses: u64,
    total_bytes_read: u64,
}

impl LocalCacheMetrics {
    fn new() -> Self {
        Self {
            hits: 0,
            misses: 0,
            total_bytes_read: 0,
        }
    }

    fn record_hit(&mut self) {
        self.hits += 1;
    }

    fn record_miss(&mut self, bytes_read: u64) {
        self.misses += 1;
        self.total_bytes_read += bytes_read;
    }
}

/// DataSource implementation for local files managed by FileLibraryManager
///
/// This implementation provides access to local media files through the unified
/// DataSource interface, enabling consistent streaming behavior regardless of
/// whether content comes from torrents or local storage.
pub struct LocalDataSource {
    file_manager: Arc<RwLock<FileLibraryManager>>,
    metrics: Arc<RwLock<LocalCacheMetrics>>,
}

impl LocalDataSource {
    /// Create new local data source with file manager
    pub fn new(file_manager: Arc<RwLock<FileLibraryManager>>) -> Self {
        Self {
            file_manager,
            metrics: Arc::new(RwLock::new(LocalCacheMetrics::new())),
        }
    }

    /// Check if file exists in the library
    async fn file_exists(&self, info_hash: InfoHash) -> bool {
        let file_manager = self.file_manager.read().await;
        file_manager.file_by_hash(info_hash).is_some()
    }

    /// Get file metadata from library
    async fn get_file_info(&self, info_hash: InfoHash) -> DataResult<(String, u64)> {
        let file_manager = self.file_manager.read().await;

        match file_manager.file_by_hash(info_hash) {
            Some(file) => {
                debug!(
                    "Found local file for {}: {} ({} bytes)",
                    info_hash, file.title, file.size
                );
                Ok((file.title.clone(), file.size))
            }
            None => {
                error!("Local file not found for hash {}", info_hash);
                Err(DataError::FileNotFound { info_hash })
            }
        }
    }

    /// Update metrics after a read operation
    async fn update_metrics(&self, cache_hit: bool, bytes_read: u64) {
        let mut metrics = self.metrics.write().await;

        if cache_hit {
            metrics.record_hit();
        } else {
            metrics.record_miss(bytes_read);
        }
    }
}

#[async_trait]
impl DataSource for LocalDataSource {
    async fn read_range(&self, info_hash: InfoHash, range: Range<u64>) -> DataResult<Vec<u8>> {
        debug!(
            "LocalDataSource: Reading range {}..{} (length: {}) for {}",
            range.start,
            range.end,
            range.end - range.start,
            info_hash
        );

        // Validate range format
        if range.start >= range.end {
            return Err(DataError::InvalidRange {
                start: range.start,
                end: range.end,
            });
        }

        // Get file info and validate range bounds
        let (file_name, file_size) = self.get_file_info(info_hash).await?;
        validate_range_bounds(&range, file_size)?;

        let length = range.end - range.start;

        // Read data from file manager
        let file_manager = self.file_manager.read().await;
        match file_manager
            .read_file_segment(info_hash, range.start, length)
            .await
        {
            Ok(data) => {
                self.update_metrics(false, data.len() as u64).await;

                debug!(
                    "LocalDataSource: Successfully read {} bytes from {} (requested {})",
                    data.len(),
                    file_name,
                    length
                );

                if data.len() != length as usize {
                    warn!(
                        "LocalDataSource: Read size mismatch for {}: got {} bytes, expected {}",
                        info_hash,
                        data.len(),
                        length
                    );
                }

                Ok(data)
            }
            Err(e) => {
                error!(
                    "LocalDataSource: Failed to read range {}..{} from {}: {}",
                    range.start, range.end, file_name, e
                );
                Err(DataError::Storage {
                    reason: format!("Failed to read local file segment: {e}"),
                })
            }
        }
    }

    async fn file_size(&self, info_hash: InfoHash) -> DataResult<u64> {
        debug!("LocalDataSource: Getting file size for {}", info_hash);

        let (file_name, size) = self.get_file_info(info_hash).await?;

        debug!(
            "LocalDataSource: File size for {} ({}) is {} bytes",
            info_hash, file_name, size
        );

        Ok(size)
    }

    async fn check_range_availability(
        &self,
        info_hash: InfoHash,
        range: Range<u64>,
    ) -> DataResult<RangeAvailability> {
        debug!(
            "LocalDataSource: Checking availability of range {}..{} for {}",
            range.start, range.end, info_hash
        );

        // For local files, check if file exists and range is valid
        let file_exists = self.file_exists(info_hash).await;

        if !file_exists {
            return Ok(RangeAvailability {
                available: false,
                missing_pieces: Vec::new(),
                cache_hit: false,
            });
        }

        // Validate range bounds
        let (_, file_size) = self.get_file_info(info_hash).await?;
        let range_valid = validate_range_bounds(&range, file_size).is_ok();

        debug!(
            "LocalDataSource: Range {}..{} for {} - available: {}, valid: {}",
            range.start, range.end, info_hash, file_exists, range_valid
        );

        Ok(RangeAvailability {
            available: file_exists && range_valid,
            missing_pieces: Vec::new(), // Local files don't have pieces
            cache_hit: false,           // Local files don't use cache in the same way
        })
    }

    fn source_type(&self) -> &'static str {
        "local_data_source"
    }

    async fn can_handle(&self, info_hash: InfoHash) -> bool {
        let can_handle = self.file_exists(info_hash).await;

        debug!("LocalDataSource: Can handle {}: {}", info_hash, can_handle);

        can_handle
    }
}

#[async_trait]
impl CacheableDataSource for LocalDataSource {
    async fn cache_stats(&self) -> CacheStats {
        let metrics = self.metrics.read().await;

        // Local files don't use traditional caching, but we track access patterns
        CacheStats {
            hits: metrics.hits,
            misses: metrics.misses,
            evictions: 0,    // No evictions for local files
            memory_usage: 0, // No persistent memory cache
            entry_count: 0,  // No cached entries
        }
    }

    async fn clear_cache(&self, info_hash: InfoHash) -> DataResult<()> {
        // Local files don't maintain a cache, but we can reset metrics
        info!(
            "LocalDataSource: Cache clear requested for {} (no-op)",
            info_hash
        );
        Ok(())
    }

    async fn clear_all_cache(&self) -> DataResult<()> {
        // Reset metrics
        {
            let mut metrics = self.metrics.write().await;
            *metrics = LocalCacheMetrics::new();
        }

        info!("LocalDataSource: All cache metrics reset");
        Ok(())
    }
}

/// Create a new LocalDataSource from a FileLibraryManager
pub fn create_local_data_source(file_manager: Arc<RwLock<FileLibraryManager>>) -> LocalDataSource {
    LocalDataSource::new(file_manager)
}

#[cfg(test)]
mod tests {

    use tempfile::TempDir;

    use super::*;

    async fn create_test_file_manager() -> (Arc<RwLock<FileLibraryManager>>, TempDir, InfoHash) {
        let temp_dir = TempDir::new().unwrap();
        let file_manager = Arc::new(RwLock::new(FileLibraryManager::new()));

        // Create a test file
        let test_file_path = temp_dir.path().join("test.mp4");
        let test_data = vec![42u8; 1024];

        // Write test data to file
        tokio::fs::write(&test_file_path, &test_data).await.unwrap();

        // Scan the directory to add the file to the library
        let info_hash = {
            let mut manager = file_manager.write().await;
            manager.scan_directory(temp_dir.path()).await.unwrap();

            // Get the actual info_hash from the scanned files
            manager.all_files()[0].info_hash
        };

        (file_manager, temp_dir, info_hash)
    }

    #[tokio::test]
    async fn test_source_type() {
        let (file_manager, _temp_dir, _) = create_test_file_manager().await;
        let source = LocalDataSource::new(file_manager);

        assert_eq!(source.source_type(), "local_data_source");
    }

    #[tokio::test]
    async fn test_file_existence_check() {
        let (file_manager, _temp_dir, info_hash) = create_test_file_manager().await;
        let source = LocalDataSource::new(file_manager);

        // Test existing file
        assert!(source.can_handle(info_hash).await);

        // Test non-existing file
        let unknown_hash = InfoHash::new([2u8; 20]);
        assert!(!source.can_handle(unknown_hash).await);
    }

    #[tokio::test]
    async fn test_file_size() {
        let (file_manager, _temp_dir, info_hash) = create_test_file_manager().await;
        let source = LocalDataSource::new(file_manager);

        let size = source.file_size(info_hash).await.unwrap();
        assert_eq!(size, 1024);
    }

    #[tokio::test]
    async fn test_range_availability() {
        let (file_manager, _temp_dir, info_hash) = create_test_file_manager().await;
        let source = LocalDataSource::new(file_manager);

        // Valid range
        let availability = source
            .check_range_availability(info_hash, 0..512)
            .await
            .unwrap();
        assert!(availability.available);
        assert!(availability.missing_pieces.is_empty());

        // Invalid range (exceeds file size)
        let availability = source.check_range_availability(info_hash, 0..2048).await;
        // This should either error or return unavailable
        assert!(availability.is_err() || !availability.unwrap().available);
    }

    #[tokio::test]
    async fn test_invalid_ranges() {
        let (file_manager, _temp_dir, info_hash) = create_test_file_manager().await;
        let source = LocalDataSource::new(file_manager);

        // Invalid range (start >= end)
        let start = 500;
        let end = 400;
        let invalid_range = start..end;
        let result = source.read_range(info_hash, invalid_range).await;
        assert!(matches!(result, Err(DataError::InvalidRange { .. })));

        // Range exceeds file size
        let result = source.read_range(info_hash, 0..2048).await;
        assert!(matches!(result, Err(DataError::RangeExceedsFile { .. })));
    }

    #[tokio::test]
    async fn test_cache_stats() {
        let (file_manager, _temp_dir, _) = create_test_file_manager().await;
        let source = LocalDataSource::new(file_manager);

        let stats = source.cache_stats().await;
        assert_eq!(stats.hits, 0);
        assert_eq!(stats.misses, 0);
        assert_eq!(stats.evictions, 0);
        assert_eq!(stats.memory_usage, 0);
        assert_eq!(stats.entry_count, 0);
    }

    #[tokio::test]
    async fn test_cache_clearing() {
        let (file_manager, _temp_dir, info_hash) = create_test_file_manager().await;
        let source = LocalDataSource::new(file_manager);

        // Clear cache should not error
        assert!(source.clear_cache(info_hash).await.is_ok());
        assert!(source.clear_all_cache().await.is_ok());
    }

    #[tokio::test]
    async fn test_file_not_found() {
        let (file_manager, _temp_dir, _) = create_test_file_manager().await;
        let source = LocalDataSource::new(file_manager);

        let unknown_hash = InfoHash::new([2u8; 20]);

        // Should return FileNotFound error
        let result = source.file_size(unknown_hash).await;
        assert!(matches!(result, Err(DataError::FileNotFound { .. })));

        let result = source.read_range(unknown_hash, 0..100).await;
        assert!(matches!(result, Err(DataError::FileNotFound { .. })));
    }
}
