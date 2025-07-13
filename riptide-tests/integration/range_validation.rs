//! Integration tests for HTTP range validation
//!
//! Tests the fix for range validation bug where requests beyond file size
//! previously created invalid ranges instead of returning HTTP 416.

use std::collections::HashMap;
use std::ops::Range;
use std::sync::Arc;

use async_trait::async_trait;
use riptide_core::config::RiptideConfig;
use riptide_core::storage::{DataError, DataResult, DataSource, RangeAvailability};
use riptide_core::streaming::{
    Ffmpeg, HttpStreaming, RemuxingOptions, RemuxingResult, StrategyError,
};
use riptide_core::torrent::{InfoHash, TorrentEngineHandle};
use tempfile::TempDir;
use tokio::sync::RwLock;

/// Test data source that serves complete files for range validation testing
#[derive(Clone)]
struct TestDataSource {
    files: Arc<RwLock<HashMap<InfoHash, Vec<u8>>>>,
}

impl TestDataSource {
    fn new() -> Self {
        Self {
            files: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    async fn add_file(&self, info_hash: InfoHash, data: Vec<u8>) {
        let mut files = self.files.write().await;
        files.insert(info_hash, data);
    }
}

#[async_trait]
impl DataSource for TestDataSource {
    async fn read_range(&self, info_hash: InfoHash, range: Range<u64>) -> DataResult<Vec<u8>> {
        let files = self.files.read().await;
        let file_data = files
            .get(&info_hash)
            .ok_or_else(|| DataError::FileNotFound { info_hash })?;

        let start = range.start as usize;
        let end = std::cmp::min(range.end as usize, file_data.len());

        if start >= file_data.len() {
            return Err(DataError::RangeExceedsFile {
                start: range.start,
                end: range.end,
                file_size: file_data.len() as u64,
            });
        }

        Ok(file_data[start..end].to_vec())
    }

    async fn file_size(&self, info_hash: InfoHash) -> DataResult<u64> {
        let files = self.files.read().await;
        files
            .get(&info_hash)
            .map(|data| data.len() as u64)
            .ok_or_else(|| DataError::FileNotFound { info_hash })
    }

    async fn check_range_availability(
        &self,
        info_hash: InfoHash,
        _range: Range<u64>,
    ) -> DataResult<RangeAvailability> {
        let files = self.files.read().await;
        if files.contains_key(&info_hash) {
            Ok(RangeAvailability {
                available: true,
                missing_pieces: vec![],
                cache_hit: true,
            })
        } else {
            Ok(RangeAvailability {
                available: false,
                missing_pieces: vec![],
                cache_hit: false,
            })
        }
    }

    fn source_type(&self) -> &'static str {
        "test_data_source"
    }

    async fn can_handle(&self, info_hash: InfoHash) -> bool {
        let files = self.files.read().await;
        files.contains_key(&info_hash)
    }
}

/// Mock FFmpeg processor for testing
struct MockFfmpeg {
    _temp_dir: TempDir,
}

impl MockFfmpeg {
    fn new() -> Self {
        Self {
            _temp_dir: TempDir::new().expect("Failed to create temp dir"),
        }
    }
}

#[async_trait]
impl Ffmpeg for MockFfmpeg {
    async fn remux_to_mp4(
        &self,
        _input_path: &std::path::Path,
        output_path: &std::path::Path,
        _options: &RemuxingOptions,
    ) -> riptide_core::streaming::StreamingResult<RemuxingResult> {
        // Create a simple MP4 file for testing
        let mp4_data = vec![0u8; 1024]; // Simple test data
        tokio::fs::write(output_path, &mp4_data)
            .await
            .map_err(StrategyError::IoError)?;

        Ok(RemuxingResult {
            output_size: mp4_data.len() as u64,
            processing_time: 0.1,
            streams_reencoded: false,
        })
    }

    fn is_available(&self) -> bool {
        true
    }

