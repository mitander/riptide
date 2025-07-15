//! Main chunk buffer implementation for streaming remuxed media

use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use std::time::{Duration, Instant};

use tokio::sync::{Notify, RwLock};
use tracing::{debug, info, warn};

use super::{BufferConfig, BufferError, BufferStats};
use crate::streaming::realtime_remuxer::RemuxedChunk;

/// Main chunk buffer for managing remuxed media data
///
/// This buffer stores remuxed chunks in memory and provides efficient access
/// for HTTP range requests. It implements LRU eviction when capacity is reached
/// and permanently caches header chunks for fast access.
pub struct ChunkBuffer {
    /// Map of offset to chunk data for fast lookup
    chunks: Arc<RwLock<HashMap<u64, RemuxedChunk>>>,

    /// Ordered list of chunk offsets for LRU eviction
    chunk_order: Arc<RwLock<VecDeque<u64>>>,

    /// Permanently cached header chunks (never evicted)
    header_cache: Arc<RwLock<Vec<RemuxedChunk>>>,

    /// Configuration for buffer behavior
    config: BufferConfig,

    /// Statistics tracking
    stats: Arc<RwLock<BufferStats>>,

    /// Notifier for new chunk availability
    new_chunk_notify: Arc<Notify>,

    /// Current total bytes buffered (excluding headers)
    current_bytes: Arc<RwLock<u64>>,

    /// Whether we have received header chunks
    has_headers: Arc<RwLock<bool>>,
}

impl ChunkBuffer {
    /// Creates a new chunk buffer with the given configuration
    pub fn new(config: BufferConfig) -> Self {
        Self {
            chunks: Arc::new(RwLock::new(HashMap::new())),
            chunk_order: Arc::new(RwLock::new(VecDeque::new())),
            header_cache: Arc::new(RwLock::new(Vec::new())),
            config,
            stats: Arc::new(RwLock::new(BufferStats::new())),
            new_chunk_notify: Arc::new(Notify::new()),
            current_bytes: Arc::new(RwLock::new(0)),
            has_headers: Arc::new(RwLock::new(false)),
        }
    }

    /// Writes a remuxed chunk to the buffer
    ///
    /// Header chunks are cached permanently. Regular chunks may be evicted
    /// when the buffer reaches capacity using LRU policy.
    ///
    /// # Errors
    ///
    /// - `BufferError::CapacityExceeded` - If chunk cannot fit even after eviction
    pub async fn write_chunk(&self, chunk: RemuxedChunk) -> Result<(), BufferError> {
        let chunk_size = chunk.data.len() as u64;
        let is_header = chunk.is_header;
        let offset = chunk.offset;

        // Handle header chunks separately
        if is_header {
            let mut header_cache = self.header_cache.write().await;
            header_cache.push(chunk);
            *self.has_headers.write().await = true;

            let mut stats = self.stats.write().await;
            stats.record_chunk_written(chunk_size, true);

            info!(
                "Cached header chunk at offset {} ({} bytes)",
                offset, chunk_size
            );
            self.new_chunk_notify.notify_waiters();
            return Ok(());
        }

        // For regular chunks, check capacity and evict if needed
        {
            let mut chunks = self.chunks.write().await;
            let mut chunk_order = self.chunk_order.write().await;
            let mut current_bytes = self.current_bytes.write().await;
            let mut stats = self.stats.write().await;

            // Evict chunks if we're at capacity
            while (chunks.len() >= self.config.max_chunks
                || *current_bytes + chunk_size > self.config.max_bytes)
                && !chunk_order.is_empty()
            {
                if let Some(oldest_offset) = chunk_order.pop_front() {
                    if let Some(old_chunk) = chunks.remove(&oldest_offset) {
                        let old_size = old_chunk.data.len() as u64;
                        *current_bytes = current_bytes.saturating_sub(old_size);
                        stats.record_chunk_evicted(old_size);

                        debug!(
                            "Evicted chunk at offset {} ({} bytes)",
                            oldest_offset, old_size
                        );
                    }
                }
            }

            // Check if we can still fit the chunk after eviction
            if chunks.len() >= self.config.max_chunks
                || *current_bytes + chunk_size > self.config.max_bytes
            {
                return Err(BufferError::CapacityExceeded);
            }

            // Add the new chunk
            chunks.insert(offset, chunk);
            chunk_order.push_back(offset);
            *current_bytes += chunk_size;

            // Update statistics
            stats.record_chunk_written(chunk_size, false);
            stats.chunks_buffered = chunks.len();
            stats.bytes_buffered = *current_bytes;

            debug!(
                "Buffered chunk at offset {} ({} bytes), total: {}/{} chunks, {}/{} bytes",
                offset,
                chunk_size,
                chunks.len(),
                self.config.max_chunks,
                *current_bytes,
                self.config.max_bytes
            );
        }

        // Notify waiters of new data
        self.new_chunk_notify.notify_waiters();
        Ok(())
    }

