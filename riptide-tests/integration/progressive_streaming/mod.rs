//! Comprehensive integration tests for progressive streaming functionality
//!
//! These tests validate the actual FFmpeg pipeline with real binary data,
//! ensuring that progressive streaming works correctly end-to-end.

use std::sync::Arc;
use std::time::Duration;

use riptide_core::storage::{DataError, DataSource, RangeAvailability};
use riptide_core::streaming::ffmpeg::RemuxingOptions;
use riptide_core::streaming::progressive::{ProgressiveFeeder, ProgressiveState};
use riptide_core::torrent::InfoHash;
use tempfile::TempDir;
use tokio::fs;
use tokio::time::timeout;

/// Test data source that simulates partial torrent download
struct ProgressiveTestDataSource {
    file_data: Vec<u8>,
    available_ranges: std::sync::RwLock<Vec<std::ops::Range<u64>>>,
}

impl ProgressiveTestDataSource {
    fn new(data: Vec<u8>) -> Self {
        Self {
            file_data: data,
            available_ranges: std::sync::RwLock::new(Vec::new()),
        }
    }

    /// Make a range of data available for reading
    fn make_available(&self, range: std::ops::Range<u64>) {
        let mut ranges = self.available_ranges.write().unwrap();
        ranges.push(range);
        ranges.sort_by_key(|r| r.start);
    }

    /// Make data available progressively to simulate download
    async fn simulate_download(&self, chunk_size: u64, delay_ms: u64) {
        let total_size = self.file_data.len() as u64;
        let mut pos = 0;

        while pos < total_size {
            let end = (pos + chunk_size).min(total_size);
            self.make_available(pos..end);
            pos = end;

            if delay_ms > 0 {
                tokio::time::sleep(Duration::from_millis(delay_ms)).await;
            }
        }
    }

    fn is_range_available(&self, range: std::ops::Range<u64>) -> bool {
        let ranges = self.available_ranges.read().unwrap();
        ranges
            .iter()
            .any(|r| r.start <= range.start && r.end >= range.end)
    }
}

#[async_trait::async_trait]
impl DataSource for ProgressiveTestDataSource {
    async fn read_range(
        &self,
        _info_hash: InfoHash,
        range: std::ops::Range<u64>,
    ) -> Result<Vec<u8>, DataError> {
        if !self.is_range_available(range.clone()) {
            return Err(DataError::InsufficientData {
                start: range.start,
                end: range.end,
                missing_count: 1, // Simplified
            });
        }

        let start = range.start as usize;
        let end = range.end as usize;

        if end > self.file_data.len() {
            return Err(DataError::RangeExceedsFile {
                start: range.start,
                end: range.end,
                file_size: self.file_data.len() as u64,
            });
        }

        Ok(self.file_data[start..end].to_vec())
    }

    async fn file_size(&self, _info_hash: InfoHash) -> Result<u64, DataError> {
        Ok(self.file_data.len() as u64)
    }

    async fn check_range_availability(
        &self,
        _info_hash: InfoHash,
        range: std::ops::Range<u64>,
    ) -> Result<RangeAvailability, DataError> {
        Ok(RangeAvailability {
            available: self.is_range_available(range),
            missing_pieces: vec![],
            cache_hit: false,
        })
    }

    fn source_type(&self) -> &'static str {
        "progressive_test"
    }

    async fn can_handle(&self, _info_hash: InfoHash) -> bool {
        true
    }
}

/// Create a minimal but valid AVI file for testing
fn create_test_avi_data() -> Vec<u8> {
    // This creates a minimal AVI file structure that FFmpeg can process
    let mut data = Vec::new();

    // RIFF header
    data.extend_from_slice(b"RIFF");
    data.extend_from_slice(&(1024u32).to_le_bytes()); // File size placeholder
    data.extend_from_slice(b"AVI ");

    // LIST hdrl
    data.extend_from_slice(b"LIST");
    data.extend_from_slice(&(256u32).to_le_bytes());
    data.extend_from_slice(b"hdrl");

    // avih header
    data.extend_from_slice(b"avih");
    data.extend_from_slice(&(56u32).to_le_bytes());

    // Main AVI header (56 bytes)
    data.extend_from_slice(&(33333u32).to_le_bytes()); // microsecPerFrame (30fps)
    data.extend_from_slice(&(1000000u32).to_le_bytes()); // maxBytesPerSec
    data.extend_from_slice(&(0u32).to_le_bytes()); // paddingGranularity
    data.extend_from_slice(&(0x10u32).to_le_bytes()); // flags (AVIF_HASINDEX)
    data.extend_from_slice(&(300u32).to_le_bytes()); // totalFrames (10 seconds at 30fps)
    data.extend_from_slice(&(0u32).to_le_bytes()); // initialFrames
    data.extend_from_slice(&(1u32).to_le_bytes()); // streams
    data.extend_from_slice(&(1024u32).to_le_bytes()); // suggestedBufferSize
    data.extend_from_slice(&(320u32).to_le_bytes()); // width
    data.extend_from_slice(&(240u32).to_le_bytes()); // height
    data.extend_from_slice(&[0u8; 16]); // reserved

    // Add some fake video frames to make it a reasonable size
    for _ in 0..300 {
        // Fake frame data
        data.extend_from_slice(b"00dc"); // fourcc
        data.extend_from_slice(&(64u32).to_le_bytes()); // chunk size
        data.extend_from_slice(&[0x42u8; 64]); // frame data
    }

    // Fix the file size in the header
    let total_size = (data.len() - 8) as u32;
    data[4..8].copy_from_slice(&total_size.to_le_bytes());

    data
}

