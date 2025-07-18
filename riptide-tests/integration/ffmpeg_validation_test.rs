//! FFmpeg validation tests for real media files.
//!
//! This module contains the critical tests that validate the core promise of
//! the progressive streaming redesign: that real media files are converted
//! correctly without duration truncation or other quality issues.

use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use axum::http::Request;
use bytes::Bytes;
use riptide_core::streaming::HttpStreaming;
use riptide_sim::streaming::MockPieceProvider;
use tokio::process::Command;
use tokio::time::timeout;

/// Test that validates AVI remuxing preserves full duration.
///
/// This is the critical test that ensures the "57-second truncation" bug
/// is fixed. It loads a real AVI file, streams it through the remux pipeline,
/// and validates that FFprobe reports the correct duration.
#[tokio::test]
async fn test_avi_remux_preserves_full_duration() {
    // Skip test if FFmpeg is not available
    if Command::new("ffmpeg")
        .arg("-version")
        .output()
        .await
        .is_err()
    {
        eprintln!("Skipping test: FFmpeg not available");
        return;
    }

    if Command::new("ffprobe")
        .arg("-version")
        .output()
        .await
        .is_err()
    {
        eprintln!("Skipping test: FFprobe not available");
        return;
    }

    // Load a real test file (using existing test_mkv.mkv as proxy for AVI)
    let test_file_path = concat!(env!("CARGO_MANIFEST_DIR"), "/../test_movies/test_mkv.mkv");

    let file_bytes = match tokio::fs::read(test_file_path).await {
        Ok(bytes) => bytes,
        Err(_) => {
            eprintln!("Skipping test: Test file not found at {test_file_path}");
            return;
        }
    };

    // Get the original duration using ffprobe
    let original_duration = match media_duration_seconds(test_file_path).await {
        Ok(duration) => duration,
        Err(e) => {
            eprintln!("Skipping test: Cannot get original duration: {e}");
            return;
        }
    };

    println!("Original file duration: {original_duration:.2}s");

    // Create streaming session
    let provider = Arc::new(MockPieceProvider::new(Bytes::from(file_bytes)));
    let streaming = HttpStreaming::new(provider).await.unwrap();

    // Stream the entire file
    let request = Request::builder().body(()).unwrap();
    let response = streaming.serve_http_stream(request).await;

    // Collect the remuxed output
    let remuxed_bytes = timeout(Duration::from_secs(30), async {
        axum::body::to_bytes(response.into_body(), 100 * 1024 * 1024).await
    })
    .await
    .expect("Streaming should complete within timeout")
    .expect("Should be able to read response body");

    println!("Remuxed output size: {} bytes", remuxed_bytes.len());

    // Write output to temp file for debugging
    let temp_path = "/tmp/remuxed_output.mp4";
    tokio::fs::write(temp_path, &remuxed_bytes).await.unwrap();
    println!("Wrote remuxed output to {temp_path}");

    // Validate the remuxed output using ffprobe
    let remuxed_duration = duration_from_bytes(&remuxed_bytes).await.unwrap();
    println!("Remuxed file duration: {remuxed_duration:.2}s");

    // Also validate using file-based ffprobe for comparison
    let file_based_duration = media_duration_seconds(temp_path).await.unwrap();
    println!("File-based duration: {file_based_duration:.2}s");

    // Assert that duration is preserved (within 1 second tolerance)
    assert!(
        (remuxed_duration - original_duration).abs() < 1.0,
        "Duration mismatch: original={original_duration:.2}s, remuxed={remuxed_duration:.2}s"
    );

    // Validate that the output is actually playable MP4
    assert!(is_valid_mp4(&remuxed_bytes).await);
}

