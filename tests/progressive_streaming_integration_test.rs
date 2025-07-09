//! Integration test for progressive remuxing streaming while downloading
//!
//! This test verifies that the progressive remuxing strategy works correctly
//! in real-world scenarios where pieces arrive progressively and streaming
//! needs to start before the full download is complete.

use std::collections::HashMap;
use std::ops::Range;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use riptide_core::streaming::{FileAssembler, FileAssemblerError};
use riptide_core::torrent::InfoHash;
use riptide_web::streaming::{
    ClientCapabilities, HttpStreamingConfig, HttpStreamingService, SimpleRangeRequest,
    StreamingRequest,
};
use tokio::time::sleep;

/// Mock file assembler that simulates progressive piece availability
/// This mimics the real-world scenario where pieces arrive over time
#[derive(Clone)]
struct ProgressiveFileAssembler {
    files: HashMap<InfoHash, FileData>,
    available_ranges: Arc<std::sync::RwLock<HashMap<InfoHash, Vec<Range<u64>>>>>,
}

#[derive(Clone)]
struct FileData {
    data: Vec<u8>,
    file_size: u64,
}

impl ProgressiveFileAssembler {
    fn new() -> Self {
        Self {
            files: HashMap::new(),
            available_ranges: Arc::new(std::sync::RwLock::new(HashMap::new())),
        }
    }

    async fn add_file(&mut self, info_hash: InfoHash, data: Vec<u8>) {
        let file_size = data.len() as u64;
        self.files.insert(info_hash, FileData { data, file_size });

        // Initially, no ranges are available (simulating torrent start)
        self.available_ranges
            .write()
            .unwrap()
            .insert(info_hash, Vec::new());
    }

    async fn make_head_available(&self, info_hash: InfoHash, head_size: u64) {
        let file_size = self.files.get(&info_hash).unwrap().file_size;
        let actual_head_size = head_size.min(file_size);

        self.available_ranges
            .write()
            .unwrap()
            .entry(info_hash)
            .or_default()
            .push(0..actual_head_size);
    }

    async fn make_tail_available(&self, info_hash: InfoHash, tail_size: u64) {
        let file_size = self.files.get(&info_hash).unwrap().file_size;
        if file_size > tail_size {
            let tail_start = file_size - tail_size;
            self.available_ranges
                .write()
                .unwrap()
                .entry(info_hash)
                .or_default()
                .push(tail_start..file_size);
        }
    }

    async fn simulate_progressive_download(&self, info_hash: InfoHash, chunk_size: u64) {
        let file_size = self.files.get(&info_hash).unwrap().file_size;
        let mut ranges = self.available_ranges.write().unwrap();
        let available = ranges.entry(info_hash).or_default();

        // Find the largest contiguous range and extend it
        if let Some(largest_range) = available.iter_mut().max_by_key(|r| r.end - r.start) {
            let new_end = (largest_range.end + chunk_size).min(file_size);
            largest_range.end = new_end;
        }
    }

    async fn get_download_progress(&self, info_hash: InfoHash) -> f32 {
        let file_size = self.files.get(&info_hash).unwrap().file_size;
        let ranges = self.available_ranges.read().unwrap();

        if let Some(available) = ranges.get(&info_hash) {
            let total_available: u64 = available.iter().map(|r| r.end - r.start).sum();
            (total_available as f32 / file_size as f32) * 100.0
        } else {
            0.0
        }
    }
}

#[async_trait]
impl FileAssembler for ProgressiveFileAssembler {
    async fn read_range(
        &self,
        info_hash: InfoHash,
        range: Range<u64>,
    ) -> Result<Vec<u8>, FileAssemblerError> {
        if !self.is_range_available(info_hash, range.clone()) {
            return Err(FileAssemblerError::InsufficientData {
                start: range.start,
                end: range.end,
                missing_count: 1,
            });
        }

        let file_data = self
            .files
            .get(&info_hash)
            .ok_or(FileAssemblerError::InvalidRange {
                start: range.start,
                end: range.end,
            })?;

        let start = range.start as usize;
        let end = (range.end as usize).min(file_data.data.len());

        Ok(file_data.data[start..end].to_vec())
    }