    /// Reads data for the specified byte range
    ///
    /// Returns all available data within the requested range from both
    /// header cache and regular buffer. Data is returned in offset order.
    pub async fn read_range(&self, start: u64, end: u64) -> Vec<u8> {
        let mut result = Vec::new();
        let mut found_any = false;

        // First check header cache
        let header_cache = self.header_cache.read().await;
        for chunk in header_cache.iter() {
            if self.chunk_overlaps_range(chunk, start, end) {
                let data = self.extract_range_from_chunk(chunk, start, end);
                result.extend_from_slice(&data);
                found_any = true;
            }
        }
        drop(header_cache);

        // Then check regular chunks
        let chunks = self.chunks.read().await;
        let mut chunk_list: Vec<_> = chunks.iter().collect();
        chunk_list.sort_by_key(|(offset, _)| *offset);

        for (_, chunk) in chunk_list {
            if self.chunk_overlaps_range(chunk, start, end) {
                let data = self.extract_range_from_chunk(chunk, start, end);
                result.extend_from_slice(&data);
                found_any = true;
            }
        }
        drop(chunks);

        // Update statistics
        if found_any {
            self.stats.write().await.record_chunk_served();
        } else {
            self.stats.write().await.record_range_miss();
        }

        debug!(
            "Read range {}-{}: returned {} bytes",
            start,
            end,
            result.len()
        );
        result
    }

    /// Checks if headers are available
    ///
    /// Headers are required for clients to begin playback.
    pub async fn has_headers(&self) -> bool {
        *self.has_headers.read().await
    }

    /// Gets all cached header data concatenated
    ///
    /// Returns the complete header data needed to start playback.
    pub async fn header_data(&self) -> Vec<u8> {
        let header_cache = self.header_cache.read().await;
        let mut result = Vec::new();

        for chunk in header_cache.iter() {
            result.extend_from_slice(&chunk.data);
        }

        result
    }

    /// Waits for new chunks to become available
    ///
    /// This method can be used to implement long-polling for chunk availability.
    pub async fn wait_for_chunks(&self) {
        self.new_chunk_notify.notified().await;
    }