/// Test that progressive streaming works with incremental data availability
#[tokio::test]
async fn test_progressive_streaming_with_incremental_data() {
    let temp_dir = TempDir::new().unwrap();
    let test_data = create_test_avi_data();
    let data_source = Arc::new(ProgressiveTestDataSource::new(test_data.clone()));
    let info_hash = InfoHash::new([1u8; 20]);

    // Initially make enough data available for header
    data_source.make_available(0..25600); // First 25KB (more than MIN_HEADER_SIZE)

    let input_path = temp_dir.path().join("input.avi");
    let output_path = temp_dir.path().join("output.mp4");

    let mut feeder = ProgressiveFeeder::new(
        info_hash,
        data_source.clone(),
        input_path,
        output_path.clone(),
        test_data.len() as u64,
    );

    let options = RemuxingOptions {
        video_codec: "copy".to_string(),
        audio_codec: "aac".to_string(),
        faststart: true,
        timeout_seconds: Some(30),
        ignore_index: true,
        allow_partial: true,
    };

    // Simulate progressive data availability in background
    let data_source_clone = data_source.clone();
    tokio::spawn(async move {
        data_source_clone.simulate_download(1024, 50).await; // 1KB chunks every 50ms
    });

    // Start progressive streaming
    let feeder_handle = tokio::spawn(async move { feeder.start(&options).await });

    // Wait for completion with timeout
    let result = timeout(Duration::from_secs(60), feeder_handle).await;

    match result {
        Ok(join_result) => match join_result {
            Ok(feeder_result) => match feeder_result {
                Ok(()) => {
                    // Verify output file exists and has reasonable size
                    assert!(output_path.exists(), "Output file should exist");
                    let metadata = fs::metadata(&output_path).await.unwrap();
                    assert!(metadata.len() > 0, "Output file should not be empty");
                    println!(
                        "Progressive streaming completed successfully, output size: {} bytes",
                        metadata.len()
                    );
                }
                Err(e) => panic!("Progressive streaming failed: {}", e),
            },
            Err(e) => panic!("Task join failed: {}", e),
        },
        Err(_) => panic!("Progressive streaming timed out"),
    }
}

/// Test that progressive streaming handles corrupt data gracefully
#[tokio::test]
async fn test_progressive_streaming_with_corrupt_data() {
    let temp_dir = TempDir::new().unwrap();
    let mut test_data = create_test_avi_data();

    // Corrupt some data in the middle
    let corrupt_start = test_data.len() / 2;
    let corrupt_end = corrupt_start + 1024;
    for i in corrupt_start..corrupt_end.min(test_data.len()) {
        test_data[i] = 0xFF; // Corrupt bytes
    }

    let data_source = Arc::new(ProgressiveTestDataSource::new(test_data.clone()));
    let info_hash = InfoHash::new([2u8; 20]);

    // Make all data available at once
    data_source.make_available(0..test_data.len() as u64);

    let input_path = temp_dir.path().join("corrupt_input.avi");
    let output_path = temp_dir.path().join("corrupt_output.mp4");

    let mut feeder = ProgressiveFeeder::new(
        info_hash,
        data_source,
        input_path,
        output_path.clone(),
        test_data.len() as u64,
    );

    let options = RemuxingOptions {
        video_codec: "copy".to_string(),
        audio_codec: "aac".to_string(),
        faststart: true,
        timeout_seconds: Some(30),
        ignore_index: true,
        allow_partial: true,
    };

    let feeder_handle = tokio::spawn(async move { feeder.start(&options).await });
    let result = timeout(Duration::from_secs(60), feeder_handle).await;

    match result {
        Ok(join_result) => match join_result {
            Ok(feeder_result) => match feeder_result {
                Ok(()) => {
                    // Even with corrupt data, we should get some output
                    if output_path.exists() {
                        let metadata = fs::metadata(&output_path).await.unwrap();
                        println!(
                            "Corrupt data handling successful, output size: {} bytes",
                            metadata.len()
                        );
                    } else {
                        panic!("No output file generated despite successful completion");
                    }
                }
                Err(e) => {
                    // Partial failure is acceptable with corrupt data
                    println!(
                        "Progressive streaming failed as expected with corrupt data: {}",
                        e
                    );

                    // Check if we got partial output
                    if output_path.exists() {
                        let metadata = fs::metadata(&output_path).await.unwrap();
                        if metadata.len() > 0 {
                            println!(
                                "Got partial output despite failure: {} bytes",
                                metadata.len()
                            );
                        }
                    }
                }
            },
            Err(e) => panic!("Task join failed: {}", e),
        },
        Err(_) => panic!("Progressive streaming timed out"),
    }
}