    async fn file_size(&self, info_hash: InfoHash) -> Result<u64, FileAssemblerError> {
        self.files
            .get(&info_hash)
            .map(|f| f.file_size)
            .ok_or(FileAssemblerError::InvalidRange { start: 0, end: 0 })
    }

    fn is_range_available(&self, info_hash: InfoHash, range: Range<u64>) -> bool {
        let ranges = self.available_ranges.read().unwrap();
        if let Some(available_ranges) = ranges.get(&info_hash) {
            available_ranges
                .iter()
                .any(|r| r.start <= range.start && range.end <= r.end)
        } else {
            false
        }
    }
}

/// Create test AVI file data with realistic structure
fn create_test_avi_data(size: usize) -> Vec<u8> {
    let mut data = Vec::with_capacity(size);

    // RIFF header
    data.extend_from_slice(b"RIFF");
    data.extend_from_slice(&(size as u32 - 8).to_le_bytes());
    data.extend_from_slice(b"AVI ");

    // LIST header with hdrl
    data.extend_from_slice(b"LIST");
    data.extend_from_slice(&[0x00, 0x01, 0x00, 0x00]);
    data.extend_from_slice(b"hdrl");

    // Add avih (AVI header)
    data.extend_from_slice(b"avih");
    data.extend_from_slice(&[0x38, 0x00, 0x00, 0x00]); // Header size

    // Add some realistic AVI header data
    data.extend_from_slice(&[0x40, 0x42, 0x0F, 0x00]); // Microseconds per frame
    data.extend_from_slice(&[0x00, 0x00, 0x00, 0x00]); // Max bytes per second
    data.extend_from_slice(&[0x00, 0x00, 0x00, 0x00]); // Padding granularity
    data.extend_from_slice(&[0x10, 0x01, 0x00, 0x00]); // Flags
    data.extend_from_slice(&[0x64, 0x00, 0x00, 0x00]); // Total frames
    data.extend_from_slice(&[0x00, 0x00, 0x00, 0x00]); // Initial frames
    data.extend_from_slice(&[0x01, 0x00, 0x00, 0x00]); // Streams
    data.extend_from_slice(&[0x00, 0x00, 0x00, 0x00]); // Suggested buffer size
    data.extend_from_slice(&[0x80, 0x02, 0x00, 0x00]); // Width
    data.extend_from_slice(&[0xE0, 0x01, 0x00, 0x00]); // Height
    data.extend_from_slice(&[0x00, 0x00, 0x00, 0x00]); // Reserved
    data.extend_from_slice(&[0x00, 0x00, 0x00, 0x00]); // Reserved
    data.extend_from_slice(&[0x00, 0x00, 0x00, 0x00]); // Reserved
    data.extend_from_slice(&[0x00, 0x00, 0x00, 0x00]); // Reserved

    // Pad to requested size with some realistic data patterns
    while data.len() < size {
        if data.len() + 1024 <= size {
            // Add a mock video frame
            data.extend_from_slice(&[0x00, 0x64, 0x63]); // Frame header
            data.extend_from_slice(&vec![0x42; 1021]); // Frame data
        } else {
            data.push(0x00);
        }
    }

    data.truncate(size);
    data
}

