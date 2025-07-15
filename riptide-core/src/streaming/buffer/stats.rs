//! Statistics tracking for chunk buffer operations

use std::time::{Duration, Instant};

/// Statistics for chunk buffer usage and performance
#[derive(Debug, Clone, Default)]
pub struct BufferStats {
    /// Total number of chunks written to buffer
    pub chunks_written: u64,

    /// Total number of chunks served from buffer
    pub chunks_served: u64,

    /// Total number of chunks evicted due to capacity limits
    pub chunks_evicted: u64,

    /// Current number of chunks in buffer
    pub chunks_buffered: usize,

    /// Total bytes currently buffered
    pub bytes_buffered: u64,

    /// Maximum bytes that were buffered at any point
    pub peak_bytes_buffered: u64,

    /// Number of successful range requests
    pub range_hits: u64,

    /// Number of failed range requests (data not available)
    pub range_misses: u64,

    /// Timestamp when first chunk was written
    pub first_chunk_time: Option<Instant>,

    /// Timestamp when last chunk was written
    pub last_chunk_time: Option<Instant>,

    /// Total time buffer has been active
    pub buffer_age: Duration,

    /// Number of header chunks cached
    pub header_chunks: u32,

    /// Average chunk size in bytes
    pub avg_chunk_size: f64,
}

impl BufferStats {
    /// Creates new empty statistics
    pub fn new() -> Self {
        Self::default()
    }

    /// Calculates buffer utilization as percentage (0-100)
    ///
    /// Returns the percentage of maximum buffer capacity currently in use.
    pub fn utilization_percent(&self, max_bytes: u64) -> f64 {
        if max_bytes == 0 {
            return 0.0;
        }
        (self.bytes_buffered as f64 / max_bytes as f64) * 100.0
    }

    /// Calculates hit ratio for range requests (0-1)
    ///
    /// Returns the percentage of range requests that were successfully served
    /// from the buffer without waiting for new data.
    pub fn hit_ratio(&self) -> f64 {
        let total_requests = self.range_hits + self.range_misses;
        if total_requests == 0 {
            return 0.0;
        }
        self.range_hits as f64 / total_requests as f64
    }

    /// Calculates eviction rate as chunks per second
    ///
    /// Returns the rate at which chunks are being evicted from the buffer.
    pub fn eviction_rate(&self) -> f64 {
        if self.buffer_age.is_zero() {
            return 0.0;
        }
        self.chunks_evicted as f64 / self.buffer_age.as_secs_f64()
    }

    /// Calculates throughput as chunks per second
    ///
    /// Returns the rate at which chunks are being written to the buffer.
    pub fn write_throughput(&self) -> f64 {
        if self.buffer_age.is_zero() {
            return 0.0;
        }
        self.chunks_written as f64 / self.buffer_age.as_secs_f64()
    }

    /// Calculates data rate in bytes per second
    ///
    /// Returns the rate at which data is flowing through the buffer.
    pub fn data_rate_bps(&self) -> f64 {
        if self.buffer_age.is_zero() {
            return 0.0;
        }
        (self.chunks_written as f64 * self.avg_chunk_size) / self.buffer_age.as_secs_f64()
    }

    /// Checks if buffer is healthy based on various metrics
    ///
    /// Returns true if the buffer appears to be operating normally.
    /// Considers hit ratio, eviction rate, and utilization.
    pub fn is_healthy(&self, max_bytes: u64) -> bool {
        // Good hit ratio (>70%)
        let hit_ratio_good = self.hit_ratio() > 0.7;

        // Reasonable utilization (10-90%)
        let utilization = self.utilization_percent(max_bytes);
        let utilization_good = utilization > 10.0 && utilization < 90.0;

        // Low eviction rate (<1 per second)
        let eviction_rate_good = self.eviction_rate() < 1.0;

        hit_ratio_good && utilization_good && eviction_rate_good
    }

    /// Updates statistics when a chunk is written
    pub fn record_chunk_written(&mut self, chunk_size: u64, is_header: bool) {
        self.chunks_written += 1;
        self.bytes_buffered += chunk_size;
        self.peak_bytes_buffered = self.peak_bytes_buffered.max(self.bytes_buffered);

        if is_header {
            self.header_chunks += 1;
        }

        // Update average chunk size
        self.avg_chunk_size = (self.avg_chunk_size * (self.chunks_written - 1) as f64
            + chunk_size as f64)
            / self.chunks_written as f64;

        let now = Instant::now();
        if self.first_chunk_time.is_none() {
            self.first_chunk_time = Some(now);
        }
        self.last_chunk_time = Some(now);

        self.update_buffer_age();
    }

    /// Updates statistics when a chunk is served
    pub fn record_chunk_served(&mut self) {
        self.chunks_served += 1;
        self.range_hits += 1;
    }

    /// Updates statistics when a chunk is evicted
    pub fn record_chunk_evicted(&mut self, chunk_size: u64) {
        self.chunks_evicted += 1;
        self.bytes_buffered = self.bytes_buffered.saturating_sub(chunk_size);
        self.chunks_buffered = self.chunks_buffered.saturating_sub(1);
    }

    /// Updates statistics when a range request misses
    pub fn record_range_miss(&mut self) {
        self.range_misses += 1;
    }