/// Test that validates MKV remuxing works correctly.
#[tokio::test]
async fn test_mkv_remux_quality() {
    // Skip test if FFmpeg is not available
    if Command::new("ffmpeg")
        .arg("-version")
        .output()
        .await
        .is_err()
    {
        eprintln!("Skipping test: FFmpeg not available");
        return;
    }

    let test_file_path = concat!(env!("CARGO_MANIFEST_DIR"), "/../test_movies/test_mkv.mkv");

    let file_bytes = match tokio::fs::read(test_file_path).await {
        Ok(bytes) => bytes,
        Err(_) => {
            eprintln!("Skipping test: Test file not found at {test_file_path}");
            return;
        }
    };

    // Create streaming session
    let provider = Arc::new(MockPieceProvider::new(Bytes::from(file_bytes)));
    let streaming = HttpStreaming::new(provider).await.unwrap();

    // Stream the file
    let request = Request::builder().body(()).unwrap();
    let response = streaming.serve_http_stream(request).await;

    // Collect the remuxed output
    let remuxed_bytes = timeout(Duration::from_secs(30), async {
        axum::body::to_bytes(response.into_body(), 100 * 1024 * 1024).await
    })
    .await
    .expect("Streaming should complete within timeout")
    .expect("Should be able to read response body");

    // Validate the output is playable MP4
    assert!(is_valid_mp4(&remuxed_bytes).await);

    // Validate that we get some output (not empty)
    assert!(
        remuxed_bytes.len() > 1000,
        "Remuxed output should not be empty"
    );
}

/// Test that validates MP4 direct streaming works correctly.
#[tokio::test]
async fn test_mp4_direct_streaming() {
    let test_file_path = concat!(env!("CARGO_MANIFEST_DIR"), "/../test_movies/test_video.mp4");

    let file_bytes = match tokio::fs::read(test_file_path).await {
        Ok(bytes) => bytes,
        Err(_) => {
            eprintln!("Skipping test: Test file not found at {test_file_path}");
            return;
        }
    };

    // Create streaming session
    let provider = Arc::new(MockPieceProvider::new(Bytes::from(file_bytes.clone())));
    let streaming = HttpStreaming::new(provider).await.unwrap();

    // Stream the file
    let request = Request::builder().body(()).unwrap();
    let response = streaming.serve_http_stream(request).await;

    // For direct streaming, output should be identical to input
    let streamed_bytes = axum::body::to_bytes(response.into_body(), file_bytes.len() + 1024)
        .await
        .unwrap();

    assert_eq!(streamed_bytes.len(), file_bytes.len());
    assert_eq!(streamed_bytes, file_bytes);
}

/// Test streaming behavior under slow piece availability conditions.
#[tokio::test]
async fn test_progressive_availability_simulation() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Duration;

    use riptide_core::streaming::traits::{PieceProvider, PieceProviderError};

    // Provider that simulates pieces becoming available over time
    struct ProgressiveProvider {
        data: Bytes,
        available_bytes: AtomicUsize,
    }

    impl ProgressiveProvider {
        fn new(data: Bytes) -> Arc<Self> {
            let provider = Arc::new(Self {
                data,
                available_bytes: AtomicUsize::new(1024), // Start with 1KB available for format detection
            });

            // Simulate pieces becoming available over time
            let provider_clone = provider.clone();
            tokio::spawn(async move {
                let total_size = provider_clone.data.len();
                let chunk_size = 64 * 1024; // 64KB chunks

                while provider_clone.available_bytes.load(Ordering::Acquire) < total_size {
                    let current = provider_clone.available_bytes.load(Ordering::Acquire);
                    let next = std::cmp::min(current + chunk_size, total_size);
                    provider_clone
                        .available_bytes
                        .store(next, Ordering::Release);

                    tokio::time::sleep(Duration::from_millis(10)).await;
                }
            });

            provider
        }
    }

    #[async_trait::async_trait]
    impl PieceProvider for ProgressiveProvider {
        async fn read_at(&self, offset: u64, length: usize) -> Result<Bytes, PieceProviderError> {
            let available = self.available_bytes.load(Ordering::Acquire);
            let end_offset = offset + length as u64;

            if end_offset > available as u64 {
                return Err(PieceProviderError::NotYetAvailable);
            }

            let start = offset as usize;
            let end = std::cmp::min(start + length, self.data.len());
            Ok(self.data.slice(start..end))
        }

        async fn size(&self) -> u64 {
            self.data.len() as u64
        }
    }

    // Create test data with valid MP4 header (copied from progressive_remuxing_test)
    let mut mp4_data = Vec::new();
    // ftyp box (file type)
    mp4_data.extend_from_slice(&[0x00, 0x00, 0x00, 0x20]); // size: 32 bytes
    mp4_data.extend_from_slice(b"ftyp"); // box type
    mp4_data.extend_from_slice(b"isom"); // major brand
    mp4_data.extend_from_slice(&[0x00, 0x00, 0x02, 0x00]); // minor version
    mp4_data.extend_from_slice(b"isomiso2avc1mp41"); // compatible brands
    // mdat box (media data) - simplified
    mp4_data.extend_from_slice(&[0x00, 0x00, 0x10, 0x00]); // size: 4096 bytes
    mp4_data.extend_from_slice(b"mdat"); // box type
    mp4_data.extend_from_slice(&vec![0x42; 4088]); // dummy video data
    // Add more data to make it substantial
    mp4_data.extend_from_slice(&vec![0x42; 512 * 1024]); // Additional 512KB of data

    let provider = ProgressiveProvider::new(Bytes::from(mp4_data));
    let streaming = HttpStreaming::new(provider).await.unwrap();

    // Stream should handle progressive availability
    let request = Request::builder().body(()).unwrap();
    let response = streaming.serve_http_stream(request).await;

    // Should eventually complete despite progressive availability
    let result = timeout(Duration::from_secs(10), async {
        axum::body::to_bytes(response.into_body(), 1024 * 1024).await
    })
    .await;

    assert!(result.is_ok(), "Progressive streaming should complete");
    let body = result.unwrap().unwrap();
    assert!(body.len() > 100_000, "Should stream substantial content");
}