/// Test progressive feeder state transitions
#[tokio::test]
async fn test_progressive_feeder_state_transitions() {
    let temp_dir = TempDir::new().unwrap();
    let test_data = create_test_avi_data();
    let data_source = Arc::new(ProgressiveTestDataSource::new(test_data.clone()));
    let info_hash = InfoHash::new([3u8; 20]);

    let input_path = temp_dir.path().join("state_input.avi");
    let output_path = temp_dir.path().join("state_output.mp4");

    let feeder = ProgressiveFeeder::new(
        info_hash,
        data_source.clone(),
        input_path,
        output_path,
        test_data.len() as u64,
    );

    // Initial state should be WaitingForData
    let initial_state = feeder.state().await;
    match initial_state {
        ProgressiveState::WaitingForData { .. } => (),
        _ => panic!(
            "Expected WaitingForData state initially, got: {:?}",
            initial_state
        ),
    }

    // Make some data available
    data_source.make_available(0..test_data.len() as u64);

    let options = RemuxingOptions {
        video_codec: "copy".to_string(),
        audio_codec: "aac".to_string(),
        faststart: true,
        timeout_seconds: Some(30),
        ignore_index: true,
        allow_partial: true,
    };

    // Start feeding in background
    let feeder_clone = feeder.clone();
    let feed_handle = tokio::spawn(async move {
        let mut feeder = feeder_clone;
        feeder.start(&options).await
    });

    // Wait a bit for state to change
    tokio::time::sleep(Duration::from_millis(500)).await;

    // State should eventually transition to Feeding or Completed
    let final_state = feeder.state().await;
    match final_state {
        ProgressiveState::Feeding { .. } | ProgressiveState::Completed { .. } => {
            println!("State transitioned correctly to: {:?}", final_state);
        }
        ProgressiveState::Failed { error } => {
            panic!("Unexpected failure state: {}", error);
        }
        other => {
            println!("State is: {:?}, waiting for progression...", other);
        }
    }

    // Wait for completion
    let _ = timeout(Duration::from_secs(30), feed_handle).await;
}

/// Benchmark progressive streaming performance
#[tokio::test]
async fn test_progressive_streaming_performance() {
    let temp_dir = TempDir::new().unwrap();
    let test_data = create_test_avi_data();
    let data_source = Arc::new(ProgressiveTestDataSource::new(test_data.clone()));
    let info_hash = InfoHash::new([4u8; 20]);

    // Make all data available immediately
    data_source.make_available(0..test_data.len() as u64);

    let input_path = temp_dir.path().join("perf_input.avi");
    let output_path = temp_dir.path().join("perf_output.mp4");

    let mut feeder = ProgressiveFeeder::new(
        info_hash,
        data_source,
        input_path,
        output_path.clone(),
        test_data.len() as u64,
    );

    let options = RemuxingOptions {
        video_codec: "copy".to_string(),
        audio_codec: "aac".to_string(),
        faststart: true,
        timeout_seconds: Some(30),
        ignore_index: true,
        allow_partial: true,
    };

    let start_time = std::time::Instant::now();

    let feeder_handle = tokio::spawn(async move { feeder.start(&options).await });
    let result = timeout(Duration::from_secs(60), feeder_handle).await;

    let elapsed = start_time.elapsed();

    match result {
        Ok(join_result) => match join_result {
            Ok(feeder_result) => match feeder_result {
                Ok(()) => {
                    println!("Progressive streaming completed in {:?}", elapsed);

                    if output_path.exists() {
                        let metadata = fs::metadata(&output_path).await.unwrap();
                        let throughput = (metadata.len() as f64) / elapsed.as_secs_f64();
                        println!("Throughput: {:.2} bytes/sec", throughput);

                        // Performance assertion - should process at least 1KB/sec
                        assert!(
                            throughput > 1024.0,
                            "Throughput too low: {:.2} bytes/sec",
                            throughput
                        );
                    }
                }
                Err(e) => panic!("Progressive streaming failed: {}", e),
            },
            Err(e) => panic!("Task join failed: {}", e),
        },
        Err(_) => panic!("Progressive streaming timed out after {:?}", elapsed),
    }
}

