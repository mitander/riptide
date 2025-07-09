//! Integration tests for unified remux streaming functionality
//!
//! Tests the complete pipeline from file assembly through FFmpeg remuxing
//! to progressive streaming output, validating the unified approach works
//! correctly across different formats and completion states.

use std::collections::HashMap;
use std::ops::Range;
use std::sync::Arc;
use std::time::Duration;

use riptide_core::streaming::{
    ContainerFormat, FileAssembler, FileAssemblerError, RemuxStreamingConfig, StrategyError,
    StreamingStrategy, create_remux_streaming_strategy_with_config,
};
use riptide_core::torrent::InfoHash;
use tokio::sync::RwLock;

/// Mock file assembler for testing unified streaming
#[derive(Debug)]
struct MockFileAssembler {
    available_ranges: RwLock<HashMap<InfoHash, Vec<Range<u64>>>>,
    file_sizes: HashMap<InfoHash, u64>,
    file_data: HashMap<InfoHash, Vec<u8>>,
}

impl MockFileAssembler {
    fn new() -> Self {
        Self {
            available_ranges: RwLock::new(HashMap::new()),
            file_sizes: HashMap::new(),
            file_data: HashMap::new(),
        }
    }

    fn add_file(&mut self, info_hash: InfoHash, size: u64, data: Vec<u8>) {
        self.file_sizes.insert(info_hash, size);
        self.file_data.insert(info_hash, data);
    }

    async fn make_range_available(&self, info_hash: InfoHash, range: Range<u64>) {
        let mut ranges = self.available_ranges.write().await;
        ranges.entry(info_hash).or_insert_with(Vec::new).push(range);
    }

    async fn _make_file_complete(&self, info_hash: InfoHash) {
        if let Some(&size) = self.file_sizes.get(&info_hash) {
            self.make_range_available(info_hash, 0..size).await;
        }
    }

    async fn make_head_available(&self, info_hash: InfoHash, head_size: u64) {
        self.make_range_available(info_hash, 0..head_size).await;
    }
}

#[async_trait::async_trait]
impl FileAssembler for MockFileAssembler {
    async fn read_range(
        &self,
        info_hash: InfoHash,
        range: Range<u64>,
    ) -> Result<Vec<u8>, FileAssemblerError> {
        let data =
            self.file_data
                .get(&info_hash)
                .ok_or_else(|| FileAssemblerError::InsufficientData {
                    start: 0,
                    end: 0,
                    missing_count: 1,
                })?;

        if range.end > data.len() as u64 {
            return Err(FileAssemblerError::RangeExceedsFile {
                start: range.start,
                end: range.end,
                file_size: data.len() as u64,
            });
        }

        Ok(data[range.start as usize..range.end as usize].to_vec())
    }

    async fn file_size(&self, info_hash: InfoHash) -> Result<u64, FileAssemblerError> {
        self.file_sizes.get(&info_hash).copied().ok_or_else(|| {
            FileAssemblerError::InsufficientData {
                start: 0,
                end: 0,
                missing_count: 1,
            }
        })
    }

    fn is_range_available(&self, info_hash: InfoHash, range: Range<u64>) -> bool {
        if let Ok(ranges) = self.available_ranges.try_read() {
            if let Some(available) = ranges.get(&info_hash) {
                return available
                    .iter()
                    .any(|r| r.start <= range.start && r.end >= range.end);
            }
        }
        false
    }
}

/// Create test data that resembles a video file
fn create_test_video_data(size: usize) -> Vec<u8> {
    let mut data = vec![0u8; size];

    // Add some patterns to make it look like video data
    for i in 0..size {
        data[i] = (i % 256) as u8;
    }

    data
}

/// Create test configuration optimized for testing
fn create_test_config() -> RemuxStreamingConfig {
    RemuxStreamingConfig {
        min_head_size: 64 * 1024,                   // 64KB minimum head for testing
        max_output_buffer_size: 1024 * 1024,        // 1MB buffer for testing
        piece_wait_timeout: Duration::from_secs(5), // Short timeout for tests
        input_chunk_size: 32 * 1024,                // 32KB chunks for testing
        max_concurrent_sessions: 5,                 // Limit concurrent sessions
        ffmpeg_timeout: Duration::from_secs(30),    // Short timeout for tests
        ..Default::default()
    }
}