    /// Waits for data to become available in a specific range
    ///
    /// Returns true if data becomes available within the timeout period.
    pub async fn wait_for_range(&self, start: u64, end: u64) -> bool {
        let deadline = Instant::now() + self.config.wait_timeout;

        loop {
            // Check if we have data in this range
            if !self.read_range(start, end).await.is_empty() {
                return true;
            }

            // Calculate remaining time
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return false;
            }

            // Wait for new chunks or timeout
            match tokio::time::timeout(remaining, self.new_chunk_notify.notified()).await {
                Ok(_) => continue,      // New chunks available, check again
                Err(_) => return false, // Timeout
            }
        }
    }

    /// Gets current buffer statistics
    pub async fn stats(&self) -> BufferStats {
        self.stats.read().await.clone()
    }

    /// Clears all buffered chunks except headers
    ///
    /// Headers are preserved to maintain playback capability.
    pub async fn clear(&self) {
        let mut chunks = self.chunks.write().await;
        let mut chunk_order = self.chunk_order.write().await;
        let mut current_bytes = self.current_bytes.write().await;
        let mut stats = self.stats.write().await;

        chunks.clear();
        chunk_order.clear();
        *current_bytes = 0;

        stats.chunks_buffered = 0;
        stats.bytes_buffered = 0;

        info!("Cleared chunk buffer (headers preserved)");
    }

    /// Gets the range of offsets currently buffered
    ///
    /// Returns (min_offset, max_offset) or None if buffer is empty.
    /// Includes both header cache and regular buffer.
    pub async fn buffered_range(&self) -> Option<(u64, u64)> {
        let mut min_offset = u64::MAX;
        let mut max_offset = 0u64;
        let mut found_any = false;

        // Check header cache
        let header_cache = self.header_cache.read().await;
        for chunk in header_cache.iter() {
            min_offset = min_offset.min(chunk.offset);
            max_offset = max_offset.max(chunk.offset + chunk.data.len() as u64);
            found_any = true;
        }
        drop(header_cache);

        // Check regular chunks
        let chunks = self.chunks.read().await;
        for (offset, chunk) in chunks.iter() {
            min_offset = min_offset.min(*offset);
            max_offset = max_offset.max(offset + chunk.data.len() as u64);
            found_any = true;
        }
        drop(chunks);

        if found_any {
            Some((min_offset, max_offset))
        } else {
            None
        }
    }

    /// Checks if buffer is healthy and performing well
    ///
    /// Returns true if hit ratio, utilization, and eviction rate are good.
    pub async fn is_healthy(&self) -> bool {
        let stats = self.stats.read().await;
        stats.is_healthy(self.config.max_bytes)
    }

    /// Gets a summary of buffer status
    pub async fn status_summary(&self) -> String {
        let stats = self.stats.read().await;
        let has_headers = *self.has_headers.read().await;
        let buffered_range = self.buffered_range().await;

        format!(
            "ChunkBuffer: headers={}, range={:?}, {}",
            has_headers,
            buffered_range,
            stats.format_summary(self.config.max_bytes)
        )
    }

    /// Helper: Checks if a chunk overlaps with the requested range
    fn chunk_overlaps_range(&self, chunk: &RemuxedChunk, start: u64, end: u64) -> bool {
        let chunk_start = chunk.offset;
        let chunk_end = chunk_start + chunk.data.len() as u64;
        chunk_end > start && chunk_start < end
    }

    /// Helper: Extracts data from a chunk that overlaps with the range
    fn extract_range_from_chunk(&self, chunk: &RemuxedChunk, start: u64, end: u64) -> Vec<u8> {
        let chunk_start = chunk.offset;
        let chunk_end = chunk_start + chunk.data.len() as u64;

        // Calculate intersection
        let copy_start = start
            .saturating_sub(chunk_start)
            .min(chunk.data.len() as u64);
        let copy_end = end.saturating_sub(chunk_start).min(chunk.data.len() as u64);

        if copy_start < copy_end {
            let start_idx = copy_start as usize;
            let end_idx = copy_end as usize;
            chunk.data[start_idx..end_idx].to_vec()
        } else {
            Vec::new()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_chunk(offset: u64, size: usize, is_header: bool) -> RemuxedChunk {
        RemuxedChunk {
            offset,
            data: vec![offset as u8; size],
            is_header,
            timestamp: Instant::now(),
        }
    }

    #[tokio::test]
    async fn test_chunk_buffer_creation() {
        let config = BufferConfig::default();
        let buffer = ChunkBuffer::new(config);

        assert!(!buffer.has_headers().await);
        assert_eq!(buffer.buffered_range().await, None);
    }

    #[tokio::test]
    async fn test_header_chunk_caching() {
        let config = BufferConfig::default();
        let buffer = ChunkBuffer::new(config);

        let header_chunk = create_test_chunk(0, 1024, true);
        buffer.write_chunk(header_chunk).await.unwrap();

        assert!(buffer.has_headers().await);
        let header_data = buffer.header_data().await;
        assert_eq!(header_data.len(), 1024);
        assert_eq!(header_data[0], 0);
    }

    #[tokio::test]
    async fn test_regular_chunk_buffering() {
        let config = BufferConfig::default();
        let buffer = ChunkBuffer::new(config);

        let chunk = create_test_chunk(1024, 512, false);
        buffer.write_chunk(chunk).await.unwrap();

        let data = buffer.read_range(1024, 1536).await;
        assert_eq!(data.len(), 512);
        assert_eq!(data[0], 1024 % 256); // offset as u8
    }

    #[tokio::test]
    async fn test_chunk_eviction() {
        let mut config = BufferConfig::default();
        config.max_chunks = 2; // Only allow 2 chunks
        config.max_bytes = 4096; // Plenty of space

        let buffer = ChunkBuffer::new(config);

        // Write 3 chunks
        for i in 0..3 {
            let chunk = create_test_chunk(i * 1000, 1000, false);
            buffer.write_chunk(chunk).await.unwrap();
        }

        // First chunk should be evicted
        let data = buffer.read_range(0, 1000).await;
        assert!(data.is_empty());

        // Later chunks should still be available
        let data = buffer.read_range(1000, 2000).await;
        assert!(!data.is_empty());
    }

    #[tokio::test]
    async fn test_range_read_across_chunks() {
        let config = BufferConfig::default();
        let buffer = ChunkBuffer::new(config);

        // Write adjacent chunks
        let chunk1 = create_test_chunk(0, 1000, false);
        let chunk2 = create_test_chunk(1000, 1000, false);

        buffer.write_chunk(chunk1).await.unwrap();
        buffer.write_chunk(chunk2).await.unwrap();

        // Read across both chunks
        let data = buffer.read_range(500, 1500).await;
        assert_eq!(data.len(), 1000);
    }

    #[tokio::test]
    async fn test_buffered_range() {
        let config = BufferConfig::default();
        let buffer = ChunkBuffer::new(config);

        assert_eq!(buffer.buffered_range().await, None);

        // Add header chunk
        let header = create_test_chunk(0, 100, true);
        buffer.write_chunk(header).await.unwrap();

        // Add regular chunk
        let chunk = create_test_chunk(1000, 500, false);
        buffer.write_chunk(chunk).await.unwrap();

        let range = buffer.buffered_range().await;
        assert_eq!(range, Some((0, 1500)));
    }

    #[tokio::test]
    async fn test_wait_for_range() {
        let config = BufferConfig::low_latency();
        let buffer = Arc::new(ChunkBuffer::new(config));

        let buffer_clone = Arc::clone(&buffer);
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(50)).await;
            let chunk = create_test_chunk(1000, 1000, false);
            buffer_clone.write_chunk(chunk).await.unwrap();
        });

        let available = buffer.wait_for_range(1000, 2000).await;
        assert!(available);
    }

    #[tokio::test]
    async fn test_capacity_exceeded() {
        let mut config = BufferConfig::default();
        config.max_bytes = 1000; // Very small
        config.max_chunks = 100; // Plenty of chunks

        let buffer = ChunkBuffer::new(config);

        // This should succeed
        let chunk1 = create_test_chunk(0, 500, false);
        assert!(buffer.write_chunk(chunk1).await.is_ok());

        // This should fail (would exceed byte limit)
        let chunk2 = create_test_chunk(1000, 600, false);
        assert!(matches!(
            buffer.write_chunk(chunk2).await,
            Err(BufferError::CapacityExceeded)
        ));
    }

    #[tokio::test]
    async fn test_statistics_tracking() {
        let config = BufferConfig::default();
        let buffer = ChunkBuffer::new(config);

        let chunk = create_test_chunk(0, 1024, false);
        buffer.write_chunk(chunk).await.unwrap();

        let stats = buffer.stats().await;
        assert_eq!(stats.chunks_written, 1);
        assert_eq!(stats.bytes_buffered, 1024);

        // Reading should update hit stats
        buffer.read_range(0, 1024).await;

        let stats = buffer.stats().await;
        assert_eq!(stats.range_hits, 1);
    }
}