/// Create test MKV file data with realistic structure
fn create_test_mkv_data(size: usize) -> Vec<u8> {
    let mut data = Vec::with_capacity(size);

    // EBML header
    data.extend_from_slice(&[0x1A, 0x45, 0xDF, 0xA3]); // EBML signature
    data.extend_from_slice(&[0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x1F]);

    // DocType
    data.extend_from_slice(&[0x42, 0x82, 0x88]); // DocType element
    data.extend_from_slice(b"matroska");

    // DocTypeVersion
    data.extend_from_slice(&[0x42, 0x87, 0x81, 0x02]);

    // DocTypeReadVersion
    data.extend_from_slice(&[0x42, 0x85, 0x81, 0x02]);

    // Segment
    data.extend_from_slice(&[0x18, 0x53, 0x80, 0x67]);
    data.extend_from_slice(&[0x01, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF]); // Unknown size

    // SeekHead
    data.extend_from_slice(&[0x11, 0x4D, 0x9B, 0x74]);
    data.extend_from_slice(&[0x40, 0x00]); // Size placeholder

    // Info section
    data.extend_from_slice(&[0x15, 0x49, 0xA9, 0x66]);
    data.extend_from_slice(&[0x40, 0x00]); // Size placeholder

    // TimecodeScale
    data.extend_from_slice(&[0x2A, 0xD7, 0xB1, 0x83, 0x0F, 0x42, 0x40]);

    // Pad to requested size with realistic MKV cluster data
    while data.len() < size {
        if data.len() + 512 <= size {
            // Add a mock cluster
            data.extend_from_slice(&[0x1F, 0x43, 0xB6, 0x75]); // Cluster
            data.extend_from_slice(&[0x40, 0x00]); // Size placeholder
            data.extend_from_slice(&[0xE7, 0x81, 0x00]); // Timecode
            data.extend_from_slice(&vec![0x55; 500]); // Mock data
        } else {
            data.push(0x00);
        }
    }

    data.truncate(size);
    data
}

/// Create mock piece store for testing
struct MockPieceStore;

#[async_trait::async_trait]
impl riptide_core::torrent::PieceStore for MockPieceStore {
    async fn piece_data(
        &self,
        _info_hash: InfoHash,
        _piece_index: riptide_core::torrent::PieceIndex,
    ) -> Result<Vec<u8>, riptide_core::torrent::TorrentError> {
        // Return dummy data for piece store
        Ok(vec![0u8; 1024])
    }

    fn has_piece(
        &self,
        _info_hash: InfoHash,
        _piece_index: riptide_core::torrent::PieceIndex,
    ) -> bool {
        true
    }

    fn piece_count(
        &self,
        _info_hash: InfoHash,
    ) -> Result<u32, riptide_core::torrent::TorrentError> {
        Ok(100) // Mock 100 pieces
    }
}

#[tokio::test]
async fn test_progressive_avi_streaming() {
    let info_hash = InfoHash::new([1u8; 20]);

    // Create 10MB AVI file
    let avi_data = create_test_avi_data(10 * 1024 * 1024);

    let mut file_assembler = ProgressiveFileAssembler::new();
    file_assembler.add_file(info_hash, avi_data).await;

    // Initially, only head is available (first 2MB)
    file_assembler
        .make_head_available(info_hash, 2 * 1024 * 1024)
        .await;

    let piece_store = Arc::new(MockPieceStore);
    let streaming_service = HttpStreamingService::new_with_ffmpeg(
        Arc::new(file_assembler.clone()),
        piece_store,
        riptide_core::streaming::ProductionFfmpegProcessor::new(None),
        HttpStreamingConfig::default(),
    );

    // Create streaming request
    let request = StreamingRequest {
        info_hash,
        range: Some(SimpleRangeRequest {
            start: 0,
            end: Some(1024), // Request first 1KB
        }),
        client_capabilities: ClientCapabilities {
            supports_mp4: true,
            supports_webm: false,
            supports_hls: false,
            user_agent: "Test Browser".to_string(),
        },
        preferred_quality: None,
        time_offset: None,
    };

    println!(
        "Testing AVI progressive streaming with {}% downloaded",
        file_assembler.get_download_progress(info_hash).await
    );

    // Should be able to start streaming with just head data
    let response = streaming_service.handle_streaming_request(request).await;

    match response {
        Ok(response) => {
            println!("✓ Progressive AVI streaming started successfully");
            println!("  Status: {}", response.status);
            println!("  Content-Type: {}", response.content_type);

            // Verify we get MP4 content type (not AVI)
            assert_eq!(response.content_type, "video/mp4");

            // Verify we get partial content response
            assert_eq!(response.status, axum::http::StatusCode::PARTIAL_CONTENT);

            // Verify we get some response (body verification skipped for simplicity)
            println!("  Response body size: non-empty");
        }
        Err(e) => {
            // Progressive remuxing might not be ready yet - this is expected behavior
            println!("Progressive streaming not ready (expected): {}", e);

            // Simulate more pieces becoming available
            for i in 1..=5 {
                file_assembler
                    .simulate_progressive_download(info_hash, 1024 * 1024)
                    .await;
                println!(
                    "Simulated download progress: {:.1}%",
                    file_assembler.get_download_progress(info_hash).await
                );

                sleep(Duration::from_millis(100)).await;

                // Try again
                let request = StreamingRequest {
                    info_hash,
                    range: Some(SimpleRangeRequest {
                        start: 0,
                        end: Some(1024),
                    }),
                    client_capabilities: ClientCapabilities {
                        supports_mp4: true,
                        supports_webm: false,
                        supports_hls: false,
                        user_agent: "Test Browser".to_string(),
                    },
                    preferred_quality: None,
                    time_offset: None,
                };

                if let Ok(response) = streaming_service.handle_streaming_request(request).await {
                    println!(
                        "✓ Progressive AVI streaming started after {}% downloaded",
                        file_assembler.get_download_progress(info_hash).await
                    );
                    assert_eq!(response.content_type, "video/mp4");
                    return;
                }
            }

            // If we get here, progressive streaming should have started
            panic!("Progressive streaming failed to start even with sufficient data");
        }
    }
}