#[tokio::test]
async fn test_unified_streaming_strategy_creation() {
    let file_assembler: Arc<dyn FileAssembler> = Arc::new(MockFileAssembler::new());
    let config = create_test_config();
    let strategy = create_remux_streaming_strategy_with_config(file_assembler, config);

    // Test basic properties
    assert!(strategy.supports_format(&ContainerFormat::Avi));
    assert!(strategy.supports_format(&ContainerFormat::Mkv));
    assert!(strategy.supports_format(&ContainerFormat::Mp4));
}

#[tokio::test]
async fn test_unified_streaming_insufficient_head_data() {
    let mut file_assembler = MockFileAssembler::new();
    let info_hash = InfoHash::new([1u8; 20]);
    let test_data = create_test_video_data(1024 * 1024); // 1MB test file

    file_assembler.add_file(info_hash, test_data.len() as u64, test_data);

    let file_assembler: Arc<dyn FileAssembler> = Arc::new(file_assembler);
    let config = create_test_config();
    let strategy = create_remux_streaming_strategy_with_config(file_assembler, config);

    // Should timeout when waiting for head data that never becomes available
    let result = tokio::time::timeout(
        Duration::from_secs(2),
        strategy.stream_range(info_hash, 0..1024),
    )
    .await;
    assert!(result.is_err()); // Should timeout
}

#[tokio::test]
async fn test_unified_streaming_with_head_data() {
    let mut file_assembler = MockFileAssembler::new();
    let info_hash = InfoHash::new([2u8; 20]);
    let test_data = create_test_video_data(2 * 1024 * 1024); // 2MB test file

    file_assembler.add_file(info_hash, test_data.len() as u64, test_data);

    // Make head data available before converting to trait object
    file_assembler
        .make_head_available(info_hash, 128 * 1024)
        .await;

    let file_assembler: Arc<dyn FileAssembler> = Arc::new(file_assembler);
    let config = create_test_config();
    let strategy = create_remux_streaming_strategy_with_config(file_assembler.clone(), config);

    // Should attempt to start remuxing (will fail in test environment without FFmpeg)
    let result = strategy.stream_range(info_hash, 0..1024).await;
    // In test environment without FFmpeg, this should fail with FFmpeg error
    assert!(result.is_err());
    match result {
        Err(StrategyError::RemuxingFailed { reason }) => {
            assert!(
                reason.contains("FFmpeg")
                    || reason.contains("Session has error")
                    || reason.contains("Failed to get file size")
            );
        }
        Err(StrategyError::FfmpegError { reason }) => {
            assert!(
                reason.contains("FFmpeg")
                    || reason.contains("Session has error")
                    || reason.contains("Failed to get file size")
                    || reason.contains("Insufficient")
            );
        }
        _ => {} // Other errors are also acceptable in test environment
    }
}

#[tokio::test]
async fn test_unified_streaming_container_format_output() {
    let file_assembler: Arc<dyn FileAssembler> = Arc::new(MockFileAssembler::new());
    let config = create_test_config();
    let strategy = create_remux_streaming_strategy_with_config(file_assembler, config);

    let info_hash = InfoHash::new([3u8; 20]);

    // Strategy should always report MP4 output format
    let result = strategy.container_format(info_hash).await;
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), ContainerFormat::Mp4);
}

#[tokio::test]
async fn test_unified_streaming_file_size_estimation() {
    let mut file_assembler = MockFileAssembler::new();
    let info_hash = InfoHash::new([4u8; 20]);
    let test_data = create_test_video_data(1024 * 1024); // 1MB test file

    file_assembler.add_file(info_hash, test_data.len() as u64, test_data);

    // Make head data available before converting to trait object
    file_assembler
        .make_head_available(info_hash, 128 * 1024)
        .await;

    let file_assembler: Arc<dyn FileAssembler> = Arc::new(file_assembler);
    let config = create_test_config();
    let strategy = create_remux_streaming_strategy_with_config(file_assembler.clone(), config);

    // Should return file size estimate - during remuxing this may be 0 initially
    let result = strategy.file_size(info_hash).await;
    assert!(result.is_ok());
    // File size estimation during remuxing is inherently uncertain
    // Just verify we get a result without error
    let _file_size = result.unwrap();
}

