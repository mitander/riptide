//! Segment caching for transcoded video segments
//!
//! Provides intelligent caching of transcoded video segments with LRU eviction,
//! statistics tracking, and efficient storage management for streaming workloads.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use lru::LruCache;
use tokio::sync::RwLock;

use super::worker_pool::{SegmentKey, TranscodedSegment, VideoFormat, VideoQuality};
use crate::torrent::InfoHash;

/// Errors that can occur during segment cache operations
#[derive(Debug, thiserror::Error)]
pub enum SegmentCacheError {
    #[error("Cache entry not found: {segment_key:?}")]
    EntryNotFound { segment_key: SegmentKey },

    #[error("Cache is full and cannot store more segments")]
    CacheFull,

    #[error("Segment file not found: {path}")]
    FileNotFound { path: PathBuf },

    #[error("I/O error: {reason}")]
    IoError { reason: String },

    #[error("Segment too large: {size} bytes exceeds limit {limit}")]
    SegmentTooLarge { size: u64, limit: u64 },

    #[error("Invalid cache configuration: {reason}")]
    InvalidConfig { reason: String },
}

/// Cached segment entry with metadata
#[derive(Debug, Clone)]
pub struct CachedSegment {
    /// Transcoded segment data
    pub segment: TranscodedSegment,
    /// When this entry was first cached
    pub cached_at: Instant,
    /// Number of times this segment has been accessed
    pub access_count: u64,
    /// Last time this segment was accessed
    pub last_accessed: Instant,
    /// Size of the segment file in bytes
    pub file_size: u64,
}

impl CachedSegment {
    /// Create new cached segment entry
    pub fn new(segment: TranscodedSegment, file_size: u64) -> Self {
        let now = Instant::now();
        Self {
            segment,
            cached_at: now,
            access_count: 1,
            last_accessed: now,
            file_size,
        }
    }

    /// Mark segment as accessed
    pub fn mark_accessed(&mut self) {
        self.access_count += 1;
        self.last_accessed = Instant::now();
    }

    /// Get age of cached entry
    pub fn age(&self) -> Duration {
        self.cached_at.elapsed()
    }

    /// Get time since last access
    pub fn idle_time(&self) -> Duration {
        self.last_accessed.elapsed()
    }

    /// Check if entry should be evicted due to age
    pub fn is_stale(&self, max_age: Duration) -> bool {
        self.age() > max_age
    }
}

/// Configuration for segment cache
#[derive(Debug, Clone)]
pub struct SegmentCacheConfig {
    /// Maximum number of segments to cache
    pub max_segments: usize,
    /// Maximum total cache size in bytes
    pub max_total_size: u64,
    /// Maximum size per segment in bytes
    pub max_segment_size: u64,
    /// Maximum age before eviction
    pub max_age: Duration,
    /// Enable automatic cleanup of stale entries
    pub enable_cleanup: bool,
    /// Cleanup interval
    pub cleanup_interval: Duration,
}

impl Default for SegmentCacheConfig {
    fn default() -> Self {
        Self {
            max_segments: 100,
            max_total_size: 1024 * 1024 * 1024,  // 1GB
            max_segment_size: 100 * 1024 * 1024, // 100MB per segment
            max_age: Duration::from_secs(3600),  // 1 hour
            enable_cleanup: true,
            cleanup_interval: Duration::from_secs(300), // 5 minutes
        }
    }
}

/// Cache statistics for monitoring
#[derive(Debug, Clone)]
pub struct SegmentCacheStats {
    /// Number of segments currently cached
    pub segment_count: usize,
    /// Total cache capacity
    pub capacity: usize,
    /// Total size of cached segments in bytes
    pub total_size: u64,
    /// Number of cache hits
    pub hit_count: u64,
    /// Number of cache misses
    pub miss_count: u64,
    /// Number of segments evicted
    pub eviction_count: u64,
    /// Cache hit rate percentage
    pub hit_rate: f64,
    /// Average segment access count
    pub average_access_count: f64,
    /// Number of segments per quality level
    pub segments_by_quality: HashMap<VideoQuality, usize>,
    /// Number of segments per format
    pub segments_by_format: HashMap<VideoFormat, usize>,
}

impl SegmentCacheStats {
    /// Calculate hit rate percentage
    pub fn calculate_hit_rate(hit_count: u64, miss_count: u64) -> f64 {
        if hit_count + miss_count == 0 {
            0.0
        } else {
            (hit_count as f64) / ((hit_count + miss_count) as f64) * 100.0
        }
    }
}