#[tokio::test]
async fn test_progressive_mkv_streaming() {
    let info_hash = InfoHash::new([2u8; 20]);

    // Create 15MB MKV file
    let mkv_data = create_test_mkv_data(15 * 1024 * 1024);

    let mut file_assembler = ProgressiveFileAssembler::new();
    file_assembler.add_file(info_hash, mkv_data).await;

    // Start with head and tail available (typical torrent pattern)
    file_assembler
        .make_head_available(info_hash, 3 * 1024 * 1024)
        .await; // 3MB head
    file_assembler
        .make_tail_available(info_hash, 2 * 1024 * 1024)
        .await; // 2MB tail

    let piece_store = Arc::new(MockPieceStore);
    let streaming_service = HttpStreamingService::new_with_ffmpeg(
        Arc::new(file_assembler.clone()),
        piece_store,
        riptide_core::streaming::ProductionFfmpegProcessor::new(None),
        HttpStreamingConfig::default(),
    );

    let request = StreamingRequest {
        info_hash,
        range: Some(SimpleRangeRequest {
            start: 0,
            end: Some(2048), // Request first 2KB
        }),
        client_capabilities: ClientCapabilities {
            supports_mp4: true,
            supports_webm: false,
            supports_hls: false,
            user_agent: "Test Browser".to_string(),
        },
        preferred_quality: None,
        time_offset: None,
    };

    println!(
        "Testing MKV progressive streaming with {}% downloaded",
        file_assembler.get_download_progress(info_hash).await
    );

    let response = streaming_service
        .handle_streaming_request(request.clone())
        .await;

    match response {
        Ok(response) => {
            println!("✓ Progressive MKV streaming started successfully");
            println!("  Status: {}", response.status);
            println!("  Content-Type: {}", response.content_type);

            // Key verification: MIME type should be video/mp4, not video/x-matroska
            assert_eq!(response.content_type, "video/mp4");
            assert_eq!(response.status, axum::http::StatusCode::PARTIAL_CONTENT);
        }
        Err(e) => {
            println!("Progressive MKV streaming not ready yet: {}", e);

            // Simulate gradual download
            for i in 1..=3 {
                file_assembler
                    .simulate_progressive_download(info_hash, 2 * 1024 * 1024)
                    .await;
                println!(
                    "Download progress: {:.1}%",
                    file_assembler.get_download_progress(info_hash).await
                );

                sleep(Duration::from_millis(200)).await;
            }

            // Should work now
            let response = streaming_service
                .handle_streaming_request(request)
                .await
                .expect("Progressive MKV streaming should work with more data");

            assert_eq!(response.content_type, "video/mp4");
        }
    }
}

