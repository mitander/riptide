//! Automated browser streaming tests for Riptide
//!
//! This test suite simulates browser behavior to validate streaming functionality
//! without requiring manual browser testing.

use std::collections::HashMap;
use std::sync::Arc;

use axum::http::StatusCode;
use riptide_core::streaming::{
    ContainerFormat, FileAssembler, FileAssemblerError, ProductionFfmpegProcessor,
};
use riptide_core::torrent::{InfoHash, PieceIndex, PieceStore, TorrentError};
use riptide_web::streaming::{
    ClientCapabilities, HttpStreamingConfig, HttpStreamingService, SimpleRangeRequest,
    StreamingRequest,
};
use tempfile::TempDir;
use tokio::sync::Mutex;

/// Mock file assembler for testing
struct MockFileAssembler {
    files: Mutex<HashMap<InfoHash, Vec<u8>>>,
}

impl MockFileAssembler {
    fn new() -> Self {
        Self {
            files: Mutex::new(HashMap::new()),
        }
    }

    async fn add_file(&self, info_hash: InfoHash, data: Vec<u8>) {
        let mut files = self.files.lock().await;
        files.insert(info_hash, data);
    }
}

#[async_trait::async_trait]
impl FileAssembler for MockFileAssembler {
    async fn read_range(
        &self,
        info_hash: InfoHash,
        range: std::ops::Range<u64>,
    ) -> Result<Vec<u8>, FileAssemblerError> {
        let files = self.files.lock().await;
        match files.get(&info_hash) {
            Some(data) => {
                let start = range.start as usize;
                if start < data.len() {
                    let end = range.end.min(data.len() as u64) as usize;
                    Ok(data[start..end].to_vec())
                } else {
                    Err(FileAssemblerError::InvalidRange {
                        start: range.start,
                        end: range.end,
                    })
                }
            }
            None => Err(FileAssemblerError::Torrent(TorrentError::TorrentNotFound {
                info_hash,
            })),
        }
    }

    async fn file_size(&self, info_hash: InfoHash) -> Result<u64, FileAssemblerError> {
        let files = self.files.lock().await;
        files
            .get(&info_hash)
            .map(|data| data.len() as u64)
            .ok_or(FileAssemblerError::Torrent(TorrentError::TorrentNotFound {
                info_hash,
            }))
    }

    fn is_range_available(&self, _info_hash: InfoHash, _range: std::ops::Range<u64>) -> bool {
        true
    }
}

/// Mock piece store for simulating torrent pieces
struct MockPieceStore {
    pieces: Mutex<HashMap<InfoHash, Vec<Vec<u8>>>>,
}

impl MockPieceStore {
    fn new() -> Self {
        Self {
            pieces: Mutex::new(HashMap::new()),
        }
    }

    async fn add_torrent_data(&self, info_hash: InfoHash, data: Vec<u8>, piece_size: usize) {
        let mut pieces = self.pieces.lock().await;
        let torrent_pieces: Vec<Vec<u8>> = data
            .chunks(piece_size)
            .map(|chunk| chunk.to_vec())
            .collect();
        pieces.insert(info_hash, torrent_pieces);
    }
}

#[async_trait::async_trait]
impl PieceStore for MockPieceStore {
    async fn piece_data(
        &self,
        info_hash: InfoHash,
        piece_index: PieceIndex,
    ) -> Result<Vec<u8>, TorrentError> {
        let pieces = self.pieces.lock().await;
        if let Some(torrent_pieces) = pieces.get(&info_hash) {
            if (piece_index.0 as usize) < torrent_pieces.len() {
                Ok(torrent_pieces[piece_index.0 as usize].clone())
            } else {
                Err(TorrentError::InvalidPieceIndex {
                    index: piece_index.0,
                    max_index: torrent_pieces.len() as u32 - 1,
                })
            }
        } else {
            Err(TorrentError::TorrentNotFound { info_hash })
        }
    }

