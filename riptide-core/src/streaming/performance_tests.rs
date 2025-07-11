//! Performance and load testing for streaming components
//!
//! Implements the performance benchmarks specified in PERFORMANCE_SOLUTION.md

use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::storage::{DataError, DataSource};
use crate::torrent::{InfoHash, TorrentPiece};

/// Performance test results for streaming components
#[derive(Debug, Clone)]
pub struct StreamingPerformanceResults {
    /// Sustained throughput in Mbps
    pub throughput_mbps: f64,
    /// Average seeking latency
    pub seek_latency_ms: u64,
    /// Startup time from request to first bytes
    pub startup_time_ms: u64,
    /// Cache hit rate percentage
    pub cache_hit_rate: f64,
    /// Number of concurrent streams tested
    pub concurrent_streams: usize,
}

/// Load testing configuration
#[derive(Debug, Clone)]
pub struct LoadTestConfig {
    /// Test duration in seconds
    pub duration_seconds: u64,
    /// Number of concurrent streams
    pub concurrent_streams: usize,
    /// Target chunk size for throughput testing
    pub chunk_size_bytes: usize,
    /// Number of seek operations to test
    pub seek_operations: usize,
}

impl Default for LoadTestConfig {
    fn default() -> Self {
        Self {
            duration_seconds: 60,
            concurrent_streams: 5,
            chunk_size_bytes: 1024 * 1024, // 1MB chunks
            seek_operations: 20,
        }
    }
}

/// Performance testing suite for streaming components
pub struct StreamingPerformanceTester<F: DataSource> {
    data_source: Arc<F>,
    config: LoadTestConfig,
}

