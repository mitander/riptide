//! Storage caching for assembled file ranges
//!
//! Provides intelligent caching of assembled byte ranges to optimize streaming
//! performance. Uses LRU eviction policy and supports cache statistics for monitoring.

use std::ops::Range;
use std::sync::Arc;
use std::time::Instant;

use lru::LruCache;
use tokio::sync::RwLock;

use crate::torrent::InfoHash;

/// Errors that can occur during cache operations
#[derive(Debug, thiserror::Error)]
pub enum StorageCacheError {
    #[error("Cache is full and cannot store more data")]
    CacheFull,

    #[error("Invalid cache key: {reason}")]
    InvalidKey { reason: String },

    #[error("Cache entry too large: {size} bytes exceeds limit {limit}")]
    EntryTooLarge { size: usize, limit: usize },
}

/// Cache key for assembled byte ranges
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct CacheKey {
    pub info_hash: InfoHash,
    pub start: u64,
    pub end: u64,
}

impl CacheKey {
    /// Create new cache key from info hash and byte range
    pub fn new(info_hash: InfoHash, range: Range<u64>) -> Self {
        Self {
            info_hash,
            start: range.start,
            end: range.end,
        }
    }

    /// Get the size of the range this key represents
    pub fn range_size(&self) -> u64 {
        self.end - self.start
    }

    /// Convert to byte range
    pub fn to_range(&self) -> Range<u64> {
        self.start..self.end
    }
}

/// Cached entry with metadata
#[derive(Debug, Clone)]
pub struct CacheEntry {
    pub data: Vec<u8>,
    pub created_at: Instant,
    pub access_count: u64,
    pub last_accessed: Instant,
}

impl CacheEntry {
    /// Create new cache entry
    pub fn new(data: Vec<u8>) -> Self {
        let now = Instant::now();
        Self {
            data,
            created_at: now,
            access_count: 1,
            last_accessed: now,
        }
    }

    /// Mark entry as accessed
    pub fn mark_accessed(&mut self) {
        self.access_count += 1;
        self.last_accessed = Instant::now();
    }

    /// Get age of entry
    pub fn age(&self) -> std::time::Duration {
        self.created_at.elapsed()
    }

    /// Get time since last access
    pub fn idle_time(&self) -> std::time::Duration {
        self.last_accessed.elapsed()
    }
}

/// Storage cache configuration
#[derive(Debug, Clone)]
pub struct StorageCacheConfig {
    /// Maximum number of entries to cache
    pub max_entries: usize,
    /// Maximum size per entry (in bytes)
    pub max_entry_size: usize,
    /// Maximum total cache size (in bytes)
    pub max_total_size: usize,
}

impl Default for StorageCacheConfig {
    fn default() -> Self {
        Self {
            max_entries: 64,
            max_entry_size: 1024 * 1024,      // 1MB
            max_total_size: 64 * 1024 * 1024, // 64MB
        }
    }
}

/// Cache statistics for monitoring
#[derive(Debug, Clone)]
pub struct CacheStatistics {
    pub entries: usize,
    pub capacity: usize,
    pub total_size: usize,
    pub hit_count: u64,
    pub miss_count: u64,
    pub eviction_count: u64,
    pub hit_rate: f64,
}

impl CacheStatistics {
    /// Calculate hit rate percentage
    pub fn calculate_hit_rate(hit_count: u64, miss_count: u64) -> f64 {
        if hit_count + miss_count == 0 {
            0.0
        } else {
            (hit_count as f64) / ((hit_count + miss_count) as f64)
        }
    }
}

/// High-performance storage cache for assembled file ranges
pub struct StorageCache {
    cache: Arc<RwLock<LruCache<CacheKey, CacheEntry>>>,
    config: StorageCacheConfig,
    hit_count: Arc<RwLock<u64>>,
    miss_count: Arc<RwLock<u64>>,
    eviction_count: Arc<RwLock<u64>>,
    current_size: Arc<RwLock<usize>>,
}

