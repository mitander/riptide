//! Integration tests for the progressive streaming redesign.
//!
//! These tests validate the complete streaming pipeline from piece provider
//! through to HTTP response, including both direct streaming and remuxing paths.

use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::http::{Request, StatusCode};
use bytes::Bytes;
use riptide_core::streaming::direct_stream::DirectStreamProducer;
use riptide_core::streaming::remux_stream::RemuxStreamProducer;
use riptide_core::streaming::traits::{PieceProvider, PieceProviderError, StreamProducer};
use riptide_sim::streaming::MockPieceProvider;
use tokio::time::timeout;

/// Creates a mock MP4 file with minimal valid structure.
fn create_mock_mp4_data() -> Bytes {
    let mut data = Vec::new();

    // ftyp box (file type)
    data.extend_from_slice(&[0x00, 0x00, 0x00, 0x20]); // size: 32 bytes
    data.extend_from_slice(b"ftyp"); // box type
    data.extend_from_slice(b"isom"); // major brand
    data.extend_from_slice(&[0x00, 0x00, 0x02, 0x00]); // minor version
    data.extend_from_slice(b"isomiso2avc1mp41"); // compatible brands

    // mdat box (media data) - simplified
    data.extend_from_slice(&[0x00, 0x00, 0x10, 0x00]); // size: 4096 bytes
    data.extend_from_slice(b"mdat"); // box type
    data.extend_from_slice(&vec![0x42; 4088]); // dummy video data

    Bytes::from(data)
}

/// Creates a mock MKV file header (simplified).
fn create_mock_mkv_data() -> Bytes {
    let mut data = Vec::new();

    // EBML header
    data.extend_from_slice(&[0x1A, 0x45, 0xDF, 0xA3]); // EBML ID
    data.extend_from_slice(&[0x93]); // size (19 bytes)
    data.extend_from_slice(&[0x42, 0x86, 0x81, 0x01]); // EBMLVersion
    data.extend_from_slice(&[0x42, 0xF7, 0x81, 0x01]); // EBMLReadVersion
    data.extend_from_slice(&[0x42, 0xF2, 0x81, 0x04]); // EBMLMaxIDLength
    data.extend_from_slice(&[0x42, 0xF3, 0x81, 0x08]); // EBMLMaxSizeLength
    data.extend_from_slice(&[0x42, 0x82, 0x88]); // DocType
    data.extend_from_slice(b"matroska");

    // Add some dummy content
    data.extend_from_slice(&vec![0x00; 10240]); // 10KB of content

    Bytes::from(data)
}

/// Creates a mock AVI file header (simplified).
#[allow(dead_code)]
fn create_mock_avi_data() -> Bytes {
    let mut data = Vec::new();

    // RIFF header
    data.extend_from_slice(b"RIFF");
    data.extend_from_slice(&[0x00, 0x40, 0x00, 0x00]); // file size - 8
    data.extend_from_slice(b"AVI ");

    // LIST hdrl
    data.extend_from_slice(b"LIST");
    data.extend_from_slice(&[0x00, 0x10, 0x00, 0x00]); // list size
    data.extend_from_slice(b"hdrl");

    // avih (AVI header)
    data.extend_from_slice(b"avih");
    data.extend_from_slice(&[0x38, 0x00, 0x00, 0x00]); // header size

    // Add some dummy header data
    data.extend_from_slice(&vec![0x00; 56]);

    // Add some dummy content
    data.extend_from_slice(&vec![0x00; 16384]); // 16KB of content

    Bytes::from(data)
}