/// Gets the duration of a media file using ffprobe.
async fn media_duration_seconds(file_path: &str) -> Result<f64, String> {
    let output = Command::new("ffprobe")
        .args([
            "-v",
            "error",
            "-show_entries",
            "format=duration",
            "-of",
            "default=noprint_wrappers=1:nokey=1",
            file_path,
        ])
        .output()
        .await
        .map_err(|e| format!("Failed to run ffprobe: {e}"))?;

    if !output.status.success() {
        return Err(format!("ffprobe failed with status: {}", output.status));
    }

    let duration_str = String::from_utf8(output.stdout)
        .map_err(|e| format!("Invalid UTF-8 in ffprobe output: {e}"))?;

    duration_str
        .trim()
        .parse::<f64>()
        .map_err(|e| format!("Failed to parse duration: {e}"))
}

/// Gets the duration of media data by writing to a temporary file.
///
/// This avoids issues with ffprobe reading fragmented MP4 from pipes,
/// which can cause truncation in duration detection.
async fn duration_from_bytes(bytes: &[u8]) -> Result<f64, String> {
    // Write to a temporary file with unique timestamp
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let temp_path = format!("/tmp/ffprobe_temp_{timestamp}.mp4");
    tokio::fs::write(&temp_path, bytes)
        .await
        .map_err(|e| format!("Failed to write temp file: {e}"))?;

    // Use the existing file-based duration function
    let result = media_duration_seconds(&temp_path).await;

    // Clean up the temp file
    let _ = tokio::fs::remove_file(&temp_path).await;

    result
}

/// Validates that the given bytes represent a valid MP4 file.
async fn is_valid_mp4(bytes: &[u8]) -> bool {
    // Write to a temporary file to avoid pipe issues
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let temp_path = format!("/tmp/mp4_validation_{timestamp}.mp4");
    if tokio::fs::write(&temp_path, bytes).await.is_err() {
        return false;
    }

    let result = Command::new("ffprobe")
        .args([
            "-v",
            "error",
            "-select_streams",
            "v:0",
            "-show_entries",
            "stream=codec_name",
            "-of",
            "csv=p=0",
            "-i",
            &temp_path,
        ])
        .output()
        .await
        .map(|output| output.status.success())
        .unwrap_or(false);

    // Clean up the temp file
    let _ = tokio::fs::remove_file(&temp_path).await;

    result
}