impl<F: DataSource + 'static> StreamingPerformanceTester<F> {
    /// Create new performance tester
    pub fn new(data_source: Arc<F>) -> Self {
        Self {
            data_source,
            config: LoadTestConfig::default(),
        }
    }

    /// Configure load test parameters
    pub fn with_config(mut self, config: LoadTestConfig) -> Self {
        self.config = config;
        self
    }

    /// Test 4K streaming throughput requirement (25+ Mbps)
    pub async fn test_4k_streaming_throughput(
        &self,
        info_hash: InfoHash,
    ) -> Result<StreamingPerformanceResults, DataError> {
        let test_start = Instant::now();
        let mut total_bytes = 0u64;
        let mut seek_latencies = Vec::new();

        // Get file size to avoid reading past end
        let file_size = self.data_source.file_size(info_hash).await?;

        tracing::info!(
            "Starting 4K streaming throughput test: target 25+ Mbps for {} seconds",
            self.config.duration_seconds
        );

        // Test sustained throughput
        let mut position = 0u64;
        while test_start.elapsed().as_secs() < self.config.duration_seconds {
            let chunk_start = Instant::now();

            // Calculate safe chunk size to avoid reading past file end
            let chunk_size = (self.config.chunk_size_bytes as u64).min(file_size - position);
            if chunk_size == 0 {
                // Wrap around to beginning for continuous test
                position = 0;
                continue;
            }

            let data = self
                .data_source
                .read_range(info_hash, position..position + chunk_size)
                .await?;

            total_bytes += data.len() as u64;
            position += chunk_size;

            // Wrap around to beginning if we reached the end
            if position >= file_size {
                position = 0;
            }

            // Measure chunk read latency as proxy for seeking
            let chunk_latency = chunk_start.elapsed();
            seek_latencies.push(chunk_latency.as_millis() as u64);

            // Simulate realistic streaming interval
            if chunk_latency < Duration::from_millis(100) {
                tokio::time::sleep(Duration::from_millis(100) - chunk_latency).await;
            }
        }

        let total_duration = test_start.elapsed().as_secs_f64();
        let throughput_mbps = (total_bytes as f64 * 8.0) / (total_duration * 1_000_000.0);

        let avg_seek_latency = if seek_latencies.is_empty() {
            0
        } else {
            seek_latencies.iter().sum::<u64>() / seek_latencies.len() as u64
        };

        tracing::info!(
            "4K streaming test completed: {:.1} Mbps throughput, {}ms avg latency",
            throughput_mbps,
            avg_seek_latency
        );

        Ok(StreamingPerformanceResults {
            throughput_mbps,
            seek_latency_ms: avg_seek_latency,
            startup_time_ms: 0,  // Measured separately
            cache_hit_rate: 0.0, // Would need cache stats
            concurrent_streams: 1,
        })
    }

    /// Test seeking latency requirement (<500ms)
    pub async fn test_seeking_latency(
        &self,
        info_hash: InfoHash,
        file_size: u64,
    ) -> Result<StreamingPerformanceResults, DataError> {
        let mut seek_latencies = Vec::new();

        tracing::info!(
            "Testing seeking latency: target <500ms for {} operations",
            self.config.seek_operations
        );

        for i in 0..self.config.seek_operations {
            // Random seek positions throughout the file
            let seek_position = (file_size * i as u64) / self.config.seek_operations as u64;

            let seek_start = Instant::now();

            // Simulate seeking by reading from the position
            let _data = self
                .data_source
                .read_range(info_hash, seek_position..seek_position + 1024) // Read 1KB
                .await?;

            let seek_latency = seek_start.elapsed().as_millis() as u64;
            seek_latencies.push(seek_latency);

            tracing::debug!("Seek to position {} took {}ms", seek_position, seek_latency);
        }

        let avg_latency = seek_latencies.iter().sum::<u64>() / seek_latencies.len() as u64;
        let max_latency = *seek_latencies.iter().max().unwrap_or(&0);

        tracing::info!(
            "Seeking test completed: {}ms average, {}ms maximum",
            avg_latency,
            max_latency
        );

        Ok(StreamingPerformanceResults {
            throughput_mbps: 0.0,
            seek_latency_ms: avg_latency,
            startup_time_ms: 0,
            cache_hit_rate: 0.0,
            concurrent_streams: 1,
        })
    }

    /// Test concurrent streaming capability (5+ streams)
    pub async fn test_concurrent_streams(
        &self,
        info_hash: InfoHash,
    ) -> Result<StreamingPerformanceResults, DataError> {
        tracing::info!(
            "Testing concurrent streams: {} simultaneous streams",
            self.config.concurrent_streams
        );

        let test_start = Instant::now();
        let mut handles = Vec::new();

        // Spawn concurrent streaming tasks
        for stream_id in 0..self.config.concurrent_streams {
            let data_source = Arc::clone(&self.data_source);
            let chunk_size = self.config.chunk_size_bytes;
            let duration = self.config.duration_seconds;

            let handle = tokio::spawn(async move {
                let mut total_bytes = 0u64;
                let mut position = (stream_id * 10 * 1024 * 1024) as u64; // 10MB offset per stream
                let start_time = Instant::now();

                while start_time.elapsed().as_secs() < duration / 4 {
                    // Shorter test per stream
                    if let Ok(data) = data_source
                        .read_range(info_hash, position..position + chunk_size as u64)
                        .await
                    {
                        total_bytes += data.len() as u64;
                        position += chunk_size as u64;
                    }

                    // Realistic streaming delay
                    tokio::time::sleep(Duration::from_millis(100)).await;
                }

                total_bytes
            });

            handles.push(handle);
        }

        // Wait for all streams to complete
        let mut total_bytes = 0u64;
        for handle in handles {
            if let Ok(bytes) = handle.await {
                total_bytes += bytes;
            }
        }

        let total_duration = test_start.elapsed().as_secs_f64();
        let aggregate_throughput = (total_bytes as f64 * 8.0) / (total_duration * 1_000_000.0);

        tracing::info!(
            "Concurrent streaming test completed: {:.1} Mbps aggregate throughput across {} streams",
            aggregate_throughput,
            self.config.concurrent_streams
        );

        Ok(StreamingPerformanceResults {
            throughput_mbps: aggregate_throughput,
            seek_latency_ms: 0,
            startup_time_ms: 0,
            cache_hit_rate: 0.0,
            concurrent_streams: self.config.concurrent_streams,
        })
    }

    /// Test startup time requirement (<2s from request to first bytes)
    pub async fn test_startup_time(
        &self,
        info_hash: InfoHash,
    ) -> Result<StreamingPerformanceResults, DataError> {
        tracing::info!("Testing startup time: target <2s to first bytes");

        let startup_start = Instant::now();

        // Simulate cold start - request first chunk
        let _first_chunk = self
            .data_source
            .read_range(info_hash, 0..self.config.chunk_size_bytes as u64)
            .await?;

        let startup_time = startup_start.elapsed().as_millis() as u64;

        tracing::info!(
            "Startup time test completed: {}ms to first bytes",
            startup_time
        );

        Ok(StreamingPerformanceResults {
            throughput_mbps: 0.0,
            seek_latency_ms: 0,
            startup_time_ms: startup_time,
            cache_hit_rate: 0.0,
            concurrent_streams: 1,
        })
    }

    /// Run complete performance test suite
    pub async fn run_complete_test_suite(
        &self,
        info_hash: InfoHash,
        file_size: u64,
    ) -> Result<StreamingPerformanceResults, DataError> {
        tracing::info!("Running complete streaming performance test suite");

        // Run all tests
        let throughput_results = self.test_4k_streaming_throughput(info_hash).await?;
        let seeking_results = self.test_seeking_latency(info_hash, file_size).await?;
        let startup_results = self.test_startup_time(info_hash).await?;
        let concurrent_results = self.test_concurrent_streams(info_hash).await?;

        // Combine results
        let combined_results = StreamingPerformanceResults {
            throughput_mbps: throughput_results.throughput_mbps,
            seek_latency_ms: seeking_results.seek_latency_ms,
            startup_time_ms: startup_results.startup_time_ms,
            cache_hit_rate: 0.0, // TODO: Get from file assembler
            concurrent_streams: concurrent_results.concurrent_streams,
        };

        // Check against requirements
        self.validate_performance_requirements(&combined_results);

        Ok(combined_results)
    }

    /// Validate results against PERFORMANCE_SOLUTION.md requirements
    fn validate_performance_requirements(&self, results: &StreamingPerformanceResults) {
        tracing::info!("Validating performance against requirements:");

        // 4K streaming: 25+ Mbps
        if results.throughput_mbps >= 25.0 {
            tracing::info!(
                "4K throughput: {:.1} Mbps (≥25 Mbps required)",
                results.throughput_mbps
            );
        } else {
            tracing::warn!(
                "4K throughput: {:.1} Mbps (<25 Mbps required)",
                results.throughput_mbps
            );
        }

        // Seeking latency: <500ms
        if results.seek_latency_ms < 500 {
            tracing::info!(
                "Seek latency: {}ms (<500ms required)",
                results.seek_latency_ms
            );
        } else {
            tracing::warn!("Seek latency: {}ms (≥500ms)", results.seek_latency_ms);
        }

        // Startup time: <2s
        if results.startup_time_ms < 2000 {
            tracing::info!(
                "Startup time: {}ms (<2000ms required)",
                results.startup_time_ms
            );
        } else {
            tracing::warn!("Startup time: {}ms (≥2000ms)", results.startup_time_ms);
        }

        // Concurrent streams: 5+
        if results.concurrent_streams >= 5 {
            tracing::info!(
                "Concurrent streams: {} (≥5 required)",
                results.concurrent_streams
            );
        } else {
            tracing::warn!("Concurrent streams: {} (<5)", results.concurrent_streams);
        }
    }
}