#[tokio::test]
async fn test_direct_streaming_mp4() {
    let mp4_data = create_mock_mp4_data();
    let provider = Arc::new(MockPieceProvider::new(mp4_data.clone()));
    let producer = DirectStreamProducer::new(provider, "video/mp4".to_string());

    let request = Request::builder().body(()).unwrap();
    let response = producer.produce_stream(request).await;

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers().get("content-type").unwrap(), "video/mp4");
    assert_eq!(
        response.headers().get("content-length").unwrap(),
        &mp4_data.len().to_string()
    );

    // Verify we can read the entire body
    let body_bytes = axum::body::to_bytes(response.into_body(), mp4_data.len() + 1024)
        .await
        .unwrap();
    assert_eq!(body_bytes.len(), mp4_data.len());
}

#[tokio::test]
async fn test_direct_streaming_range_request() {
    let mp4_data = create_mock_mp4_data();
    let provider = Arc::new(MockPieceProvider::new(mp4_data.clone()));
    let producer = DirectStreamProducer::new(provider, "video/mp4".to_string());

    // Request bytes 100-199
    let request = Request::builder()
        .header("Range", "bytes=100-199")
        .body(())
        .unwrap();
    let response = producer.produce_stream(request).await;

    assert_eq!(response.status(), StatusCode::PARTIAL_CONTENT);
    assert_eq!(response.headers().get("content-length").unwrap(), "100");
    assert_eq!(
        response.headers().get("content-range").unwrap(),
        &format!("bytes 100-199/{}", mp4_data.len())
    );

    let body_bytes = axum::body::to_bytes(response.into_body(), 1024)
        .await
        .unwrap();
    assert_eq!(body_bytes.len(), 100);
}

#[tokio::test]
async fn test_remux_streaming_mkv() {
    // Skip test if FFmpeg is not available
    if std::process::Command::new("ffmpeg")
        .arg("-version")
        .output()
        .is_err()
    {
        eprintln!("Skipping test: FFmpeg not available");
        return;
    }

    let mkv_data = create_mock_mkv_data();
    let provider = Arc::new(MockPieceProvider::new(mkv_data));
    let producer = RemuxStreamProducer::new(provider, "mkv".to_string());

    let request = Request::builder().body(()).unwrap();
    let response = producer.produce_stream(request).await;

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers().get("content-type").unwrap(), "video/mp4");

    // Note: We can't easily verify the remuxed content without parsing MP4,
    // but we can at least ensure the response starts streaming
    let response_body = response.into_body();

    // Try to read first chunk within timeout
    let result = timeout(Duration::from_secs(5), async {
        let first_chunk = axum::body::to_bytes(response_body, 1024).await;
        first_chunk
    })
    .await;

    assert!(result.is_ok(), "Should receive data within timeout");
}

#[tokio::test]
async fn test_streaming_with_delayed_pieces() {
    use std::sync::atomic::{AtomicU32, Ordering};

    // Custom provider that delays the first few reads
    struct DelayedProvider {
        inner: MockPieceProvider,
        delay_count: AtomicU32,
    }

    #[async_trait::async_trait]
    impl PieceProvider for DelayedProvider {
        async fn read_at(&self, offset: u64, length: usize) -> Result<Bytes, PieceProviderError> {
            if self.delay_count.fetch_add(1, Ordering::SeqCst) < 3 {
                tokio::time::sleep(Duration::from_millis(50)).await;
                return Err(PieceProviderError::NotYetAvailable);
            }
            self.inner.read_at(offset, length).await
        }

        async fn size(&self) -> u64 {
            self.inner.size().await
        }
    }

    let mp4_data = create_mock_mp4_data();
    let inner = MockPieceProvider::new(mp4_data.clone());
    let provider = Arc::new(DelayedProvider {
        inner,
        delay_count: AtomicU32::new(0),
    });
    let producer = DirectStreamProducer::new(provider, "video/mp4".to_string());

    let start = Instant::now();
    let request = Request::builder().body(()).unwrap();
    let response = producer.produce_stream(request).await;

    assert_eq!(response.status(), StatusCode::OK);

    // Should complete despite delays
    let body_bytes = axum::body::to_bytes(response.into_body(), mp4_data.len() + 1024)
        .await
        .unwrap();
    assert_eq!(body_bytes.len(), mp4_data.len());

    // Should have taken at least 150ms due to delays
    assert!(start.elapsed() >= Duration::from_millis(150));
}