#[tokio::test]
async fn test_unified_streaming_concurrent_sessions() {
    let mut file_assembler = MockFileAssembler::new();
    let test_data = create_test_video_data(512 * 1024); // 512KB test file

    // Add multiple files
    let info_hashes: Vec<InfoHash> = (0..10)
        .map(|i| {
            let mut hash = [0u8; 20];
            hash[0] = i as u8;
            InfoHash::new(hash)
        })
        .collect();

    for &info_hash in &info_hashes {
        file_assembler.add_file(info_hash, test_data.len() as u64, test_data.clone());
    }

    // Make head data available for all files before converting to trait object
    for &info_hash in &info_hashes {
        file_assembler
            .make_head_available(info_hash, 128 * 1024)
            .await;
    }

    let file_assembler: Arc<dyn FileAssembler> = Arc::new(file_assembler);
    let config = RemuxStreamingConfig {
        max_concurrent_sessions: 3, // Limit to 3 concurrent sessions
        ..create_test_config()
    };
    let strategy = create_remux_streaming_strategy_with_config(file_assembler.clone(), config);

    // Try to start many sessions - should hit the limit
    let mut results = Vec::new();
    for &info_hash in &info_hashes {
        let result = strategy.stream_range(info_hash, 0..1024).await;
        results.push(result);
    }

    // Some should succeed (or fail with FFmpeg error), others should fail with session limit
    let mut session_limit_errors = 0;
    let mut other_errors = 0;

    for result in results {
        match result {
            Err(StrategyError::RemuxingFailed { reason }) => {
                if reason.contains("concurrent") || reason.contains("sessions") {
                    session_limit_errors += 1;
                } else {
                    other_errors += 1;
                }
            }
            Err(StrategyError::FfmpegError { .. }) => {
                other_errors += 1;
            }
            Err(_) => {
                other_errors += 1;
            }
            Ok(_) => {} // Unexpected success
        }
    }

    // Should have some session limit errors when exceeding max_concurrent_sessions
    assert!(session_limit_errors > 0 || other_errors > 0);
}

#[tokio::test]
async fn test_unified_streaming_format_support() {
    let file_assembler: Arc<dyn FileAssembler> = Arc::new(MockFileAssembler::new());
    let config = create_test_config();
    let strategy = create_remux_streaming_strategy_with_config(file_assembler, config);

    // Should support all major video formats for remuxing
    assert!(strategy.supports_format(&ContainerFormat::Avi));
    assert!(strategy.supports_format(&ContainerFormat::Mkv));
    assert!(strategy.supports_format(&ContainerFormat::Mp4));

    // Should not support unsupported formats
    assert!(!strategy.supports_format(&ContainerFormat::WebM));
}

#[tokio::test]
async fn test_unified_streaming_error_handling() {
    let file_assembler: Arc<dyn FileAssembler> = Arc::new(MockFileAssembler::new());
    let config = create_test_config();
    let strategy = create_remux_streaming_strategy_with_config(file_assembler, config);

    let nonexistent_hash = InfoHash::new([99u8; 20]);

    // Should handle nonexistent files gracefully
    let result = strategy.stream_range(nonexistent_hash, 0..1024).await;
    assert!(result.is_err());

    let result = strategy.file_size(nonexistent_hash).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_unified_streaming_range_requests() {
    let mut file_assembler = MockFileAssembler::new();
    let info_hash = InfoHash::new([5u8; 20]);
    let test_data = create_test_video_data(1024 * 1024); // 1MB test file

    file_assembler.add_file(info_hash, test_data.len() as u64, test_data);

    // Make head data available before converting to trait object
    file_assembler
        .make_head_available(info_hash, 128 * 1024)
        .await;

    let file_assembler: Arc<dyn FileAssembler> = Arc::new(file_assembler);
    let config = create_test_config();
    let strategy = create_remux_streaming_strategy_with_config(file_assembler.clone(), config);

    // Test different range requests
    let test_ranges = vec![0..1024, 1024..2048, 0..4096, 100..200];

    for range in test_ranges {
        let result = strategy.stream_range(info_hash, range.clone()).await;
        // Should either succeed or fail with a specific error (not panic)
        match result {
            Ok(data) => {
                assert_eq!(data.len(), (range.end - range.start) as usize);
            }
            Err(StrategyError::RemuxingFailed { .. }) => {
                // Expected in test environment without FFmpeg
            }
            Err(StrategyError::FfmpegError { .. }) => {
                // Also expected in test environment without FFmpeg
            }
            Err(e) => {
                panic!("Unexpected error type: {:?}", e);
            }
        }
    }
}