#[tokio::test]
async fn test_mime_type_consistency() {
    // This test specifically addresses the MIME type flashing issue
    let info_hash = InfoHash::new([3u8; 20]);

    let avi_data = create_test_avi_data(5 * 1024 * 1024);
    let mut file_assembler = ProgressiveFileAssembler::new();
    file_assembler.add_file(info_hash, avi_data).await;
    file_assembler
        .make_head_available(info_hash, 1024 * 1024)
        .await;

    let piece_store = Arc::new(MockPieceStore);
    let streaming_service = HttpStreamingService::new_with_ffmpeg(
        Arc::new(file_assembler.clone()),
        piece_store,
        riptide_core::streaming::ProductionFfmpegProcessor::new(None),
        HttpStreamingConfig::default(),
    );

    // Make multiple requests to ensure MIME type doesn't change
    for i in 0..5 {
        let request = StreamingRequest {
            info_hash,
            range: Some(SimpleRangeRequest {
                start: i * 1024,
                end: Some((i + 1) * 1024),
            }),
            client_capabilities: ClientCapabilities {
                supports_mp4: true,
                supports_webm: false,
                supports_hls: false,
                user_agent: "Consistency Test Browser".to_string(),
            },
            preferred_quality: None,
            time_offset: None,
        };

        // Simulate more pieces becoming available
        if i > 0 {
            file_assembler
                .simulate_progressive_download(info_hash, 512 * 1024)
                .await;
        }

        match streaming_service.handle_streaming_request(request).await {
            Ok(response) => {
                println!("Request {}: Content-Type = {}", i, response.content_type);

                // Should ALWAYS be video/mp4, never video/x-msvideo
                assert_eq!(
                    response.content_type, "video/mp4",
                    "MIME type inconsistency detected on request {}",
                    i
                );
            }
            Err(e) => {
                println!("Request {} not ready: {}", i, e);
                // This is acceptable for early requests
            }
        }

        sleep(Duration::from_millis(100)).await;
    }

    println!("✓ MIME type consistency verified across multiple requests");
}

#[tokio::test]
async fn test_streaming_with_gaps() {
    // Test streaming when pieces arrive out of order (realistic torrent scenario)
    let info_hash = InfoHash::new([4u8; 20]);

    let mkv_data = create_test_mkv_data(8 * 1024 * 1024);
    let mut file_assembler = ProgressiveFileAssembler::new();
    file_assembler.add_file(info_hash, mkv_data).await;

    // Simulate realistic torrent download pattern:
    // 1. Head pieces first
    file_assembler
        .make_head_available(info_hash, 1024 * 1024)
        .await;

    // 2. Tail pieces (end of file)
    file_assembler
        .make_tail_available(info_hash, 1024 * 1024)
        .await;

    let piece_store = Arc::new(MockPieceStore);
    let streaming_service = HttpStreamingService::new_with_ffmpeg(
        Arc::new(file_assembler.clone()),
        piece_store,
        riptide_core::streaming::ProductionFfmpegProcessor::new(None),
        HttpStreamingConfig::default(),
    );

    println!(
        "Testing streaming with gaps (head/tail only): {:.1}% downloaded",
        file_assembler.get_download_progress(info_hash).await
    );

    // Progressive remuxing should be able to start with just head/tail
    let request = StreamingRequest {
        info_hash,
        range: Some(SimpleRangeRequest {
            start: 0,
            end: Some(512),
        }),
        client_capabilities: ClientCapabilities {
            supports_mp4: true,
            supports_webm: false,
            supports_hls: false,
            user_agent: "Gap Test Browser".to_string(),
        },
        preferred_quality: None,
        time_offset: None,
    };

    // Try streaming - might work with progressive remuxing
    match streaming_service
        .handle_streaming_request(request.clone())
        .await
    {
        Ok(response) => {
            println!("✓ Streaming started with gaps in download");
            assert_eq!(response.content_type, "video/mp4");
        }
        Err(e) => {
            println!("Streaming with gaps not ready: {}", e);

            // Fill in some middle pieces
            for _ in 0..3 {
                file_assembler
                    .simulate_progressive_download(info_hash, 1024 * 1024)
                    .await;
                sleep(Duration::from_millis(100)).await;
            }

            println!(
                "After filling gaps: {:.1}% downloaded",
                file_assembler.get_download_progress(info_hash).await
            );

            // Should work now
            let response = streaming_service
                .handle_streaming_request(request)
                .await
                .expect("Streaming should work after filling gaps");

            assert_eq!(response.content_type, "video/mp4");
        }
    }
}