#[tokio::test]
async fn test_streaming_performance() {
    // Create 10MB of test data
    let large_data = Bytes::from(vec![0u8; 10 * 1024 * 1024]);
    let provider = Arc::new(MockPieceProvider::new(large_data.clone()));
    let producer = DirectStreamProducer::new(provider, "video/mp4".to_string());

    let start = Instant::now();
    let request = Request::builder().body(()).unwrap();
    let response = producer.produce_stream(request).await;

    // First byte should be available almost immediately
    let first_byte_time = start.elapsed();
    assert!(
        first_byte_time < Duration::from_millis(10),
        "First byte took {:?}",
        first_byte_time
    );

    // Stream entire body
    let body_bytes = axum::body::to_bytes(response.into_body(), 11 * 1024 * 1024)
        .await
        .unwrap();

    let total_time = start.elapsed();
    assert_eq!(body_bytes.len(), large_data.len());

    // Should be able to stream 10MB in under 100ms from memory
    assert!(
        total_time < Duration::from_millis(100),
        "Streaming took {:?}",
        total_time
    );
}

#[tokio::test]
async fn test_error_handling_invalid_range() {
    let mp4_data = create_mock_mp4_data();
    let provider = Arc::new(MockPieceProvider::new(mp4_data.clone()));
    let producer = DirectStreamProducer::new(provider, "video/mp4".to_string());

    // Request range beyond file size
    let request = Request::builder()
        .header("Range", "bytes=10000-20000")
        .body(())
        .unwrap();
    let response = producer.produce_stream(request).await;

    assert_eq!(response.status(), StatusCode::RANGE_NOT_SATISFIABLE);
    assert!(response.headers().contains_key("content-range"));
}

#[tokio::test]
async fn test_concurrent_streaming_requests() {
    let mp4_data = create_mock_mp4_data();
    let provider = Arc::new(MockPieceProvider::new(mp4_data.clone()));

    // Spawn multiple concurrent requests
    let mut handles = vec![];
    for i in 0..5 {
        let provider = provider.clone();
        let handle = tokio::spawn(async move {
            let producer = DirectStreamProducer::new(provider, "video/mp4".to_string());

            // Each request asks for a different range
            let start = i * 100;
            let end = start + 99;
            let request = Request::builder()
                .header("Range", format!("bytes={}-{}", start, end))
                .body(())
                .unwrap();

            let response = producer.produce_stream(request).await;
            assert_eq!(response.status(), StatusCode::PARTIAL_CONTENT);

            let body_bytes = axum::body::to_bytes(response.into_body(), 1024)
                .await
                .unwrap();
            assert_eq!(body_bytes.len(), 100);
        });
        handles.push(handle);
    }

    // All requests should complete successfully
    for handle in handles {
        handle.await.unwrap();
    }
}

#[tokio::test]
async fn test_streaming_with_provider_errors() {
    // Provider that always returns errors
    struct ErrorProvider;

    #[async_trait::async_trait]
    impl PieceProvider for ErrorProvider {
        async fn read_at(&self, _offset: u64, _length: usize) -> Result<Bytes, PieceProviderError> {
            Err(PieceProviderError::StorageError("Disk error".to_string()))
        }

        async fn size(&self) -> u64 {
            1000
        }
    }

    let provider = Arc::new(ErrorProvider);
    let producer = DirectStreamProducer::new(provider, "video/mp4".to_string());

    let request = Request::builder().body(()).unwrap();
    let response = producer.produce_stream(request).await;

    // Should still return OK status, but body will error when read
    assert_eq!(response.status(), StatusCode::OK);

    // Reading the body should encounter an error
    let result = axum::body::to_bytes(response.into_body(), 1024).await;
    assert!(result.is_err());
}