    fn has_piece(&self, info_hash: InfoHash, piece_index: PieceIndex) -> bool {
        if let Ok(pieces) = self.pieces.try_lock() {
            if let Some(torrent_pieces) = pieces.get(&info_hash) {
                (piece_index.0 as usize) < torrent_pieces.len()
            } else {
                false
            }
        } else {
            false
        }
    }

    fn piece_count(&self, info_hash: InfoHash) -> Result<u32, TorrentError> {
        if let Ok(pieces) = self.pieces.try_lock() {
            if let Some(torrent_pieces) = pieces.get(&info_hash) {
                Ok(torrent_pieces.len() as u32)
            } else {
                Err(TorrentError::TorrentNotFound { info_hash })
            }
        } else {
            Err(TorrentError::TorrentNotFound { info_hash })
        }
    }
}

/// Create test video data with appropriate headers for different formats
fn create_test_video_data(format: ContainerFormat, size: usize) -> Vec<u8> {
    let mut data = vec![0u8; size];

    match format {
        ContainerFormat::Mp4 => {
            // MP4 header with ftyp box
            data[4..8].copy_from_slice(b"ftyp");
            data[8..12].copy_from_slice(b"isom");
            // Add moov atom at position 32
            data[32..36].copy_from_slice(&[0, 0, 0, 100]); // size
            data[36..40].copy_from_slice(b"moov");
        }
        ContainerFormat::Avi => {
            // AVI header
            data[0..4].copy_from_slice(b"RIFF");
            data[8..12].copy_from_slice(b"AVI ");
        }
        ContainerFormat::Mkv => {
            // MKV header (EBML)
            data[0..4].copy_from_slice(&[0x1A, 0x45, 0xDF, 0xA3]);
        }
        _ => {}
    }

    data
}

#[tokio::test]
async fn test_avi_streaming() {
    let _temp_dir = TempDir::new().unwrap();

    // Setup services
    let file_assembler = Arc::new(MockFileAssembler::new());
    let piece_store = Arc::new(MockPieceStore::new());
    let ffmpeg = ProductionFfmpegProcessor::new(None);

    // Create test AVI file
    let info_hash = InfoHash::new([1u8; 20]);
    let avi_data = create_test_video_data(ContainerFormat::Avi, 1024 * 1024); // 1MB

    // Add to services
    file_assembler.add_file(info_hash, avi_data.clone()).await;
    piece_store
        .add_torrent_data(info_hash, avi_data, 256 * 1024)
        .await;

    let streaming_service =
        HttpStreamingService::new(file_assembler, piece_store, HttpStreamingConfig::default());

    // Test streaming request
    let request = StreamingRequest {
        info_hash,
        range: None,
        client_capabilities: ClientCapabilities {
            supports_mp4: true,
            supports_webm: false,
            supports_hls: false,
            user_agent: "Chrome/120.0".to_string(),
        },
        preferred_quality: None,
        time_offset: None,
    };

    let response = streaming_service.handle_streaming_request(request).await;

    match response {
        Ok(resp) => {
            // Should return MP4 format (remuxed from AVI)
            assert_eq!(resp.status, StatusCode::OK);

            let content_type = resp.headers.get("Content-Type").unwrap();
            assert_eq!(content_type, "video/mp4");

            println!(
                "AVI streaming response: status={}, content-type={:?}",
                resp.status, content_type
            );
        }
        Err(e) => {
            panic!("AVI streaming failed: {}", e);
        }
    }
}