/// High-performance cache for transcoded video segments
pub struct SegmentCache {
    cache: Arc<RwLock<LruCache<SegmentKey, CachedSegment>>>,
    config: SegmentCacheConfig,
    hit_count: Arc<RwLock<u64>>,
    miss_count: Arc<RwLock<u64>>,
    eviction_count: Arc<RwLock<u64>>,
    current_size: Arc<RwLock<u64>>,
    _cleanup_handle: Option<tokio::task::JoinHandle<()>>,
}

impl SegmentCache {
    /// Create new segment cache with configuration
    pub fn new(config: SegmentCacheConfig) -> Self {
        let capacity = std::num::NonZeroUsize::new(config.max_segments)
            .unwrap_or_else(|| std::num::NonZeroUsize::new(100).unwrap());

        let cache = Arc::new(RwLock::new(LruCache::new(capacity)));
        let hit_count = Arc::new(RwLock::new(0));
        let miss_count = Arc::new(RwLock::new(0));
        let eviction_count = Arc::new(RwLock::new(0));
        let current_size = Arc::new(RwLock::new(0));

        // Start cleanup task if enabled
        let cleanup_handle = if config.enable_cleanup {
            let cache_clone = Arc::clone(&cache);
            let current_size_clone = Arc::clone(&current_size);
            let eviction_count_clone = Arc::clone(&eviction_count);
            let max_age = config.max_age;
            let cleanup_interval = config.cleanup_interval;

            Some(tokio::spawn(async move {
                let mut interval = tokio::time::interval(cleanup_interval);
                loop {
                    interval.tick().await;
                    Self::cleanup_stale_entries(
                        &cache_clone,
                        &current_size_clone,
                        &eviction_count_clone,
                        max_age,
                    )
                    .await;
                }
            }))
        } else {
            None
        };

        Self {
            cache,
            config,
            hit_count,
            miss_count,
            eviction_count,
            current_size,
            _cleanup_handle: cleanup_handle,
        }
    }

    /// Create cache with default configuration
    pub fn new_default() -> Self {
        Self::new(SegmentCacheConfig::default())
    }

    /// Get cached segment by key
    pub async fn get(&self, key: &SegmentKey) -> Option<TranscodedSegment> {
        let mut cache = self.cache.write().await;

        if let Some(cached_segment) = cache.get_mut(key) {
            cached_segment.mark_accessed();
            let mut hit_count = self.hit_count.write().await;
            *hit_count += 1;

            tracing::debug!(
                "Cache hit for segment {:?} (access #{}, age: {:?})",
                key,
                cached_segment.access_count,
                cached_segment.age()
            );

            Some(cached_segment.segment.clone())
        } else {
            let mut miss_count = self.miss_count.write().await;
            *miss_count += 1;

            tracing::debug!("Cache miss for segment {:?}", key);
            None
        }
    }

    /// Store segment in cache
    pub async fn put(
        &self,
        key: SegmentKey,
        segment: TranscodedSegment,
    ) -> Result<(), SegmentCacheError> {
        // Validate segment size
        let file_size = segment.file_size;
        if file_size > self.config.max_segment_size {
            return Err(SegmentCacheError::SegmentTooLarge {
                size: file_size,
                limit: self.config.max_segment_size,
            });
        }

        let cached_segment = CachedSegment::new(segment, file_size);
        let mut cache = self.cache.write().await;
        let mut current_size = self.current_size.write().await;

        // Evict entries if necessary to make room
        while *current_size + file_size > self.config.max_total_size && !cache.is_empty() {
            if let Some((_, evicted_segment)) = cache.pop_lru() {
                *current_size -= evicted_segment.file_size;
                let mut eviction_count = self.eviction_count.write().await;
                *eviction_count += 1;

                tracing::debug!(
                    "Evicted segment due to size limit (freed {} bytes)",
                    evicted_segment.file_size
                );
            }
        }

        // Add the new segment
        if let Some(old_segment) = cache.put(key.clone(), cached_segment) {
            *current_size -= old_segment.file_size;
        }
        *current_size += file_size;

        tracing::debug!(
            "Cached segment {:?} (size: {} bytes, total cache size: {} bytes)",
            key,
            file_size,
            *current_size
        );

        Ok(())
    }

    /// Remove segment from cache
    pub async fn remove(&self, key: &SegmentKey) -> Option<TranscodedSegment> {
        let mut cache = self.cache.write().await;
        let mut current_size = self.current_size.write().await;

        if let Some(cached_segment) = cache.pop(key) {
            *current_size -= cached_segment.file_size;
            Some(cached_segment.segment)
        } else {
            None
        }
    }