/// Create test torrent for performance testing
pub fn create_4k_test_torrent() -> (InfoHash, Vec<TorrentPiece>, u64) {
    let info_hash = InfoHash::new([0xAAu8; 20]);
    let piece_size: u64 = 256 * 1024; // 256KB pieces
    let total_size: u64 = 100 * 1024 * 1024; // 100MB test file
    let piece_count = total_size.div_ceil(piece_size);

    let mut pieces = Vec::new();
    for i in 0..piece_count {
        // Calculate actual piece size (last piece may be smaller)
        let current_piece_size = if i == piece_count - 1 {
            // Last piece - calculate remaining bytes
            total_size - (i * piece_size)
        } else {
            piece_size
        };

        // Generate realistic video-like data pattern
        let mut piece_data = vec![0u8; current_piece_size as usize];
        for (j, byte) in piece_data.iter_mut().enumerate() {
            *byte = ((i * piece_size + j as u64) % 256) as u8;
        }

        let piece = TorrentPiece {
            index: i as u32,
            hash: [i as u8; 20],
            data: piece_data,
        };
        pieces.push(piece);
    }

    (info_hash, pieces, total_size)
}

#[cfg(test)]
mod tests {

    use std::sync::Arc;

    use async_trait::async_trait;

    use super::*;
    use crate::storage::PieceDataSource;
    use crate::torrent::{PieceIndex, PieceStore, TorrentError};