impl StorageCache {
    /// Create new storage cache with configuration
    pub fn new(config: StorageCacheConfig) -> Self {
        let capacity = std::num::NonZeroUsize::new(config.max_entries)
            .unwrap_or_else(|| std::num::NonZeroUsize::new(64).unwrap());

        Self {
            cache: Arc::new(RwLock::new(LruCache::new(capacity))),
            config,
            hit_count: Arc::new(RwLock::new(0)),
            miss_count: Arc::new(RwLock::new(0)),
            eviction_count: Arc::new(RwLock::new(0)),
            current_size: Arc::new(RwLock::new(0)),
        }
    }

    /// Create cache with default configuration
    pub fn new_default() -> Self {
        Self::new(StorageCacheConfig::default())
    }

    /// Get entry from cache
    pub async fn get(&self, key: &CacheKey) -> Option<Vec<u8>> {
        let mut cache = self.cache.write().await;

        if let Some(entry) = cache.get_mut(key) {
            entry.mark_accessed();
            let mut hit_count = self.hit_count.write().await;
            *hit_count += 1;

            tracing::debug!(
                "Cache hit for range {}-{} of torrent {} (size: {} bytes)",
                key.start,
                key.end,
                key.info_hash,
                entry.data.len()
            );

            Some(entry.data.clone())
        } else {
            let mut miss_count = self.miss_count.write().await;
            *miss_count += 1;

            tracing::debug!(
                "Cache miss for range {}-{} of torrent {}",
                key.start,
                key.end,
                key.info_hash
            );

            None
        }
    }

    /// Put entry into cache
    ///
    /// # Errors
    /// Returns `StorageCacheError::EntryTooLarge` if the entry exceeds the maximum size limit.
    pub async fn put(&self, key: CacheKey, data: Vec<u8>) -> Result<(), StorageCacheError> {
        // Validate entry size
        if data.len() > self.config.max_entry_size {
            return Err(StorageCacheError::EntryTooLarge {
                size: data.len(),
                limit: self.config.max_entry_size,
            });
        }

        let entry = CacheEntry::new(data);
        let entry_size = entry.data.len();

        let mut cache = self.cache.write().await;
        let mut current_size = self.current_size.write().await;

        // Check if we need to evict entries to make room
        while *current_size + entry_size > self.config.max_total_size && !cache.is_empty() {
            if let Some((_, evicted_entry)) = cache.pop_lru() {
                *current_size -= evicted_entry.data.len();
                let mut eviction_count = self.eviction_count.write().await;
                *eviction_count += 1;

                tracing::debug!(
                    "Evicted cache entry (size: {} bytes) to make room",
                    evicted_entry.data.len()
                );
            }
        }

        // Add the new entry
        if let Some(old_entry) = cache.put(key.clone(), entry) {
            *current_size -= old_entry.data.len();
        }
        *current_size += entry_size;

        tracing::debug!(
            "Cached range {}-{} of torrent {} (size: {} bytes)",
            key.start,
            key.end,
            key.info_hash,
            entry_size
        );

        Ok(())
    }

    /// Remove all entries for a specific torrent
    pub async fn clear_torrent(&self, info_hash: InfoHash) {
        let mut cache = self.cache.write().await;
        let mut current_size = self.current_size.write().await;

        // Collect keys to remove
        let keys_to_remove: Vec<CacheKey> = cache
            .iter()
            .filter(|(key, _)| key.info_hash == info_hash)
            .map(|(key, _)| key.clone())
            .collect();

        // Remove entries and update size
        for key in keys_to_remove {
            if let Some(entry) = cache.pop(&key) {
                *current_size -= entry.data.len();
            }
        }

        tracing::debug!("Cleared cache entries for torrent {}", info_hash);
    }

    /// Clear all cache entries
    pub async fn clear_all(&self) {
        let mut cache = self.cache.write().await;
        let mut current_size = self.current_size.write().await;

        cache.clear();
        *current_size = 0;

        tracing::debug!("Cleared all cache entries");
    }