    /// Clear all segments for a specific torrent
    pub async fn clear_torrent(&self, info_hash: InfoHash) {
        let mut cache = self.cache.write().await;
        let mut current_size = self.current_size.write().await;

        // Collect keys to remove
        let keys_to_remove: Vec<SegmentKey> = cache
            .iter()
            .filter(|(key, _)| key.info_hash == info_hash)
            .map(|(key, _)| key.clone())
            .collect();

        // Remove entries and update size
        for key in keys_to_remove {
            if let Some(cached_segment) = cache.pop(&key) {
                *current_size -= cached_segment.file_size;
            }
        }

        tracing::debug!("Cleared cache entries for torrent {}", info_hash);
    }

    /// Clear segments for specific quality level
    pub async fn clear_quality(&self, quality: VideoQuality) {
        let mut cache = self.cache.write().await;
        let mut current_size = self.current_size.write().await;

        // Collect keys to remove
        let keys_to_remove: Vec<SegmentKey> = cache
            .iter()
            .filter(|(key, _)| key.quality == quality)
            .map(|(key, _)| key.clone())
            .collect();

        // Remove entries and update size
        for key in keys_to_remove {
            if let Some(cached_segment) = cache.pop(&key) {
                *current_size -= cached_segment.file_size;
            }
        }

        tracing::debug!("Cleared cache entries for quality {:?}", quality);
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
    pub async fn statistics(&self) -> SegmentCacheStats {
        let cache = self.cache.read().await;
        let hit_count = *self.hit_count.read().await;
        let miss_count = *self.miss_count.read().await;
        let eviction_count = *self.eviction_count.read().await;
        let current_size = *self.current_size.read().await;

        // Calculate quality and format distributions
        let mut segments_by_quality = HashMap::new();
        let mut segments_by_format = HashMap::new();
        let mut total_access_count = 0u64;

        for (key, cached_segment) in cache.iter() {
            *segments_by_quality.entry(key.quality).or_insert(0) += 1;
            *segments_by_format.entry(key.format).or_insert(0) += 1;
            total_access_count += cached_segment.access_count;
        }

        let average_access_count = if !cache.is_empty() {
            total_access_count as f64 / cache.len() as f64
        } else {
            0.0
        };

        SegmentCacheStats {
            segment_count: cache.len(),
            capacity: cache.cap().get(),
            total_size: current_size,
            hit_count,
            miss_count,
            eviction_count,
            hit_rate: SegmentCacheStats::calculate_hit_rate(hit_count, miss_count),
            average_access_count,
            segments_by_quality,
            segments_by_format,
        }
    }

    /// Check if cache contains segment
    pub async fn contains(&self, key: &SegmentKey) -> bool {
        let cache = self.cache.read().await;
        cache.contains(key)
    }

    /// Get list of cached segments for a torrent
    pub async fn get_cached_segments(&self, info_hash: InfoHash) -> Vec<SegmentKey> {
        let cache = self.cache.read().await;
        cache
            .iter()
            .filter(|(key, _)| key.info_hash == info_hash)
            .map(|(key, _)| key.clone())
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

    /// Manual cleanup of stale entries
    pub async fn cleanup_stale(&self) {
        Self::cleanup_stale_entries(
            &self.cache,
            &self.current_size,
            &self.eviction_count,
            self.config.max_age,
        )
        .await;
    }

    /// Internal cleanup implementation
    async fn cleanup_stale_entries(
        cache: &Arc<RwLock<LruCache<SegmentKey, CachedSegment>>>,
        current_size: &Arc<RwLock<u64>>,
        eviction_count: &Arc<RwLock<u64>>,
        max_age: Duration,
    ) {
        let mut cache = cache.write().await;
        let mut current_size = current_size.write().await;
        let mut eviction_count = eviction_count.write().await;

        // Collect stale keys
        let stale_keys: Vec<SegmentKey> = cache
            .iter()
            .filter(|(_, cached_segment)| cached_segment.is_stale(max_age))
            .map(|(key, _)| key.clone())
            .collect();

        // Remove stale entries
        for key in stale_keys {
            if let Some(cached_segment) = cache.pop(&key) {
                *current_size -= cached_segment.file_size;
                *eviction_count += 1;
                tracing::debug!("Evicted stale segment {:?}", key);
            }
        }
    }
}

impl Drop for SegmentCache {
    fn drop(&mut self) {
        if let Some(handle) = self._cleanup_handle.take() {
            handle.abort();
        }
    }
}

#[cfg(test)]
mod tests {

    use tempfile::tempdir;

    use super::*;

    fn create_test_segment_key() -> SegmentKey {
        SegmentKey::new(
            InfoHash::new([1u8; 20]),
            Duration::from_secs(0),
            Duration::from_secs(6),
            VideoQuality::Medium,
            VideoFormat::Mp4,
        )
    }

    fn create_test_transcoded_segment(size: u64) -> TranscodedSegment {
        let temp_dir = tempdir().unwrap();
        let output_path = temp_dir.path().join("segment.mp4");

        TranscodedSegment {
            segment: create_test_segment_key(),
            output_path,
            file_size: size,
            duration: Duration::from_secs(6),
            processing_time: Duration::from_secs(2),
        }
    }

    #[tokio::test]
    async fn test_segment_cache_basic_operations() {
        let cache = SegmentCache::new_default();
        let key = create_test_segment_key();
        let segment = create_test_transcoded_segment(1024);

        // Test cache miss
        assert!(cache.get(&key).await.is_none());

        // Test cache put
        cache.put(key.clone(), segment.clone()).await.unwrap();

        // Test cache hit
        let cached_segment = cache.get(&key).await.unwrap();
        assert_eq!(cached_segment.segment.info_hash, segment.segment.info_hash);

        // Verify statistics
        let stats = cache.statistics().await;
        assert_eq!(stats.segment_count, 1);
        assert_eq!(stats.hit_count, 1);
        assert_eq!(stats.miss_count, 1);
        assert_eq!(stats.hit_rate, 50.0);
    }

    #[tokio::test]
    async fn test_segment_cache_size_limits() {
        let config = SegmentCacheConfig {
            max_segments: 3,
            max_total_size: 1000, // Small limit to force eviction
            max_segment_size: 2000,
            ..Default::default()
        };
        let cache = SegmentCache::new(config);

        // Add first segment
        let key1 = SegmentKey::new(
            InfoHash::new([1u8; 20]),
            Duration::from_secs(0),
            Duration::from_secs(6),
            VideoQuality::Low,
            VideoFormat::Mp4,
        );
        let segment1 = create_test_transcoded_segment(400);
        cache.put(key1.clone(), segment1).await.unwrap();

        // Add second segment
        let key2 = SegmentKey::new(
            InfoHash::new([1u8; 20]),
            Duration::from_secs(6),
            Duration::from_secs(6),
            VideoQuality::Low,
            VideoFormat::Mp4,
        );
        let segment2 = create_test_transcoded_segment(400);
        cache.put(key2.clone(), segment2).await.unwrap();

        // Both should fit within size limit (800 < 1000)
        assert!(cache.get(&key1).await.is_some());
        assert!(cache.get(&key2).await.is_some());

        // Add third segment, should evict first (1200 > 1000)
        let key3 = SegmentKey::new(
            InfoHash::new([1u8; 20]),
            Duration::from_secs(12),
            Duration::from_secs(6),
            VideoQuality::Low,
            VideoFormat::Mp4,
        );
        let segment3 = create_test_transcoded_segment(400);
        cache.put(key3.clone(), segment3).await.unwrap();

        // First segment should be evicted
        assert!(cache.get(&key1).await.is_none());
        assert!(cache.get(&key2).await.is_some());
        assert!(cache.get(&key3).await.is_some());

        let stats = cache.statistics().await;
        assert!(stats.eviction_count > 0);
    }

    #[tokio::test]
    async fn test_segment_cache_oversized_segment() {
        let config = SegmentCacheConfig {
            max_segment_size: 1000,
            ..Default::default()
        };
        let cache = SegmentCache::new(config);
        let key = create_test_segment_key();
        let large_segment = create_test_transcoded_segment(2000); // Exceeds limit

        let result = cache.put(key, large_segment).await;
        assert!(matches!(
            result,
            Err(SegmentCacheError::SegmentTooLarge { .. })
        ));
    }

    #[tokio::test]
    async fn test_segment_cache_torrent_clearing() {
        let cache = SegmentCache::new_default();
        let info_hash1 = InfoHash::new([1u8; 20]);
        let info_hash2 = InfoHash::new([2u8; 20]);

        // Add segments for both torrents
        let key1 = SegmentKey::new(
            info_hash1,
            Duration::from_secs(0),
            Duration::from_secs(6),
            VideoQuality::Medium,
            VideoFormat::Mp4,
        );
        let key2 = SegmentKey::new(
            info_hash2,
            Duration::from_secs(0),
            Duration::from_secs(6),
            VideoQuality::Medium,
            VideoFormat::Mp4,
        );

        cache
            .put(key1.clone(), create_test_transcoded_segment(1024))
            .await
            .unwrap();
        cache
            .put(key2.clone(), create_test_transcoded_segment(1024))
            .await
            .unwrap();

        // Clear first torrent
        cache.clear_torrent(info_hash1).await;

        // Only second torrent should remain
        assert!(cache.get(&key1).await.is_none());
        assert!(cache.get(&key2).await.is_some());
    }

    #[tokio::test]
    async fn test_segment_cache_quality_clearing() {
        let cache = SegmentCache::new_default();
        let info_hash = InfoHash::new([1u8; 20]);

        // Add segments with different qualities
        let key_low = SegmentKey::new(
            info_hash,
            Duration::from_secs(0),
            Duration::from_secs(6),
            VideoQuality::Low,
            VideoFormat::Mp4,
        );
        let key_high = SegmentKey::new(
            info_hash,
            Duration::from_secs(0),
            Duration::from_secs(6),
            VideoQuality::High,
            VideoFormat::Mp4,
        );

        cache
            .put(key_low.clone(), create_test_transcoded_segment(1024))
            .await
            .unwrap();
        cache
            .put(key_high.clone(), create_test_transcoded_segment(1024))
            .await
            .unwrap();

        // Clear low quality segments
        cache.clear_quality(VideoQuality::Low).await;

        // Only high quality should remain
        assert!(cache.get(&key_low).await.is_none());
        assert!(cache.get(&key_high).await.is_some());
    }

    #[tokio::test]
    async fn test_cached_segment_metadata() {
        let segment = create_test_transcoded_segment(1024);
        let mut cached = CachedSegment::new(segment, 1024);

        assert_eq!(cached.access_count, 1);
        assert_eq!(cached.file_size, 1024);
        assert!(cached.age() < Duration::from_millis(100));
        assert!(cached.idle_time() < Duration::from_millis(100));

        // Test access tracking
        tokio::time::sleep(Duration::from_millis(10)).await;
        cached.mark_accessed();
        assert_eq!(cached.access_count, 2);
        assert!(cached.age() > Duration::from_millis(5));

        // Test staleness check
        assert!(!cached.is_stale(Duration::from_secs(1)));
    }

    #[tokio::test]
    async fn test_segment_cache_statistics() {
        let cache = SegmentCache::new_default();
        let info_hash = InfoHash::new([1u8; 20]);

        // Add segments with different qualities and formats
        let segments = vec![
            (VideoQuality::Low, VideoFormat::Mp4),
            (VideoQuality::Medium, VideoFormat::Mp4),
            (VideoQuality::High, VideoFormat::WebM),
        ];

        for (i, (quality, format)) in segments.iter().enumerate() {
            let key = SegmentKey::new(
                info_hash,
                Duration::from_secs(i as u64 * 6),
                Duration::from_secs(6),
                *quality,
                *format,
            );
            cache
                .put(key, create_test_transcoded_segment(1024))
                .await
                .unwrap();
        }

        let stats = cache.statistics().await;
        assert_eq!(stats.segment_count, 3);
        assert_eq!(stats.segments_by_quality.len(), 3);
        assert_eq!(stats.segments_by_format.len(), 2);
        assert_eq!(stats.segments_by_format[&VideoFormat::Mp4], 2);
        assert_eq!(stats.segments_by_format[&VideoFormat::WebM], 1);
    }

    #[tokio::test]
    async fn test_segment_cache_reset_statistics() {
        let cache = SegmentCache::new_default();
        let key = create_test_segment_key();
        let segment = create_test_transcoded_segment(1024);

        // Generate some statistics
        cache.put(key.clone(), segment).await.unwrap();
        cache.get(&key).await;

        // Create a different key to ensure cache miss
        let miss_key = SegmentKey::new(
            InfoHash::new([2u8; 20]), // Different info hash
            Duration::from_secs(6),
            Duration::from_secs(6),
            VideoQuality::Medium,
            VideoFormat::Mp4,
        );
        cache.get(&miss_key).await; // Miss

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

    #[tokio::test]
    async fn test_get_cached_segments() {
        let cache = SegmentCache::new_default();
        let info_hash = InfoHash::new([1u8; 20]);

        // Add multiple segments for the torrent
        for i in 0..3 {
            let key = SegmentKey::new(
                info_hash,
                Duration::from_secs(i * 6),
                Duration::from_secs(6),
                VideoQuality::Medium,
                VideoFormat::Mp4,
            );
            cache
                .put(key, create_test_transcoded_segment(1024))
                .await
                .unwrap();
        }

        let cached_segments = cache.get_cached_segments(info_hash).await;
        assert_eq!(cached_segments.len(), 3);

        // Check that all segments belong to the correct torrent
        for segment_key in cached_segments {
            assert_eq!(segment_key.info_hash, info_hash);
        }
    }
}