    /// Updates buffer age based on first chunk time
    fn update_buffer_age(&mut self) {
        if let Some(first_time) = self.first_chunk_time {
            self.buffer_age = first_time.elapsed();
        }
    }

    /// Formats statistics as human-readable string
    pub fn format_summary(&self, max_bytes: u64) -> String {
        format!(
            "Buffer Stats: {}/{} chunks ({:.1}%), {:.1}MB/{:.1}MB ({:.1}%), hit ratio: {:.1}%, eviction rate: {:.2}/s",
            self.chunks_buffered,
            self.chunks_written,
            if self.chunks_written > 0 {
                (self.chunks_buffered as f64 / self.chunks_written as f64) * 100.0
            } else {
                0.0
            },
            self.bytes_buffered as f64 / 1024.0 / 1024.0,
            max_bytes as f64 / 1024.0 / 1024.0,
            self.utilization_percent(max_bytes),
            self.hit_ratio() * 100.0,
            self.eviction_rate()
        )
    }
}

#[cfg(test)]
mod tests {
    use std::thread;

    use super::*;

    #[test]
    fn test_buffer_stats_new() {
        let stats = BufferStats::new();
        assert_eq!(stats.chunks_written, 0);
        assert_eq!(stats.chunks_served, 0);
        assert_eq!(stats.bytes_buffered, 0);
        assert!(stats.first_chunk_time.is_none());
    }

    #[test]
    fn test_record_chunk_written() {
        let mut stats = BufferStats::new();

        stats.record_chunk_written(1024, false);

        assert_eq!(stats.chunks_written, 1);
        assert_eq!(stats.bytes_buffered, 1024);
        assert_eq!(stats.avg_chunk_size, 1024.0);
        assert!(stats.first_chunk_time.is_some());
        assert!(stats.last_chunk_time.is_some());
    }

    #[test]
    fn test_record_header_chunk() {
        let mut stats = BufferStats::new();

        stats.record_chunk_written(2048, true);

        assert_eq!(stats.header_chunks, 1);
        assert_eq!(stats.chunks_written, 1);
    }

    #[test]
    fn test_utilization_percent() {
        let mut stats = BufferStats::new();
        stats.bytes_buffered = 5000;

        let utilization = stats.utilization_percent(10000);
        assert_eq!(utilization, 50.0);

        let utilization_zero_max = stats.utilization_percent(0);
        assert_eq!(utilization_zero_max, 0.0);
    }

    #[test]
    fn test_hit_ratio() {
        let mut stats = BufferStats::new();

        // No requests yet
        assert_eq!(stats.hit_ratio(), 0.0);

        // Add some hits and misses
        stats.range_hits = 8;
        stats.range_misses = 2;

        assert_eq!(stats.hit_ratio(), 0.8);
    }

    #[test]
    fn test_average_chunk_size_calculation() {
        let mut stats = BufferStats::new();

        stats.record_chunk_written(1000, false);
        assert_eq!(stats.avg_chunk_size, 1000.0);

        stats.record_chunk_written(2000, false);
        assert_eq!(stats.avg_chunk_size, 1500.0);

        stats.record_chunk_written(3000, false);
        assert_eq!(stats.avg_chunk_size, 2000.0);
    }

    #[test]
    fn test_record_chunk_evicted() {
        let mut stats = BufferStats::new();
        stats.bytes_buffered = 1024;
        stats.chunks_buffered = 1;

        stats.record_chunk_evicted(512);

        assert_eq!(stats.chunks_evicted, 1);
        assert_eq!(stats.bytes_buffered, 512);
        assert_eq!(stats.chunks_buffered, 0);
    }

    #[test]
    fn test_is_healthy() {
        let mut stats = BufferStats::new();

        // Set up healthy stats
        stats.range_hits = 80;
        stats.range_misses = 20;
        stats.bytes_buffered = 5000; // 50% of 10000

        assert!(stats.is_healthy(10000));

        // Test unhealthy - low hit ratio
        stats.range_hits = 50;
        stats.range_misses = 50;
        assert!(!stats.is_healthy(10000));
    }

    #[test]
    fn test_buffer_age_tracking() {
        let mut stats = BufferStats::new();

        stats.record_chunk_written(1024, false);
        thread::sleep(std::time::Duration::from_millis(10));
        stats.record_chunk_written(1024, false);

        assert!(stats.buffer_age > Duration::from_millis(9));
    }

    #[test]
    fn test_data_rate_calculation() {
        let mut stats = BufferStats::new();
        stats.chunks_written = 10;
        stats.avg_chunk_size = 1024.0;
        stats.buffer_age = Duration::from_secs(1);

        let rate = stats.data_rate_bps();
        assert_eq!(rate, 10240.0); // 10 chunks * 1024 bytes / 1 second
    }

    #[test]
    fn test_format_summary() {
        let mut stats = BufferStats::new();
        stats.chunks_written = 100;
        stats.chunks_buffered = 50;
        stats.bytes_buffered = 1024 * 1024; // 1MB
        stats.range_hits = 80;
        stats.range_misses = 20;

        let summary = stats.format_summary(10 * 1024 * 1024); // 10MB max
        assert!(summary.contains("50/100 chunks"));
        assert!(summary.contains("1.0MB/10.0MB"));
        assert!(summary.contains("80.0%")); // hit ratio
    }
}
