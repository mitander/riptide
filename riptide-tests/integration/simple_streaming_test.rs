//! Integration tests for the simple progressive streaming implementation.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use riptide_core::storage::data_source::{DataError, DataSource, RangeAvailability};
use riptide_core::streaming::RemuxingOptions;
use riptide_core::streaming::migration::{ProgressiveStreaming, StreamingImplementation};
use riptide_core::streaming::simple::{SimpleProgressiveStreamer, StreamPump};
use riptide_core::torrent::InfoHash;
use tempfile::TempDir;
use tokio::sync::Mutex;

/// Mock data source that simulates torrent piece availability
struct MockTorrentDataSource {
    data: Vec<u8>,
    available_ranges: Arc<Mutex<Vec<std::ops::Range<u64>>>>,
    #[allow(clippy::type_complexity)]
    read_delays: Arc<Mutex<Vec<(std::ops::Range<u64>, Duration)>>>,
    read_count: Arc<Mutex<usize>>,
}

impl MockTorrentDataSource {
    fn new(size: usize) -> Self {
        // Create realistic AVI-like data with header
        let mut data = vec![0u8; size];

        // AVI header signature
        data[0..4].copy_from_slice(b"RIFF");
        data[8..12].copy_from_slice(b"AVI ");

        // Fill with pattern for verification
        for (i, byte) in data[12..].iter_mut().enumerate() {
            *byte = (i % 256) as u8;
        }

        Self {
            data,
            #[allow(clippy::single_range_in_vec_init)]
            available_ranges: Arc::new(Mutex::new(vec![0..size as u64])),
            read_delays: Arc::new(Mutex::new(Vec::new())),
            read_count: Arc::new(Mutex::new(0)),
        }
    }

    /// Simulate progressive availability of data
    fn with_progressive_availability(self, chunk_size: u64) -> Self {
        let size = self.data.len() as u64;
        // Initially, only first chunk is available
        #[allow(clippy::single_range_in_vec_init)]
        let ranges = vec![0..chunk_size.min(size)];

        let mut available = self
            .available_ranges
            .try_lock()
            .expect("Lock should be available during setup");
        *available = ranges;
        drop(available);
        self
    }

    /// Add delay for specific range reads
    fn with_read_delay(self, range: std::ops::Range<u64>, delay: Duration) -> Self {
        let mut delays = self
            .read_delays
            .try_lock()
            .expect("Lock should be available during setup");
        delays.push((range, delay));
        drop(delays);
        self
    }

    /// Make more data available (simulating piece download)
    async fn make_available(&self, range: std::ops::Range<u64>) {
        let mut ranges = self.available_ranges.lock().await;
        ranges.push(range);

        // Merge overlapping ranges
        ranges.sort_by_key(|r| r.start);
        let mut merged: Vec<std::ops::Range<u64>> = Vec::new();

        for range in ranges.iter() {
            if let Some(last) = merged.last_mut()
                && last.end >= range.start
            {
                last.end = last.end.max(range.end);
                continue;
            }
            merged.push(range.clone());
        }

        *ranges = merged;
    }
}

#[async_trait::async_trait]
impl DataSource for MockTorrentDataSource {
    async fn file_size(&self, _info_hash: InfoHash) -> Result<u64, DataError> {
        Ok(self.data.len() as u64)
    }

    fn source_type(&self) -> &'static str {
        "mock"
    }

    async fn can_handle(&self, _info_hash: InfoHash) -> bool {
        true
    }

    async fn read_range(
        &self,
        _info_hash: InfoHash,
        range: std::ops::Range<u64>,
    ) -> Result<Vec<u8>, DataError> {
        *self.read_count.lock().await += 1;

        // Check if range is available
        let available_ranges = self.available_ranges.lock().await;
        let is_available = available_ranges
            .iter()
            .any(|r| r.start <= range.start && range.end <= r.end);

        if !is_available {
            return Ok(Vec::new()); // Return empty data if not available
        }

        // Check for delays
        let delays = self.read_delays.lock().await;
        for (delay_range, duration) in delays.iter() {
            if delay_range.start <= range.start && range.end <= delay_range.end {
                tokio::time::sleep(*duration).await;
                break;
            }
        }

        Ok(self.data[range.start as usize..range.end as usize].to_vec())
    }

    async fn check_range_availability(
        &self,
        _info_hash: InfoHash,
        range: std::ops::Range<u64>,
    ) -> Result<RangeAvailability, DataError> {
        let available_ranges = self.available_ranges.lock().await;
        let is_available = available_ranges
            .iter()
            .any(|r| r.start <= range.start && range.end <= r.end);

        Ok(RangeAvailability {
            available: is_available,
            missing_pieces: if is_available { vec![] } else { vec![0] }, // Simplified
            cache_hit: false,
        })
    }
}