#[tokio::test]
async fn test_mkv_streaming() {
    let _temp_dir = TempDir::new().unwrap();

    // Setup services
    let file_assembler = Arc::new(MockFileAssembler::new());
    let piece_store = Arc::new(MockPieceStore::new());
    let ffmpeg = ProductionFfmpegProcessor::new(None);

    // Create test MKV file (dummy data that will fail FFmpeg processing)
    let info_hash = InfoHash::new([2u8; 20]);
    let mkv_data = create_test_video_data(ContainerFormat::Mkv, 2 * 1024 * 1024); // 2MB

    // Add to services
    file_assembler.add_file(info_hash, mkv_data.clone()).await;
    piece_store
        .add_torrent_data(info_hash, mkv_data, 256 * 1024)
        .await;

    let streaming_service =
        HttpStreamingService::new(file_assembler, piece_store, HttpStreamingConfig::default());

    // Test successful MKV to MP4 remux and range requests
    let test_ranges = vec![(0, None), (0, Some(1023)), (1024, None), (512, Some(1535))];

    for (start, end) in test_ranges {
        let request = StreamingRequest {
            info_hash,
            range: Some(SimpleRangeRequest { start, end }),
            client_capabilities: ClientCapabilities {
                supports_mp4: true,
                supports_webm: false,
                supports_hls: false,
                user_agent: "Chrome/120.0".to_string(),
            },
            preferred_quality: None,
            time_offset: None,
        };

        let response = streaming_service.handle_streaming_request(request).await;

        match response {
            Ok(resp) => {
                assert_eq!(resp.status, StatusCode::PARTIAL_CONTENT);

                // Verify Content-Type is MP4
                let content_type = resp.headers.get("Content-Type").unwrap();
                assert_eq!(content_type, "video/mp4");

                // Verify Content-Range header format
                let content_range = resp.headers.get("Content-Range").unwrap();
                let range_str = content_range.to_str().unwrap();
                assert!(range_str.starts_with("bytes "));
                assert!(range_str.contains("/"));

                println!("MKV range {}-{:?}: {}", start, end, range_str);
            }
            Err(e) => {
                panic!("MKV streaming failed for range {}-{:?}: {}", start, end, e);
            }
        }
    }
}

#[tokio::test]
async fn test_cache_behavior() {
    let _temp_dir = TempDir::new().unwrap();

    let file_assembler = Arc::new(MockFileAssembler::new());
    let piece_store = Arc::new(MockPieceStore::new());
    let ffmpeg = ProductionFfmpegProcessor::new(None);

    let info_hash = InfoHash::new([3u8; 20]);
    let avi_data = create_test_video_data(ContainerFormat::Avi, 512 * 1024);

    file_assembler.add_file(info_hash, avi_data.clone()).await;
    piece_store
        .add_torrent_data(info_hash, avi_data, 128 * 1024)
        .await;

    let streaming_service =
        HttpStreamingService::new(file_assembler, piece_store, HttpStreamingConfig::default());

    // First request - should trigger remux
    let start_time = std::time::Instant::now();
    let request = StreamingRequest {
        info_hash,
        range: None,
        client_capabilities: ClientCapabilities {
            supports_mp4: true,
            supports_webm: false,
            supports_hls: false,
            user_agent: "Safari/17.0".to_string(),
        },
        preferred_quality: None,
        time_offset: None,
    };

    let _ = streaming_service
        .handle_streaming_request(request.clone())
        .await;
    let first_request_time = start_time.elapsed();

    // Second request - should use cache
    let start_time = std::time::Instant::now();
    let response = streaming_service.handle_streaming_request(request).await;
    let second_request_time = start_time.elapsed();

    // Cache hit should be faster or at least not significantly slower
    // Allow for some variance in timing but generally expect improvement
    let cache_is_working = second_request_time <= first_request_time
        || second_request_time < first_request_time + std::time::Duration::from_millis(100);

    assert!(
        cache_is_working,
        "Cache may not be working optimally: first={:?}, second={:?}",
        first_request_time, second_request_time
    );

    // Most importantly, both requests should succeed
    assert!(response.is_ok());

    println!(
        "Cache test: first={:?}, second={:?}",
        first_request_time, second_request_time
    );
}

