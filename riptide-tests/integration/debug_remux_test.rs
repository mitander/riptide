//! Debug tests for remux stream issues.
//!
//! This module contains targeted tests to debug the remux truncation issue.

use std::sync::Arc;
use std::time::Duration;

use axum::http::Request;
use bytes::Bytes;
use riptide_core::streaming::remux_stream::RemuxStreamProducer;
use riptide_core::streaming::traits::{PieceProvider, StreamProducer};
use riptide_sim::streaming::MockPieceProvider;
use tokio::process::Command;
use tokio::time::timeout;

/// Test to verify that the input pump feeds all data to FFmpeg.
#[tokio::test]
async fn test_input_pump_feeds_all_data() {
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

    // Create a large test file - use the existing test MKV
    let test_file_path = concat!(env!("CARGO_MANIFEST_DIR"), "/../test_movies/test_mkv.mkv");

    let file_bytes = match tokio::fs::read(test_file_path).await {
        Ok(bytes) => bytes,
        Err(_) => {
            eprintln!("Skipping test: Test file not found at {test_file_path}");
            return;
        }
    };

    println!("Test file size: {} bytes", file_bytes.len());

    // Create mock provider
    let provider = Arc::new(MockPieceProvider::new(Bytes::from(file_bytes.clone())));

    // Verify the provider can read all data
    let total_size = provider.size().await;
    println!("Provider reports size: {total_size} bytes");

    // Read the entire file from the provider to verify it works
    let mut all_data = Vec::new();
    let mut offset = 0;
    const CHUNK_SIZE: usize = 64 * 1024;

    while offset < total_size {
        let chunk_size = std::cmp::min(CHUNK_SIZE, (total_size - offset) as usize);
        let chunk = provider.read_at(offset, chunk_size).await.unwrap();
        all_data.extend_from_slice(&chunk);
        offset += chunk.len() as u64;
    }

    println!("Successfully read {} bytes from provider", all_data.len());
    assert_eq!(all_data.len(), file_bytes.len());

    // Create remux producer
    let producer = RemuxStreamProducer::new(provider, "mkv".to_string());

    // Stream the file
    let request = Request::builder().body(()).unwrap();
    let response = producer.produce_stream(request).await;

    // Collect all output with generous timeout
    let remuxed_bytes = timeout(Duration::from_secs(60), async {
        axum::body::to_bytes(response.into_body(), 200 * 1024 * 1024).await
    })
    .await
    .expect("Remuxing should complete within timeout")
    .expect("Should be able to read response body");

    println!("Remuxed output size: {} bytes", remuxed_bytes.len());

    // The remuxed output should be substantial (at least 50% of input size)
    let min_expected_size = file_bytes.len() / 2;
    assert!(
        remuxed_bytes.len() >= min_expected_size,
        "Remuxed output too small: {} bytes, expected at least {} bytes",
        remuxed_bytes.len(),
        min_expected_size
    );
}

/// Test to verify FFmpeg command works correctly in isolation.
#[tokio::test]
async fn test_ffmpeg_command_isolation() {
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

    println!(
        "Testing FFmpeg command with {} byte input",
        file_bytes.len()
    );

    // Spawn FFmpeg process directly
    let mut child = Command::new("ffmpeg")
        .args([
            "-hide_banner",
            "-loglevel",
            "error",
            "-i",
            "pipe:0",
            "-c:v",
            "copy",
            "-c:a",
            "copy",
            "-movflags",
            "frag_keyframe+empty_moov+default_base_moof",
            "-f",
            "mp4",
            "pipe:1",
        ])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("Failed to spawn FFmpeg");

    // Write input data
    let stdin = child.stdin.take().unwrap();
    let input_bytes = file_bytes.clone();

    let write_task = tokio::spawn(async move {
        use tokio::io::AsyncWriteExt;
        let mut stdin = tokio::io::BufWriter::new(stdin);

        // Write data in chunks
        const CHUNK_SIZE: usize = 64 * 1024;
        let mut written = 0;

        for chunk in input_bytes.chunks(CHUNK_SIZE) {
            stdin.write_all(chunk).await.unwrap();
            written += chunk.len();

            if written % (1024 * 1024) == 0 {
                println!("Written {written} bytes to FFmpeg");
            }
        }

        stdin.flush().await.unwrap();
        drop(stdin);
        println!("Finished writing {written} bytes to FFmpeg");
    });

    // Read output
    let mut stdout = child.stdout.take().unwrap();
    let mut output_data = Vec::new();

    let read_task = tokio::spawn(async move {
        use tokio::io::AsyncReadExt;
        let mut buffer = vec![0u8; 8192];
        let mut total_read = 0;

        loop {
            match stdout.read(&mut buffer).await {
                Ok(0) => break, // EOF
                Ok(n) => {
                    output_data.extend_from_slice(&buffer[..n]);
                    total_read += n;

                    if total_read % (1024 * 1024) == 0 {
                        println!("Read {total_read} bytes from FFmpeg");
                    }
                }
                Err(e) => {
                    eprintln!("Error reading from FFmpeg: {e}");
                    break;
                }
            }
        }

        println!("Finished reading {total_read} bytes from FFmpeg");
        output_data
    });

    // Wait for both tasks
    let (write_result, read_result) = tokio::join!(write_task, read_task);
    write_result.unwrap();
    let output = read_result.unwrap();

    // Wait for FFmpeg to complete
    let status = child.wait().await.unwrap();
    println!("FFmpeg exit status: {status}");

    println!("Final output size: {} bytes", output.len());

    // Verify we got substantial output
    assert!(
        output.len() > 1000,
        "FFmpeg should produce substantial output"
    );

    // Verify the output is valid MP4 by checking for ftyp box
    assert!(output.len() >= 8, "Output should be at least 8 bytes");
    assert_eq!(
        &output[4..8],
        b"ftyp",
        "Output should start with MP4 ftyp box"
    );
}