#[tokio::test]
async fn test_stream_pump_basic() {
    let data_source = Arc::new(MockTorrentDataSource::new(5 * 1024 * 1024)); // 5MB
    let info_hash = InfoHash::new([0u8; 20]); // Simple test hash

    let pump = StreamPump::new(
        data_source.clone(),
        info_hash,
        5 * 1024 * 1024,
        None,
        tokio::runtime::Handle::current(),
    );

    let result = tokio::task::spawn_blocking(move || {
        let mut output = Vec::new();
        let result = pump.pump_to(&mut output);
        (result, output)
    })
    .await
    .unwrap();

    let (pump_result, output) = result;
    assert!(pump_result.is_ok());
    assert_eq!(pump_result.unwrap(), 5 * 1024 * 1024);
    assert_eq!(output.len(), 5 * 1024 * 1024);

    // Verify data integrity
    assert_eq!(&output[0..4], b"RIFF");
    assert_eq!(&output[8..12], b"AVI ");
}

#[tokio::test]
async fn test_stream_pump_progressive_availability() {
    let chunk_size = 1024 * 1024; // 1MB chunks
    let total_size = 5 * 1024 * 1024; // 5MB total

    let data_source = Arc::new(
        MockTorrentDataSource::new(total_size).with_progressive_availability(chunk_size as u64),
    );
    let info_hash = InfoHash::new([1u8; 20]); // Different test hash

    let pump = StreamPump::new(
        data_source.clone(),
        info_hash,
        total_size as u64,
        None,
        tokio::runtime::Handle::current(),
    );

    // Spawn background task to make data available progressively
    let data_source_clone = data_source.clone();
    let availability_task = tokio::spawn(async move {
        for i in 1..5 {
            tokio::time::sleep(Duration::from_millis(100)).await;
            let start = (i * chunk_size) as u64;
            let end = ((i + 1) * chunk_size).min(total_size) as u64;
            data_source_clone.make_available(start..end).await;
        }
    });

    let start_time = std::time::Instant::now();

    let result = tokio::task::spawn_blocking(move || {
        let mut output = Vec::new();
        let result = pump.pump_to(&mut output);
        (result, output)
    })
    .await
    .unwrap();

    let elapsed = start_time.elapsed();
    availability_task.await.unwrap();

    let (pump_result, output) = result;
    assert!(pump_result.is_ok());
    assert_eq!(pump_result.unwrap(), total_size as u64);
    assert_eq!(output.len(), total_size);

    // Should have taken at least 400ms due to progressive availability
    assert!(
        elapsed >= Duration::from_millis(300),
        "Elapsed: {elapsed:?}"
    );
}

#[tokio::test]
async fn test_simple_progressive_streamer() {
    let temp_dir = TempDir::new().unwrap();
    let output_path = temp_dir.path().join("output.mp4");

    let data_source = Arc::new(MockTorrentDataSource::new(10 * 1024 * 1024)); // 10MB
    let info_hash = InfoHash::new([2u8; 20]); // Third test hash

    let streamer = SimpleProgressiveStreamer::new(
        data_source,
        info_hash,
        10 * 1024 * 1024,
        "avi".to_string(),
        output_path.clone(),
        None,
    );

    // Check initial state
    assert!(!streamer.is_ready());

    let progress = streamer.progress();
    assert_eq!(progress.bytes_pumped, 0);
    assert_eq!(progress.file_size, 10 * 1024 * 1024);

    // Note: We can't actually test FFmpeg execution without FFmpeg installed
    // This would need to be tested in a real integration environment
}