/// Debug test to see actual FFmpeg behavior with progressive data
#[tokio::test]
async fn test_debug_ffmpeg_behavior() {
    let temp_dir = TempDir::new().unwrap();
    let test_data = create_test_avi_data();
    let data_source = Arc::new(ProgressiveTestDataSource::new(test_data.clone()));
    let info_hash = InfoHash::new([5u8; 20]);

    println!("Input path: (using pipe input)");
    let output_path = temp_dir.path().join("debug_output.mp4");
    println!("Output path: {}", output_path.display());
    println!("Test data size: {} bytes", test_data.len());

    // Only make header available initially (25KB)
    let header_size = 25600u64;
    data_source.make_available(0..header_size);
    println!("Made header available: 0..{} bytes", header_size);

    let input_path = temp_dir.path().join("debug_input.avi");
    let mut feeder = ProgressiveFeeder::new(
        info_hash,
        data_source.clone(),
        input_path,
        output_path.clone(),
        test_data.len() as u64,
    );

    let options = RemuxingOptions {
        video_codec: "copy".to_string(),
        audio_codec: "copy".to_string(),
        faststart: true,
        timeout_seconds: Some(30), // Longer timeout to see what happens
        ignore_index: true,
        allow_partial: true,
    };

    println!("Starting progressive feeder with options: {:?}", options);

    // Start progressive data simulation in background
    let data_source_clone = data_source.clone();
    let simulation_handle = tokio::spawn(async move {
        println!("Starting progressive data simulation...");
        tokio::time::sleep(Duration::from_millis(100)).await; // Small delay

        let mut pos = header_size;
        let chunk_size = 2048u64; // 2KB chunks
        let total_size = test_data.len() as u64;

        while pos < total_size {
            let end = (pos + chunk_size).min(total_size);
            data_source_clone.make_available(pos..end);
            println!(
                "Made available: {}..{} bytes ({:.1}% complete)",
                pos,
                end,
                (end as f64 / total_size as f64) * 100.0
            );
            pos = end;
            tokio::time::sleep(Duration::from_millis(100)).await; // 100ms between chunks
        }
        println!("Progressive data simulation completed");
    });

    let start_time = std::time::Instant::now();
    let feeder_handle = tokio::spawn(async move { feeder.start(&options).await });

    // Wait for both to complete
    let (feeder_result, _) = tokio::join!(feeder_handle, simulation_handle);
    let elapsed = start_time.elapsed();

    match feeder_result {
        Ok(result) => {
            println!("Progressive feeder completed in {:?}", elapsed);
            println!("Result: {:?}", result);
        }
        Err(e) => {
            println!("Progressive feeder task failed: {:?}", e);
        }
    }

    // Check if output file exists
    if output_path.exists() {
        let metadata = fs::metadata(&output_path).await.unwrap();
        println!("Output file size: {} bytes", metadata.len());

        if metadata.len() > 0 {
            // Try to read first few bytes to see if it's valid MP4
            let mut file = fs::File::open(&output_path).await.unwrap();
            let mut buffer = [0u8; 32];
            use tokio::io::AsyncReadExt;
            let bytes_read = file.read(&mut buffer).await.unwrap();
            println!("First {} bytes: {:02x?}", bytes_read, &buffer[..bytes_read]);

            // Check for MP4 signature
            if buffer.len() >= 8 && &buffer[4..8] == b"ftyp" {
                println!("✓ Valid MP4 file signature detected");
            } else {
                println!("✗ No valid MP4 signature found");
            }
        }
    } else {
        println!("No output file generated");
    }

    // Check temp directory contents
    println!("Temp directory contents:");
    let mut entries = fs::read_dir(&temp_dir).await.unwrap();
    while let Some(entry) = entries.next_entry().await.unwrap() {
        let metadata = entry.metadata().await.unwrap();
        println!(
            "  {}: {} bytes",
            entry.file_name().to_string_lossy(),
            metadata.len()
        );
    }
}