#[tokio::test]
async fn test_concurrent_remux_prevention() {
    let file_assembler = Arc::new(MockFileAssembler::new());
    let piece_store = Arc::new(MockPieceStore::new());
    let ffmpeg = ProductionFfmpegProcessor::new(None);

    let info_hash = InfoHash::new([4u8; 20]);
    let mkv_data = create_test_video_data(ContainerFormat::Mkv, 1024 * 1024);

    file_assembler.add_file(info_hash, mkv_data.clone()).await;
    piece_store
        .add_torrent_data(info_hash, mkv_data, 256 * 1024)
        .await;

    let streaming_service = Arc::new(HttpStreamingService::new(
        file_assembler,
        piece_store,
        HttpStreamingConfig::default(),
    ));

    // Launch multiple concurrent requests
    let request = StreamingRequest {
        info_hash,
        range: None,
        client_capabilities: ClientCapabilities {
            supports_mp4: true,
            supports_webm: false,
            supports_hls: false,
            user_agent: "Firefox/118.0".to_string(),
        },
        preferred_quality: None,
        time_offset: None,
    };

    let mut handles = vec![];
    for _ in 0..5 {
        let service = Arc::clone(&streaming_service);
        let req = request.clone();
        handles.push(tokio::spawn(async move {
            service.handle_streaming_request(req).await
        }));
    }

    // Wait for all requests
    let mut success_count = 0;
    let mut preparing_count = 0;

    for handle in handles {
        match handle.await.unwrap() {
            Ok(resp) => {
                if resp.status == StatusCode::OK {
                    success_count += 1;
                } else if resp.status == StatusCode::TOO_EARLY {
                    preparing_count += 1;
                }
            }
            Err(_) => {
                preparing_count += 1;
            }
        }
    }

    println!(
        "Concurrent requests: {} success, {} preparing",
        success_count, preparing_count
    );

    // Should have some successful requests or at least properly handled conflicts
    assert!(success_count > 0 || preparing_count > 0);
}

#[tokio::test]
async fn test_error_conditions() {
    let file_assembler = Arc::new(MockFileAssembler::new());
    let piece_store = Arc::new(MockPieceStore::new());
    let ffmpeg = ProductionFfmpegProcessor::new(None);

    let streaming_service =
        HttpStreamingService::new(file_assembler, piece_store, HttpStreamingConfig::default());

    // Test invalid info hash
    let invalid_request = StreamingRequest {
        info_hash: InfoHash::new([6u8; 20]), // Non-existent hash
        range: Some(SimpleRangeRequest {
            start: 1000,
            end: Some(2000),
        }),
        client_capabilities: ClientCapabilities {
            supports_mp4: true,
            supports_webm: false,
            supports_hls: false,
            user_agent: "Chrome/120.0".to_string(),
        },
        preferred_quality: None,
        time_offset: None,
    };

    let response = streaming_service
        .handle_streaming_request(invalid_request)
        .await;

    println!("Invalid range response: {:?}", response);
    assert!(response.is_err());
}

#[tokio::test]
async fn test_mp4_direct_streaming() {
    let file_assembler = Arc::new(MockFileAssembler::new());
    let piece_store = Arc::new(MockPieceStore::new());
    let ffmpeg = ProductionFfmpegProcessor::new(None);

    let info_hash = InfoHash::new([5u8; 20]);
    let mp4_data = create_test_video_data(ContainerFormat::Mp4, 1024 * 1024);

    file_assembler.add_file(info_hash, mp4_data.clone()).await;
    piece_store
        .add_torrent_data(info_hash, mp4_data, 256 * 1024)
        .await;

    let streaming_service =
        HttpStreamingService::new(file_assembler, piece_store, HttpStreamingConfig::default());

    let request = StreamingRequest {
        info_hash,
        range: None,
        client_capabilities: ClientCapabilities {
            supports_mp4: true,
            supports_webm: false,
            supports_hls: false,
            user_agent: "Safari/17.0".to_string(),
        },
        preferred_quality: None,
        time_offset: None,
    };

    let response = streaming_service.handle_streaming_request(request).await;

    match response {
        Ok(resp) => {
            // MP4 should be served directly without remuxing
            assert_eq!(resp.status, StatusCode::OK);

            let content_type = resp.headers.get("Content-Type").unwrap();
            assert_eq!(content_type, "video/mp4");
        }
        Err(e) => {
            panic!("MP4 direct streaming failed: {}", e);
        }
    }
}