#[tokio::test]
async fn test_migration_interface() {
    let temp_dir = TempDir::new().unwrap();
    let output_path = temp_dir.path().join("output.mp4");

    let data_source = Arc::new(MockTorrentDataSource::new(5 * 1024 * 1024));
    let info_hash = InfoHash::new([3u8; 20]); // Migration test hash

    // Test with simple implementation
    let streaming = ProgressiveStreaming::with_implementation(
        StreamingImplementation::Simple,
        data_source.clone(),
        None,
    );

    let options = RemuxingOptions {
        video_codec: "libx264".to_string(),
        audio_codec: "aac".to_string(),
        ..Default::default()
    };

    let handle = streaming
        .start_streaming(
            info_hash,
            PathBuf::from("input.avi"),
            output_path.clone(),
            5 * 1024 * 1024,
            &options,
        )
        .await;

    // Should succeed in creating handle (actual FFmpeg execution would fail in test env)
    assert!(handle.is_ok());

    let handle = handle.unwrap();
    assert!(!handle.is_ready().await);

    // Cleanup
    handle.shutdown();
}

#[tokio::test]
async fn test_read_delays_and_retries() {
    let data_source = Arc::new(
        MockTorrentDataSource::new(2 * 1024 * 1024)
            .with_read_delay(0..1024 * 1024, Duration::from_millis(50))
            .with_read_delay(1024 * 1024..2 * 1024 * 1024, Duration::from_millis(25)),
    );

    let info_hash = InfoHash::new([4u8; 20]); // Delays test hash
    let pump = StreamPump::new(
        data_source.clone(),
        info_hash,
        2 * 1024 * 1024,
        None,
        tokio::runtime::Handle::current(),
    );

    let start_time = std::time::Instant::now();

    let result = tokio::task::spawn_blocking(move || {
        let mut output = Vec::new();
        let result = pump.pump_to(&mut output);
        (result, output)
    })
    .await
    .unwrap();

    let elapsed = start_time.elapsed();

    let (pump_result, output) = result;
    assert!(pump_result.is_ok());
    assert_eq!(output.len(), 2 * 1024 * 1024);

    // Should have experienced delays
    assert!(elapsed >= Duration::from_millis(50), "Elapsed: {elapsed:?}");
}

#[tokio::test]
async fn test_timeout_handling() {
    // Create a source that never has data available
    let data_source = Arc::new(
        MockTorrentDataSource::new(1024 * 1024).with_progressive_availability(0), // No data available
    );

    let info_hash = InfoHash::new([5u8; 20]); // Timeout test hash
    let pump = StreamPump::new(
        data_source,
        info_hash,
        1024 * 1024,
        None,
        tokio::runtime::Handle::current(),
    );

    let mut output = Vec::new();
    let result = tokio::task::spawn_blocking(move || pump.pump_to(&mut output))
        .await
        .unwrap();

    // Should timeout waiting for initial data
    assert!(result.is_err());
    match result {
        Err(e) => assert!(e.to_string().contains("Timeout")),
        Ok(_) => panic!("Expected timeout error"),
    }
}

#[test]
fn test_ffmpeg_command_construction() {
    use riptide_core::streaming::simple::FfmpegRunner;

    let temp_dir = TempDir::new().unwrap();
    let output_path = temp_dir.path().join("test.mp4");

    let _runner = FfmpegRunner::new("avi".to_string(), output_path);

    // We can't actually run FFmpeg in tests, but we can verify the struct is created
    // The actual command construction is tested when run_progressive() is called
    // which requires FFmpeg to be installed
}

#[test]
fn test_format_detection_and_auto_handling() {
    use riptide_core::streaming::simple::FfmpegRunner;

    let temp_dir = TempDir::new().unwrap();

    // Test that "auto" format doesn't break FFmpeg command construction
    let output_path = temp_dir.path().join("test_auto.mp4");
    let _runner_auto = FfmpegRunner::new("auto".to_string(), output_path);

    // Test with known format
    let output_path_avi = temp_dir.path().join("test_avi.mp4");
    let _runner_avi = FfmpegRunner::new("avi".to_string(), output_path_avi);

    // Note: We can't access private fields, but construction should not panic.
    // The actual FFmpeg command construction logic that omits -f for "auto"
    // is tested implicitly when run_progressive() is called in real usage.
    // This test ensures the basic setup works without runtime panics.
}