    /// Get cache statistics
    pub async fn statistics(&self) -> CacheStatistics {
        let cache = self.cache.read().await;
        let hit_count = *self.hit_count.read().await;
        let miss_count = *self.miss_count.read().await;
        let eviction_count = *self.eviction_count.read().await;
        let current_size = *self.current_size.read().await;

        CacheStatistics {
            entries: cache.len(),
            capacity: cache.cap().get(),
            total_size: current_size,
            hit_count,
            miss_count,
            eviction_count,
            hit_rate: CacheStatistics::calculate_hit_rate(hit_count, miss_count),
        }
    }

    /// Check if cache contains key
    pub async fn contains_key(&self, key: &CacheKey) -> bool {
        let cache = self.cache.read().await;
        cache.contains(key)
    }

    /// Get list of cached ranges for a torrent
    pub async fn cached_ranges(&self, info_hash: InfoHash) -> Vec<Range<u64>> {
        let cache = self.cache.read().await;
        cache
            .iter()
            .filter(|(key, _)| key.info_hash == info_hash)
            .map(|(key, _)| key.to_range())
            .collect()
    }

    /// Reset cache statistics
    pub async fn reset_statistics(&self) {
        let mut hit_count = self.hit_count.write().await;
        let mut miss_count = self.miss_count.write().await;
        let mut eviction_count = self.eviction_count.write().await;

        *hit_count = 0;
        *miss_count = 0;
        *eviction_count = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_info_hash() -> InfoHash {
        InfoHash::new([1u8; 20])
    }

    #[tokio::test]
    async fn test_storage_cache_basic_operations() {
        let cache = StorageCache::new_default();
        let info_hash = create_test_info_hash();
        let key = CacheKey::new(info_hash, 0..10);
        let data = b"test data!".to_vec();

        // Test cache miss
        assert!(cache.get(&key).await.is_none());

        // Test cache put
        cache.put(key.clone(), data.clone()).await.unwrap();

        // Test cache hit
        let cached_data = cache.get(&key).await.unwrap();
        assert_eq!(cached_data, data);

        // Verify statistics
        let stats = cache.statistics().await;
        assert_eq!(stats.entries, 1);
        assert_eq!(stats.hit_count, 1);
        assert_eq!(stats.miss_count, 1);
        assert_eq!(stats.hit_rate, 0.5);
    }

    #[tokio::test]
    async fn test_storage_cache_eviction() {
        let config = StorageCacheConfig {
            max_entries: 2,
            max_entry_size: 1024,
            max_total_size: 10, // Very small to force eviction
        };
        let cache = StorageCache::new(config);
        let info_hash = create_test_info_hash();

        // Add first entry
        let key1 = CacheKey::new(info_hash, 0..10);
        let data1 = b"12345".to_vec();
        cache.put(key1.clone(), data1.clone()).await.unwrap();

        // Add second entry
        let key2 = CacheKey::new(info_hash, 10..20);
        let data2 = b"67890".to_vec();
        cache.put(key2.clone(), data2.clone()).await.unwrap();

        // Add third entry, should evict first
        let key3 = CacheKey::new(info_hash, 20..30);
        let data3 = b"abcde".to_vec();
        cache.put(key3.clone(), data3.clone()).await.unwrap();

        // First entry should be evicted
        assert!(cache.get(&key1).await.is_none());
        assert!(cache.get(&key2).await.is_some());
        assert!(cache.get(&key3).await.is_some());

        // Check statistics
        let stats = cache.statistics().await;
        assert!(stats.eviction_count > 0);
    }

    #[tokio::test]
    async fn test_storage_cache_entry_too_large() {
        let config = StorageCacheConfig {
            max_entries: 10,
            max_entry_size: 5, // Very small limit
            max_total_size: 1024,
        };
        let cache = StorageCache::new(config);
        let info_hash = create_test_info_hash();
        let key = CacheKey::new(info_hash, 0..10);
        let large_data = vec![0u8; 10]; // Exceeds max_entry_size

        let result = cache.put(key, large_data).await;
        assert!(matches!(
            result,
            Err(StorageCacheError::EntryTooLarge { .. })
        ));
    }

    #[tokio::test]
    async fn test_storage_cache_clear_torrent() {
        let cache = StorageCache::new_default();
        let info_hash1 = create_test_info_hash();
        let info_hash2 = InfoHash::new([2u8; 20]);

        // Add entries for both torrents
        let key1 = CacheKey::new(info_hash1, 0..10);
        let key2 = CacheKey::new(info_hash2, 0..10);
        let data = b"test".to_vec();

        cache.put(key1.clone(), data.clone()).await.unwrap();
        cache.put(key2.clone(), data.clone()).await.unwrap();

        // Clear first torrent
        cache.clear_torrent(info_hash1).await;

        // Only second torrent should remain
        assert!(cache.get(&key1).await.is_none());
        assert!(cache.get(&key2).await.is_some());
    }

    #[tokio::test]
    async fn test_storage_cache_clear_all() {
        let cache = StorageCache::new_default();
        let info_hash = create_test_info_hash();
        let key = CacheKey::new(info_hash, 0..10);
        let data = b"test".to_vec();

        cache.put(key.clone(), data).await.unwrap();
        assert!(cache.get(&key).await.is_some());

        cache.clear_all().await;
        assert!(cache.get(&key).await.is_none());

        let stats = cache.statistics().await;
        assert_eq!(stats.entries, 0);
        assert_eq!(stats.total_size, 0);
    }

    #[tokio::test]
    async fn test_cache_key_operations() {
        let info_hash = create_test_info_hash();
        let range = 100..200;
        let key = CacheKey::new(info_hash, range.clone());

        assert_eq!(key.range_size(), 100);
        assert_eq!(key.to_range(), range);
        assert_eq!(key.info_hash, info_hash);
    }

    #[tokio::test]
    async fn test_cache_entry_tracking() {
        let data = b"test data".to_vec();
        let mut entry = CacheEntry::new(data.clone());

        assert_eq!(entry.data, data);
        assert_eq!(entry.access_count, 1);
        assert!(entry.age() < std::time::Duration::from_millis(100));
        assert!(entry.idle_time() < std::time::Duration::from_millis(100));

        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        entry.mark_accessed();

        assert_eq!(entry.access_count, 2);
        assert!(entry.age() > std::time::Duration::from_millis(5));
    }

    #[tokio::test]
    async fn test_cached_ranges() {
        let cache = StorageCache::new_default();
        let info_hash = create_test_info_hash();
        let data = b"test".to_vec();

        // Add multiple ranges
        let ranges = vec![0..10, 20..30, 40..50];
        for range in &ranges {
            let key = CacheKey::new(info_hash, range.clone());
            cache.put(key, data.clone()).await.unwrap();
        }

        // Get cached ranges
        let mut cached_ranges = cache.cached_ranges(info_hash).await;
        cached_ranges.sort_by_key(|r| r.start);

        assert_eq!(cached_ranges.len(), 3);
        assert_eq!(cached_ranges, ranges);
    }

    #[tokio::test]
    async fn test_statistics_calculation() {
        assert_eq!(CacheStatistics::calculate_hit_rate(0, 0), 0.0);
        assert_eq!(CacheStatistics::calculate_hit_rate(10, 0), 1.0);
        assert_eq!(CacheStatistics::calculate_hit_rate(0, 10), 0.0);
        assert_eq!(CacheStatistics::calculate_hit_rate(7, 3), 0.7);
    }

    #[tokio::test]
    async fn test_reset_statistics() {
        let cache = StorageCache::new_default();
        let info_hash = create_test_info_hash();
        let key = CacheKey::new(info_hash, 0..10);
        let data = b"test".to_vec();

        // Generate some statistics
        cache.put(key.clone(), data).await.unwrap();
        cache.get(&key).await;
        cache.get(&CacheKey::new(info_hash, 100..110)).await; // Miss

        let stats = cache.statistics().await;
        assert!(stats.hit_count > 0);
        assert!(stats.miss_count > 0);

        // Reset statistics
        cache.reset_statistics().await;

        let stats = cache.statistics().await;
        assert_eq!(stats.hit_count, 0);
        assert_eq!(stats.miss_count, 0);
        assert_eq!(stats.eviction_count, 0);
    }
}