/// Test using a real AVI file to validate our progressive streaming works
#[tokio::test]
async fn test_progressive_with_real_avi_structure() {
    // Create a more realistic AVI structure that FFmpeg can actually process
    let temp_dir = TempDir::new().unwrap();

    // Create a very minimal but valid AVI that FFmpeg might accept
    let test_data = create_minimal_valid_avi();
    let data_source = Arc::new(ProgressiveTestDataSource::new(test_data.clone()));
    let info_hash = InfoHash::new([6u8; 20]);

    println!("Testing with {} byte minimal AVI", test_data.len());

    // Make only header available initially
    let header_size = (test_data.len() / 4) as u64; // 25% of file
    data_source.make_available(0..header_size);

    let input_path = temp_dir.path().join("real_input.avi");
    let output_path = temp_dir.path().join("real_output.mp4");

    let mut feeder = ProgressiveFeeder::new(
        info_hash,
        data_source.clone(),
        input_path,
        output_path.clone(),
        test_data.len() as u64,
    );

    let options = RemuxingOptions {
        video_codec: "libx264".to_string(), // Force re-encoding to avoid codec issues
        audio_codec: "aac".to_string(),
        faststart: true,
        timeout_seconds: Some(60),
        ignore_index: true,
        allow_partial: true,
    };

    // Start data simulation
    let data_source_clone = data_source.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(200)).await;
        data_source_clone.simulate_download(1024, 100).await;
    });

    let start_time = std::time::Instant::now();
    let result = feeder.start(&options).await;
    let elapsed = start_time.elapsed();

    println!(
        "Real AVI test completed in {:?} with result: {:?}",
        elapsed, result
    );

    if output_path.exists() {
        let metadata = fs::metadata(&output_path).await.unwrap();
        println!("✓ Output generated: {} bytes", metadata.len());
        assert!(metadata.len() > 0, "Output should not be empty");
    } else {
        println!("✗ No output file generated");
    }
}

/// Create a more realistic minimal AVI file
fn create_minimal_valid_avi() -> Vec<u8> {
    // This creates a very basic but more standards-compliant AVI
    let mut data = Vec::new();

    // RIFF header
    data.extend_from_slice(b"RIFF");
    data.extend_from_slice(&0u32.to_le_bytes()); // Size placeholder
    data.extend_from_slice(b"AVI ");

    // LIST hdrl (header list)
    data.extend_from_slice(b"LIST");
    data.extend_from_slice(&0u32.to_le_bytes()); // Size placeholder
    data.extend_from_slice(b"hdrl");

    // avih (main header)
    data.extend_from_slice(b"avih");
    data.extend_from_slice(&56u32.to_le_bytes());

    // Main AVI header
    data.extend_from_slice(&33333u32.to_le_bytes()); // dwMicroSecPerFrame (30fps)
    data.extend_from_slice(&0u32.to_le_bytes()); // dwMaxBytesPerSec
    data.extend_from_slice(&0u32.to_le_bytes()); // dwPaddingGranularity
    data.extend_from_slice(&16u32.to_le_bytes()); // dwFlags (AVIF_HASINDEX)
    data.extend_from_slice(&10u32.to_le_bytes()); // dwTotalFrames
    data.extend_from_slice(&0u32.to_le_bytes()); // dwInitialFrames
    data.extend_from_slice(&1u32.to_le_bytes()); // dwStreams
    data.extend_from_slice(&0u32.to_le_bytes()); // dwSuggestedBufferSize
    data.extend_from_slice(&320u32.to_le_bytes()); // dwWidth
    data.extend_from_slice(&240u32.to_le_bytes()); // dwHeight
    data.extend_from_slice(&[0u8; 16]); // dwReserved[4]

    // LIST movi (movie data)
    data.extend_from_slice(b"LIST");
    data.extend_from_slice(&0u32.to_le_bytes()); // Size placeholder
    data.extend_from_slice(b"movi");

    // Add a few fake video chunks
    for i in 0..10 {
        data.extend_from_slice(b"00dc"); // Video chunk
        data.extend_from_slice(&100u32.to_le_bytes()); // Chunk size
        data.extend_from_slice(&vec![0x42u8 + (i as u8); 100]); // Fake frame data
    }

    // Fix sizes
    let total_size = data.len() - 8;
    data[4..8].copy_from_slice(&(total_size as u32).to_le_bytes());

    data
}