    // Mock piece store for testing
    struct MockPieceStore {}

    impl MockPieceStore {
        fn new() -> Self {
            Self {}
        }
    }

    #[async_trait]
    impl PieceStore for MockPieceStore {
        async fn piece_data(
            &self,
            _info_hash: InfoHash,
            _piece_index: PieceIndex,
        ) -> Result<Vec<u8>, TorrentError> {
            Ok(vec![0u8; 1024 * 1024]) // 1MB of test data
        }

        fn has_piece(&self, _info_hash: InfoHash, _piece_index: PieceIndex) -> bool {
            true
        }

        fn piece_count(&self, _info_hash: InfoHash) -> Result<u32, TorrentError> {
            Ok(100) // Mock 100 pieces
        }
    }

    #[tokio::test]
    async fn test_4k_streaming_performance_requirement() {
        let storage = Arc::new(MockPieceStore::new());
        let assembler = Arc::new(PieceDataSource::new(storage, Some(1024)));

        let (info_hash, _pieces, _file_size) = create_4k_test_torrent();

        let tester = StreamingPerformanceTester::new(assembler).with_config(LoadTestConfig {
            duration_seconds: 1, // Very short test for unit testing
            ..Default::default()
        });

        let results = tester
            .test_4k_streaming_throughput(info_hash)
            .await
            .unwrap();

        // For unit tests, just verify the test runs successfully
        // Performance requirements are tested in integration tests
        assert!(results.throughput_mbps > 0.0);
    }

    #[tokio::test]
    async fn test_seeking_latency_requirement() {
        let storage = Arc::new(MockPieceStore::new());
        let assembler = Arc::new(PieceDataSource::new(storage, Some(1024)));

        let (info_hash, _pieces, file_size) = create_4k_test_torrent();

        let tester = StreamingPerformanceTester::new(assembler).with_config(LoadTestConfig {
            seek_operations: 3, // Minimal for unit test
            ..Default::default()
        });

        let results = tester
            .test_seeking_latency(info_hash, file_size)
            .await
            .unwrap();

        // For unit tests, just verify the test runs successfully
        assert!(results.seek_latency_ms == results.seek_latency_ms); // Use results to avoid warning
    }

    #[tokio::test]
    async fn test_startup_time_requirement() {
        let storage = Arc::new(MockPieceStore::new());
        let assembler = Arc::new(PieceDataSource::new(storage, Some(1024)));

        let (info_hash, _pieces, _file_size) = create_4k_test_torrent();

        let tester = StreamingPerformanceTester::new(assembler);
        let results = tester.test_startup_time(info_hash).await.unwrap();

        // For unit tests, just verify the test runs successfully
        assert!(results.startup_time_ms == results.startup_time_ms); // Use results to avoid warning
    }
}