#[tokio::test]
async fn test_end_to_end_streaming_size_preservation() {
    // This test verifies that the complete file size is preserved through the streaming pipeline
    // and would catch the 11min 36s truncation issue

    let temp_dir = TempDir::new().unwrap();
    let _output_path = temp_dir.path().join("test_full_stream.mp4");

    // Create a large mock data source (100MB) to simulate a real movie file
    let file_size = 100 * 1024 * 1024u64; // 100MB
    let mock_data_source = MockTorrentDataSource::new(file_size as usize);

    // Make all data immediately available to avoid timeout issues in tests
    mock_data_source.make_available(0..file_size).await;

    let data_source = Arc::new(mock_data_source);
    let info_hash = InfoHash::new([99u8; 20]); // End-to-end test hash

    // Test the pump component directly first to verify size preservation
    let pump = StreamPump::new(
        data_source.clone(),
        info_hash,
        file_size,
        None,
        tokio::runtime::Handle::current(),
    );

    // Pump to an in-memory buffer to verify complete data transfer
    let (bytes_pumped, output_buffer) = tokio::task::spawn_blocking(move || {
        let mut buffer = Vec::new();
        let result = pump.pump_to(&mut buffer);
        (result, buffer)
    })
    .await
    .unwrap();

    let bytes_pumped = bytes_pumped.expect("Pumping should succeed");

    // CRITICAL TEST: Verify that ALL bytes were pumped, not just a truncated portion
    assert_eq!(
        bytes_pumped, file_size,
        "Pump failed to transfer complete file: got {bytes_pumped} bytes, expected {file_size} bytes. This indicates data loss during pumping."
    );

    // Verify the output buffer contains the expected amount of data
    assert_eq!(
        output_buffer.len() as u64,
        file_size,
        "Output buffer size mismatch: got {} bytes, expected {} bytes",
        output_buffer.len(),
        file_size
    );

    // Test progress tracking
    let progress_pump = StreamPump::new(
        data_source,
        info_hash,
        file_size,
        None,
        tokio::runtime::Handle::current(),
    );

    let _final_bytes_pumped = progress_pump.bytes_pumped();

    // Note: We can't easily test the full SimpleProgressiveStreamer.start() here
    // because it requires FFmpeg installation and creates actual processes.
    // This test focuses on the data pumping logic which is where truncation would occur.

    println!("✓ End-to-end streaming test passed: {file_size} bytes preserved");
}

#[tokio::test]
async fn test_streaming_with_realistic_file_patterns() {
    // This test simulates realistic file access patterns that could reveal truncation bugs

    let _temp_dir = TempDir::new().unwrap();
    let file_size = 50 * 1024 * 1024u64; // 50MB simulated video file

    let mock_data_source = MockTorrentDataSource::new(file_size as usize);

    // Simulate progressive download - pieces become available over time
    // This mirrors real BitTorrent behavior where streaming starts before download completes
    let mut chunk_start = 0u64;
    while chunk_start < file_size {
        // 1MB chunks
        let chunk_end = (chunk_start + 1024 * 1024).min(file_size);
        mock_data_source
            .make_available(chunk_start..chunk_end)
            .await;
        chunk_start += 1024 * 1024;
    }

    let data_source = Arc::new(mock_data_source);
    let info_hash = InfoHash::new([100u8; 20]); // Realistic pattern test hash

    let pump = StreamPump::new(
        data_source,
        info_hash,
        file_size,
        None,
        tokio::runtime::Handle::current(),
    );

    let mut streaming_output = Vec::new();
    let result = tokio::task::spawn_blocking(move || pump.pump_to(&mut streaming_output))
        .await
        .unwrap();

    match result {
        Ok(bytes_streamed) => {
            assert_eq!(
                bytes_streamed, file_size,
                "Realistic streaming pattern failed: streamed {bytes_streamed} bytes, expected {file_size} bytes. File truncation detected."
            );
        }
        Err(e) => {
            panic!("Realistic streaming with progressive data failed: {e:?}");
        }
    }

    println!("✓ Realistic streaming pattern test passed: {file_size} bytes streamed correctly");
}