    async fn estimate_output_size(&self, input_path: &std::path::Path) -> Option<u64> {
        std::fs::metadata(input_path).ok().map(|m| m.len())
    }
}

fn create_test_mp4_data(size: usize) -> Vec<u8> {
    let mut data = Vec::new();
    // MP4 signature - ftyp box
    data.extend_from_slice(&[0x00, 0x00, 0x00, 0x20]); // box size (32 bytes)
    data.extend_from_slice(b"ftyp"); // box type
    data.extend_from_slice(b"mp42"); // major brand
    data.extend_from_slice(&[0x00, 0x00, 0x00, 0x00]); // minor version
    data.extend_from_slice(b"mp42"); // compatible brand
    data.extend_from_slice(b"isom"); // compatible brand

    // Pad to desired size
    data.resize(size, 0);
    data
}

async fn setup_test_streaming_with_size(file_size: usize) -> (HttpStreaming, InfoHash) {
    let config = RiptideConfig::default();

    // Create a mock torrent engine handle for testing
    let (sender, _receiver) = tokio::sync::mpsc::channel(100);
    let torrent_engine = TorrentEngineHandle::new(sender);

    let data_source = Arc::new(TestDataSource::new());
    let ffmpeg = Arc::new(MockFfmpeg::new());

    let info_hash = InfoHash::new([1u8; 20]);
    let test_data = create_test_mp4_data(file_size);
    data_source.add_file(info_hash, test_data).await;

    let http_streaming = HttpStreaming::new(torrent_engine, data_source, config, ffmpeg);

    (http_streaming, info_hash)
}

#[tokio::test]
async fn test_range_beyond_file_size_returns_416() {
    let (http_streaming, info_hash) = setup_test_streaming_with_size(100).await;

    // Test requesting byte position at file size (should be 416)
    let result = http_streaming
        .serve_http_stream(info_hash, Some("bytes=100-"))
        .await;

    // Should get RangeNotSatisfiable error
    match result {
        Err(StrategyError::RangeNotSatisfiable {
            requested_start,
            file_size,
        }) => {
            assert_eq!(requested_start, 100);
            assert_eq!(file_size, 100);
        }
        other => panic!("Expected RangeNotSatisfiable error, got: {other:?}"),
    }
}

#[tokio::test]
async fn test_range_beyond_file_size_by_large_amount() {
    let (http_streaming, info_hash) = setup_test_streaming_with_size(1000).await;

    // Test requesting way beyond file size
    let result = http_streaming
        .serve_http_stream(info_hash, Some("bytes=89953633-"))
        .await;

    // Should get RangeNotSatisfiable error
    match result {
        Err(StrategyError::RangeNotSatisfiable {
            requested_start,
            file_size,
        }) => {
            assert_eq!(requested_start, 89953633);
            assert_eq!(file_size, 1000);
        }
        other => panic!("Expected RangeNotSatisfiable error, got: {other:?}"),
    }
}

#[tokio::test]
async fn test_valid_range_still_works() {
    let (http_streaming, info_hash) = setup_test_streaming_with_size(100).await;

    // Test requesting valid range within file bounds
    let result = http_streaming
        .serve_http_stream(info_hash, Some("bytes=50-99"))
        .await;

    // Should succeed
    assert!(
        result.is_ok(),
        "Valid range request should succeed: {result:?}"
    );

    let response = result.unwrap();
    assert_eq!(response.body.len(), 50); // Should return 50 bytes
}

#[tokio::test]
async fn test_range_exactly_at_file_size_boundary() {
    let (http_streaming, info_hash) = setup_test_streaming_with_size(100).await;

    // Test requesting from last valid byte (should work)
    let result = http_streaming
        .serve_http_stream(info_hash, Some("bytes=99-"))
        .await;

    // Should succeed and return 1 byte
    assert!(
        result.is_ok(),
        "Range at boundary should succeed: {result:?}"
    );

    let response = result.unwrap();
    assert_eq!(response.body.len(), 1); // Should return 1 byte
}

#[tokio::test]
async fn test_large_open_ended_range_serves_full_content() {
    // This test catches regressions where start position clamping was removed
    // causing large files to be truncated during streaming
    let file_size = 1000000; // 1MB file
    let (http_streaming, info_hash) = setup_test_streaming_with_size(file_size).await;

    // Test requesting from near the middle to the end (open range)
    let start_pos = 600000; // 60% through the file
    let result = http_streaming
        .serve_http_stream(info_hash, Some(&format!("bytes={start_pos}-")))
        .await;

    // Should succeed and return the remaining 400KB
    assert!(
        result.is_ok(),
        "Large open range should succeed: {result:?}"
    );

    let response = result.unwrap();
    let expected_size = file_size - start_pos; // 400000 bytes
    assert_eq!(
        response.body.len(),
        expected_size,
        "Should serve full remaining content from {start_pos} to end of {file_size} byte file"
    );
}

#[tokio::test]
async fn test_start_position_clamping_edge_case() {
    // This test ensures that valid ranges near file boundaries are handled correctly
    // even when start position clamping is involved
    let file_size = 1000;
    let (http_streaming, info_hash) = setup_test_streaming_with_size(file_size).await;

    // Test requesting the last few bytes with exact positioning
    let result = http_streaming
        .serve_http_stream(info_hash, Some("bytes=995-"))
        .await;

    assert!(result.is_ok(), "Range near end should succeed: {result:?}");

    let response = result.unwrap();
    assert_eq!(response.body.len(), 5); // Should return last 5 bytes (995-999)
}

#[tokio::test]
async fn test_full_file_streaming_without_truncation() {
    // This test specifically catches the regression where removing start position clamping
    // caused video streaming to truncate at ~70% completion instead of serving full content
    let file_size = 1000000; // 1MB test file
    let (http_streaming, info_hash) = setup_test_streaming_with_size(file_size).await;

    // Test full file request (no range header)
    let result = http_streaming.serve_http_stream(info_hash, None).await;

    assert!(
        result.is_ok(),
        "Full file streaming should succeed: {result:?}"
    );

    let response = result.unwrap();
    assert_eq!(
        response.body.len(),
        file_size,
        "Full file streaming should return complete file without truncation"
    );

    // Test large open-ended range from beginning
    let result = http_streaming
        .serve_http_stream(info_hash, Some("bytes=0-"))
        .await;

    assert!(
        result.is_ok(),
        "Open range from start should succeed: {result:?}"
    );

    let response = result.unwrap();
    assert_eq!(
        response.body.len(),
        file_size,
        "Open range from start should return complete file without truncation"
    );
}

#[tokio::test]
async fn test_movie_length_streaming_no_truncation() {
    // Test full movie-length file to catch 7-11 minute truncation issues
    // Simulates a 16-minute movie (~100MB at typical bitrates)
    let movie_size = 100 * 1024 * 1024; // 100MB
    let (http_streaming, info_hash) = setup_test_streaming_with_size(movie_size).await;

    // Test sequential range requests throughout the movie
    let chunk_size = 1024 * 1024; // 1MB chunks
    let mut current_pos = 0;

    while current_pos < movie_size {
        let end_pos = std::cmp::min(current_pos + chunk_size - 1, movie_size - 1);
        let range_header = format!("bytes={current_pos}-{end_pos}");

        let result = http_streaming
            .serve_http_stream(info_hash, Some(&range_header))
            .await;

        assert!(
            result.is_ok(),
            "Range request {range_header} should succeed for movie streaming: {result:?}"
        );

        let response = result.unwrap();
        let expected_size = end_pos - current_pos + 1;
        assert_eq!(
            response.body.len(),
            expected_size,
            "Chunk at position {current_pos} should return {expected_size} bytes, got {}",
            response.body.len()
        );

        current_pos += chunk_size;
    }
}

#[tokio::test]
async fn test_large_open_ended_range_at_various_positions() {
    // Test open-ended ranges at positions where truncation typically occurs
    let movie_size = 50 * 1024 * 1024; // 50MB movie
    let (http_streaming, info_hash) = setup_test_streaming_with_size(movie_size).await;

    // Test positions that correspond to typical truncation points
    let test_positions = vec![
        movie_size / 4,       // 25% (4 minutes if 16min movie)
        movie_size * 7 / 16,  // 43.75% (7 minutes if 16min movie)
        movie_size * 11 / 16, // 68.75% (11 minutes if 16min movie)
        movie_size * 3 / 4,   // 75% (12 minutes if 16min movie)
        movie_size - 1024,    // Near end
    ];

    for &start_pos in &test_positions {
        let result = http_streaming
            .serve_http_stream(info_hash, Some(&format!("bytes={start_pos}-")))
            .await;

        assert!(
            result.is_ok(),
            "Open range from position {start_pos} should succeed: {result:?}"
        );

        let response = result.unwrap();
        let expected_size = movie_size - start_pos;
        assert_eq!(
            response.body.len(),
            expected_size,
            "Open range from {start_pos} should return {expected_size} bytes (to end of {movie_size} byte file), got {}",
            response.body.len()
        );
    }
}

#[tokio::test]
async fn test_progressive_range_requests_detect_truncation() {
    // Progressive range requests to detect where truncation occurs
    let movie_size = 20 * 1024 * 1024; // 20MB
    let (http_streaming, info_hash) = setup_test_streaming_with_size(movie_size).await;

    // Request progressively later starting positions
    let step_size = movie_size / 20; // 20 steps through the file

    for i in 0..20 {
        let start_pos = i * step_size;

        // Skip if we're at or beyond the end
        if start_pos >= movie_size {
            continue;
        }

        let result = http_streaming
            .serve_http_stream(info_hash, Some(&format!("bytes={start_pos}-")))
            .await;

        assert!(
            result.is_ok(),
            "Progressive range from position {start_pos} (step {i}/20) should succeed: {result:?}"
        );

        let response = result.unwrap();
        let expected_size = movie_size - start_pos;
        assert_eq!(
            response.body.len(),
            expected_size,
            "Step {i}: Range from {start_pos} should return {expected_size} bytes, got {} (truncation detected!)",
            response.body.len()
        );
    }
}

#[tokio::test]
async fn test_file_size_consistency_through_range_requests() {
    // Test that file size calculations are consistent by testing range requests
    // This catches truncation bugs where strategy reports wrong total_size
    let movie_size = 16 * 1024 * 1024; // 16MB (simulating 16-minute movie)
    let (http_streaming, info_hash) = setup_test_streaming_with_size(movie_size).await;

    // Test a small range request to trigger strategy file size calculation
    let result = http_streaming
        .serve_http_stream(info_hash, Some("bytes=0-1023"))
        .await;

    assert!(result.is_ok(), "Small range request should succeed");

    // The strategy's file size calculation happens internally during serve_http_stream
    // If there's a mismatch, it would show up in the debug logs we added

    // Test an open-ended range from the middle to ensure no truncation
    let start_pos = movie_size / 2; // Start from middle
    let result = http_streaming
        .serve_http_stream(info_hash, Some(&format!("bytes={start_pos}-")))
        .await;

    assert!(
        result.is_ok(),
        "Open range from middle should succeed: {result:?}"
    );

    let response = result.unwrap();
    let expected_size = movie_size - start_pos;
    assert_eq!(
        response.body.len(),
        expected_size,
        "Should serve exactly {} bytes from middle to end, got {} (file size mismatch detected!)",
        expected_size,
        response.body.len()
    );

    // Test multiple points to ensure file size calculation is consistent
    let test_positions = [movie_size / 4, movie_size * 3 / 4, movie_size - 1000];
    for &pos in &test_positions {
        let result = http_streaming
            .serve_http_stream(info_hash, Some(&format!("bytes={pos}-")))
            .await;

        assert!(
            result.is_ok(),
            "Range from position {pos} should succeed: {result:?}"
        );

        let response = result.unwrap();
        let expected = movie_size - pos;
        assert_eq!(
            response.body.len(),
            expected,
            "Position {pos}: expected {expected} bytes, got {} (file size inconsistency)",
            response.body.len()
        );
    }
}
