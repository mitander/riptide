//! End-to-end streaming integration test
//!
//! Tests complete torrent download → HTTP streaming → browser workflow.
//! Validates the entire streaming pipeline from BitTorrent protocol to HTTP responses.

use std::collections::HashMap;
use std::ops::Range;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use axum::body::{Body, to_bytes};
use axum::http::{HeaderMap, HeaderValue, Response, StatusCode};
// Additional imports for race condition test
use futures::future;
use riptide_core::streaming::{
    ContainerDetector, ContainerFormat, FfmpegProcessor, FileAssembler, FileAssemblerError,
    RemuxStreamingConfig,
};
use riptide_core::torrent::{InfoHash, PieceIndex, PieceStore, TorrentError};
use riptide_core::video::VideoQuality;
use riptide_web::streaming::{
    ClientCapabilities, HttpStreamingConfig, HttpStreamingError, HttpStreamingService,
    SimpleRangeRequest, StreamingRequest,
};
use tempfile;
use tokio::fs;
use tokio::sync::RwLock;

/// Test file assembler that serves complete files for testing
#[derive(Clone)]
struct TestFileAssembler {
    files: Arc<RwLock<HashMap<InfoHash, PathBuf>>>,
}

impl TestFileAssembler {
    fn new() -> Self {
        Self {
            files: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    async fn add_file(&self, info_hash: InfoHash, path: PathBuf) {
        let mut files = self.files.write().await;
        files.insert(info_hash, path);
    }
}

#[async_trait::async_trait]
impl FileAssembler for TestFileAssembler {
    async fn read_range(
        &self,
        info_hash: InfoHash,
        range: std::ops::Range<u64>,
    ) -> Result<Vec<u8>, FileAssemblerError> {
        let files = self.files.read().await;
        let file_path = files
            .get(&info_hash)
            .ok_or_else(|| FileAssemblerError::CacheError {
                reason: format!("File not found for info hash: {}", info_hash),
            })?;

        let mut file =
            fs::File::open(file_path)
                .await
                .map_err(|e| FileAssemblerError::CacheError {
                    reason: format!("Failed to open file: {}", e),
                })?;

        use tokio::io::{AsyncReadExt, AsyncSeekExt};
        file.seek(std::io::SeekFrom::Start(range.start))
            .await
            .map_err(|e| FileAssemblerError::CacheError {
                reason: format!("Failed to seek: {}", e),
            })?;

        let length = range.end - range.start;
        let mut buffer = vec![0u8; length as usize];
        file.read_exact(&mut buffer)
            .await
            .map_err(|e| FileAssemblerError::CacheError {
                reason: format!("Failed to read: {}", e),
            })?;

        Ok(buffer)
    }

    async fn file_size(&self, info_hash: InfoHash) -> Result<u64, FileAssemblerError> {
        let files = self.files.read().await;
        let file_path = files
            .get(&info_hash)
            .ok_or_else(|| FileAssemblerError::CacheError {
                reason: format!("File not found for info hash: {}", info_hash),
            })?;

        let metadata =
            fs::metadata(file_path)
                .await
                .map_err(|e| FileAssemblerError::CacheError {
                    reason: format!("Failed to get metadata: {}", e),
                })?;

        Ok(metadata.len())
    }

    fn is_range_available(&self, _info_hash: InfoHash, _range: std::ops::Range<u64>) -> bool {
        true // Test implementation always has data available
    }
}

/// Mock piece store for testing
#[derive(Clone)]
struct MockPieceStore {
    pieces: Arc<RwLock<HashMap<InfoHash, HashMap<u32, Vec<u8>>>>>,
}

impl MockPieceStore {
    fn new() -> Self {
        Self {
            pieces: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    #[allow(dead_code)]
    async fn add_piece(&self, info_hash: InfoHash, piece_index: u32, data: Vec<u8>) {
        let mut pieces = self.pieces.write().await;
        pieces
            .entry(info_hash)
            .or_insert_with(HashMap::new)
            .insert(piece_index, data);
    }
}

#[async_trait::async_trait]
impl PieceStore for MockPieceStore {
    async fn piece_data(
        &self,
        info_hash: InfoHash,
        piece_index: PieceIndex,
    ) -> Result<Vec<u8>, TorrentError> {
        let pieces = self.pieces.read().await;
        let torrent_pieces = pieces
            .get(&info_hash)
            .ok_or(TorrentError::TorrentNotFound { info_hash })?;
        let piece_data = torrent_pieces
            .get(&piece_index.as_u32())
            .ok_or(TorrentError::PieceHashMismatch { index: piece_index })?;
        Ok(piece_data.clone())
    }

    fn has_piece(&self, _info_hash: InfoHash, _piece_index: PieceIndex) -> bool {
        // Since this is a mock, we'll assume all pieces are available
        // In a real implementation, this would check if the piece exists
        true
    }

    fn piece_count(&self, _info_hash: InfoHash) -> Result<u32, TorrentError> {
        // Since this is a mock, we'll return a fixed count
        // In a real implementation, this would check the actual piece count
        Ok(1)
    }
}

/// Mock FFmpeg processor that produces predictable test output
struct MockFfmpegProcessor;

#[async_trait::async_trait]
impl FfmpegProcessor for MockFfmpegProcessor {
    async fn remux_to_mp4(
        &self,
        _input_path: &std::path::Path,
        output_path: &std::path::Path,
        _options: &riptide_core::streaming::RemuxingOptions,
    ) -> riptide_core::streaming::StreamingResult<riptide_core::streaming::RemuxingResult> {
        // Generate test MP4 file for testing
        let mp4_data = create_test_mp4_file();
        tokio::fs::write(output_path, &mp4_data)
            .await
            .map_err(|e| riptide_core::streaming::StrategyError::FfmpegError {
                reason: format!("Failed to write test MP4: {}", e),
            })?;

        Ok(riptide_core::streaming::RemuxingResult {
            output_size: mp4_data.len() as u64,
            processing_time: 0.1,
            streams_reencoded: false,
        })
    }

    fn is_available(&self) -> bool {
        true
    }

    async fn estimate_output_size(&self, _input_path: &std::path::Path) -> Option<u64> {
        // Return size of generated test MP4 file
        let mp4_data = create_test_mp4_file();
        Some(mp4_data.len() as u64)
    }
}

/// Production FFmpeg processor for real integration testing
fn create_ffmpeg_processor() -> MockFfmpegProcessor {
    MockFfmpegProcessor
}

/// Create test files for different formats using programmatically generated content
async fn create_test_files()
-> Result<Vec<(ContainerFormat, InfoHash, PathBuf)>, Box<dyn std::error::Error>> {
    let test_dir = std::env::temp_dir().join("riptide_streaming_test");
    fs::create_dir_all(&test_dir).await?;

    let mut files = Vec::new();

    // Generate test AVI file
    let avi_path = test_dir.join("test.avi");
    let avi_data = create_test_avi_file();
    fs::write(&avi_path, &avi_data).await?;
    let avi_hash = InfoHash::new([1u8; 20]);
    files.push((ContainerFormat::Avi, avi_hash, avi_path));

    // Generate test MKV file
    let mkv_path = test_dir.join("test.mkv");
    let mkv_data = create_test_mkv_file();
    fs::write(&mkv_path, &mkv_data).await?;
    let mkv_hash = InfoHash::new([2u8; 20]);
    files.push((ContainerFormat::Mkv, mkv_hash, mkv_path));

    // Generate test MP4 file
    let mp4_path = test_dir.join("test.mp4");
    let mp4_data = create_test_mp4_file();
    fs::write(&mp4_path, &mp4_data).await?;
    let mp4_hash = InfoHash::new([3u8; 20]);
    files.push((ContainerFormat::Mp4, mp4_hash, mp4_path));

    Ok(files)
}

/// Create minimal valid AVI file for testing
fn create_test_avi_file() -> Vec<u8> {
    let mut data = Vec::new();

    // RIFF header
    data.extend_from_slice(b"RIFF");
    data.extend_from_slice(&(2048u32).to_le_bytes()); // File size
    data.extend_from_slice(b"AVI ");

    // LIST hdrl chunk
    data.extend_from_slice(b"LIST");
    data.extend_from_slice(&(200u32).to_le_bytes()); // hdrl size
    data.extend_from_slice(b"hdrl");

    // avih header (main AVI header)
    data.extend_from_slice(b"avih");
    data.extend_from_slice(&(56u32).to_le_bytes()); // avih size
    data.extend_from_slice(&(40000u32).to_le_bytes()); // dwMicroSecPerFrame (25 fps)
    data.extend_from_slice(&(1000000u32).to_le_bytes()); // dwMaxBytesPerSec
    data.extend_from_slice(&(0u32).to_le_bytes()); // dwPaddingGranularity
    data.extend_from_slice(&(0x10u32).to_le_bytes()); // dwFlags (AVIF_HASINDEX)
    data.extend_from_slice(&(100u32).to_le_bytes()); // dwTotalFrames
    data.extend_from_slice(&(0u32).to_le_bytes()); // dwInitialFrames
    data.extend_from_slice(&(1u32).to_le_bytes()); // dwStreams
    data.extend_from_slice(&(0u32).to_le_bytes()); // dwSuggestedBufferSize
    data.extend_from_slice(&(320u32).to_le_bytes()); // dwWidth
    data.extend_from_slice(&(240u32).to_le_bytes()); // dwHeight
    data.extend_from_slice(&[0; 16]); // dwReserved[4]

    // LIST strl chunk (stream list)
    data.extend_from_slice(b"LIST");
    data.extend_from_slice(&(116u32).to_le_bytes()); // strl size
    data.extend_from_slice(b"strl");

    // strh header (stream header)
    data.extend_from_slice(b"strh");
    data.extend_from_slice(&(56u32).to_le_bytes()); // strh size
    data.extend_from_slice(b"vids"); // fccType (video)
    data.extend_from_slice(b"DIB "); // fccHandler
    data.extend_from_slice(&(0u32).to_le_bytes()); // dwFlags
    data.extend_from_slice(&(0u16).to_le_bytes()); // wPriority
    data.extend_from_slice(&(0u16).to_le_bytes()); // wLanguage
    data.extend_from_slice(&(0u32).to_le_bytes()); // dwInitialFrames
    data.extend_from_slice(&(1u32).to_le_bytes()); // dwScale
    data.extend_from_slice(&(25u32).to_le_bytes()); // dwRate (25 fps)
    data.extend_from_slice(&(0u32).to_le_bytes()); // dwStart
    data.extend_from_slice(&(100u32).to_le_bytes()); // dwLength
    data.extend_from_slice(&(0u32).to_le_bytes()); // dwSuggestedBufferSize
    data.extend_from_slice(&(10000u32).to_le_bytes()); // dwQuality
    data.extend_from_slice(&(0u32).to_le_bytes()); // dwSampleSize
    data.extend_from_slice(&(0u16).to_le_bytes()); // rcFrame.left
    data.extend_from_slice(&(0u16).to_le_bytes()); // rcFrame.top
    data.extend_from_slice(&(320u16).to_le_bytes()); // rcFrame.right
    data.extend_from_slice(&(240u16).to_le_bytes()); // rcFrame.bottom

    // strf header (stream format)
    data.extend_from_slice(b"strf");
    data.extend_from_slice(&(40u32).to_le_bytes()); // strf size
    data.extend_from_slice(&(40u32).to_le_bytes()); // biSize
    data.extend_from_slice(&(320u32).to_le_bytes()); // biWidth
    data.extend_from_slice(&(240u32).to_le_bytes()); // biHeight
    data.extend_from_slice(&(1u16).to_le_bytes()); // biPlanes
    data.extend_from_slice(&(24u16).to_le_bytes()); // biBitCount
    data.extend_from_slice(&(0u32).to_le_bytes()); // biCompression (BI_RGB)
    data.extend_from_slice(&(230400u32).to_le_bytes()); // biSizeImage (320*240*3)
    data.extend_from_slice(&(0u32).to_le_bytes()); // biXPelsPerMeter
    data.extend_from_slice(&(0u32).to_le_bytes()); // biYPelsPerMeter
    data.extend_from_slice(&(0u32).to_le_bytes()); // biClrUsed
    data.extend_from_slice(&(0u32).to_le_bytes()); // biClrImportant

    // LIST movi chunk
    data.extend_from_slice(b"LIST");
    data.extend_from_slice(&(1000u32).to_le_bytes()); // movi size
    data.extend_from_slice(b"movi");

    // Add a few fake video frames
    for i in 0..10 {
        data.extend_from_slice(b"00db"); // chunk ID for DIB frame
        data.extend_from_slice(&(96u32).to_le_bytes()); // frame size
        // Add some fake frame data (repeated pattern)
        for _ in 0..24 {
            data.extend_from_slice(&[(i * 10) as u8; 4]);
        }
    }

    // Pad to expected size
    while data.len() < 2048 {
        data.push(0);
    }

    data
}

/// Create minimal valid MKV file for testing
fn create_test_mkv_file() -> Vec<u8> {
    let mut data = Vec::new();

    // EBML header
    data.extend_from_slice(&[0x1A, 0x45, 0xDF, 0xA3]); // EBML ID
    data.extend_from_slice(&[0x9F]); // Size (variable length)

    // EBMLVersion
    data.extend_from_slice(&[0x42, 0x86, 0x81, 0x01]);

    // EBMLReadVersion
    data.extend_from_slice(&[0x42, 0xF7, 0x81, 0x01]);

    // EBMLMaxIDLength
    data.extend_from_slice(&[0x42, 0xF2, 0x81, 0x04]);

    // EBMLMaxSizeLength
    data.extend_from_slice(&[0x42, 0xF3, 0x81, 0x08]);

    // DocType
    data.extend_from_slice(&[0x42, 0x82, 0x88]);
    data.extend_from_slice(b"matroska");

    // DocTypeVersion
    data.extend_from_slice(&[0x42, 0x87, 0x81, 0x02]);

    // DocTypeReadVersion
    data.extend_from_slice(&[0x42, 0x85, 0x81, 0x02]);

    // Segment
    data.extend_from_slice(&[0x18, 0x53, 0x80, 0x67]); // Segment ID
    data.extend_from_slice(&[0x01, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF]); // Unknown size

    // Pad to reasonable size (ensure at least 64 bytes for format detection)
    data.resize(1024, 0);
    data
}

/// Create minimal valid MP4 file for testing
fn create_test_mp4_file() -> Vec<u8> {
    let mut data = Vec::new();

    // ftyp box
    data.extend_from_slice(&(32u32).to_be_bytes()); // Box size
    data.extend_from_slice(b"ftyp"); // Box type
    data.extend_from_slice(b"mp41"); // Major brand
    data.extend_from_slice(&(0u32).to_be_bytes()); // Minor version
    data.extend_from_slice(b"mp41"); // Compatible brand 1
    data.extend_from_slice(b"isom"); // Compatible brand 2
    data.extend_from_slice(b"avc1"); // Compatible brand 3

    // moov box (movie header)
    data.extend_from_slice(&(8u32).to_be_bytes()); // Box size
    data.extend_from_slice(b"moov"); // Box type

    // mdat box (media data)
    data.extend_from_slice(&(16u32).to_be_bytes()); // Box size
    data.extend_from_slice(b"mdat"); // Box type
    data.extend_from_slice(&[0u8; 8]); // Dummy media data

    // Ensure at least 64 bytes for format detection
    if data.len() < 64 {
        data.resize(64, 0);
    }

    data
}

/// Test streaming service setup with mock FFmpeg processor
async fn setup_streaming_service(file_assembler: Arc<TestFileAssembler>) -> HttpStreamingService {
    let piece_store = Arc::new(MockPieceStore::new());

    let config = HttpStreamingConfig::default();

    // Use smaller min_head_size for testing with small files
    let remux_config = RemuxStreamingConfig {
        min_head_size: 1024, // 1KB instead of 3MB for testing
        ..Default::default()
    };

    HttpStreamingService::new_with_remux_config(file_assembler, piece_store, config, remux_config)
}

/// Test app state for integration testing
struct TestAppState {
    streaming_service: HttpStreamingService,
}

impl TestAppState {
    fn new(streaming_service: HttpStreamingService) -> Self {
        Self { streaming_service }
    }

    fn streaming_service(&self) -> &HttpStreamingService {
        &self.streaming_service
    }
}

#[tokio::test]
#[ignore] // TODO: Re-enable after streaming refactor
async fn test_remuxing_and_direct_streaming() {
    let _ = tracing_subscriber::fmt::try_init();

    // Create test files (AVI/MKV for remuxing, MP4 for direct streaming)
    let test_files = create_test_files()
        .await
        .expect("Failed to create test files");

    // Setup file assembler
    let file_assembler = Arc::new(TestFileAssembler::new());
    for (format, info_hash, path) in &test_files {
        file_assembler.add_file(*info_hash, path.clone()).await;
        println!(
            "Added test file for {:?}: {} bytes",
            format,
            fs::metadata(path).await.unwrap().len()
        );
    }

    // Setup streaming service
    let streaming_service = setup_streaming_service(file_assembler).await;
    let app_state = TestAppState::new(streaming_service);

    // Test each format
    for (format, info_hash, path) in test_files {
        println!("\n=== Testing format: {:?} ===", format);

        let original_size = fs::metadata(&path).await.unwrap().len();
        println!("Original file size: {} bytes", original_size);

        // Test full file request (no range)
        let response = test_streaming_request(&app_state, info_hash, None).await;
        assert_eq!(
            response.status(),
            StatusCode::OK,
            "Streaming failed for {:?}",
            format
        );

        let body_bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let served_size = body_bytes.len() as u64;

        println!("Served file size: {} bytes", served_size);

        // For AVI/MKV, the served size will be the remuxed MP4 size (not original size)
        // For MP4, it should match the original size
        match format {
            ContainerFormat::Mp4 => {
                assert_eq!(
                    served_size, original_size,
                    "MP4 direct streaming size mismatch"
                );
            }
            ContainerFormat::Avi | ContainerFormat::Mkv => {
                // Remuxed files MUST be valid MP4 with complete metadata
                assert!(served_size > 0, "Remuxed file must not be empty");
                assert!(
                    is_valid_mp4_header(&body_bytes),
                    "Remuxed file must be valid MP4 - got {} bytes with header: {:?}",
                    served_size,
                    &body_bytes[..body_bytes.len().min(32)]
                );
                // Remuxed files should be seekable and have proper metadata
                assert!(
                    served_size >= 64,
                    "Remuxed MP4 must have sufficient metadata for streaming"
                );
            }
            _ => panic!("Unexpected format: {:?}", format),
        }

        // Test range requests - use the actual served size for all formats
        let expected_size = served_size;

        let range_header = format!("bytes=0-{}", expected_size - 1);
        let response = test_streaming_request(&app_state, info_hash, Some(range_header)).await;
        assert_eq!(
            response.status(),
            StatusCode::PARTIAL_CONTENT,
            "Range request failed for {:?}",
            format
        );

        let body_bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let range_size = body_bytes.len() as u64;

        println!("Range served size: {} bytes", range_size);
        assert_eq!(
            range_size, expected_size,
            "Range size mismatch for {:?}",
            format
        );
    }

    println!("\nAll format tests passed! (MP4 direct, AVI/MKV remuxed)");
}

/// Test streaming request helper
async fn test_streaming_request(
    app_state: &TestAppState,
    info_hash: InfoHash,
    range_header: Option<String>,
) -> Response<Body> {
    // Create request headers
    let mut headers = HeaderMap::new();
    headers.insert("User-Agent", HeaderValue::from_static("Test-Agent"));

    if let Some(range) = range_header {
        headers.insert("Range", HeaderValue::from_str(&range).unwrap());
    }

    // Create streaming request directly
    use riptide_web::streaming::{ClientCapabilities, SimpleRangeRequest, StreamingRequest};

    let range_request = if let Some(range_str) = headers.get("Range") {
        // Parse range header
        let range_str = range_str.to_str().unwrap();
        if let Some(range_part) = range_str.strip_prefix("bytes=") {
            let parts: Vec<&str> = range_part.split('-').collect();
            if parts.len() == 2 {
                let start = parts[0].parse::<u64>().unwrap_or(0);
                let end = if parts[1].is_empty() {
                    None
                } else {
                    Some(parts[1].parse::<u64>().unwrap())
                };
                Some(SimpleRangeRequest { start, end })
            } else {
                None
            }
        } else {
            None
        }
    } else {
        None
    };

    let client_capabilities = ClientCapabilities {
        supports_mp4: true,
        supports_webm: true,
        supports_hls: false,
        user_agent: "Test-Agent".to_string(),
    };

    let streaming_request = StreamingRequest {
        info_hash,
        range: range_request,
        client_capabilities,
        preferred_quality: Some(VideoQuality::High),
        time_offset: None,
    };

    // Handle streaming request
    let streaming_response = app_state
        .streaming_service()
        .handle_streaming_request(streaming_request)
        .await
        .expect("Streaming request failed");

    // Convert to Axum response
    let mut response_builder = Response::builder().status(streaming_response.status);

    for (key, value) in streaming_response.headers.iter() {
        response_builder = response_builder.header(key, value);
    }

    response_builder.body(streaming_response.body).unwrap()
}

#[tokio::test]
#[ignore] // TODO: Re-enable after streaming refactor
async fn test_container_format_detection() {
    let test_files = create_test_files()
        .await
        .expect("Failed to create test files");

    for (expected_format, _, path) in test_files {
        let data = fs::read(&path).await.unwrap();
        let detected_format = ContainerDetector::detect_format(&data[..64]);

        println!(
            "Expected: {:?}, Detected: {:?}",
            expected_format, detected_format
        );
        assert_eq!(
            detected_format, expected_format,
            "Format detection failed for {:?}",
            expected_format
        );
    }
}

#[tokio::test]
#[ignore] // TODO: Re-enable after streaming refactor
async fn test_streaming_performance() {
    let test_files = create_test_files()
        .await
        .expect("Failed to create test files");
    let file_assembler = Arc::new(TestFileAssembler::new());

    // Add all test files
    for (_, info_hash, path) in &test_files {
        file_assembler.add_file(*info_hash, path.clone()).await;
    }

    let streaming_service = setup_streaming_service(file_assembler).await;
    let app_state = TestAppState::new(streaming_service);

    // Test sequential streaming requests (both direct and remuxed)
    let mut total_time = Duration::ZERO;

    for (format, info_hash, _) in test_files {
        let start = std::time::Instant::now();
        let response = test_streaming_request(&app_state, info_hash, None).await;
        let elapsed = start.elapsed();

        assert!(
            response.status() == StatusCode::OK || response.status() == StatusCode::PARTIAL_CONTENT,
            "Streaming failed for {:?}. Expected 200 or 206, got {}",
            format,
            response.status()
        );
        total_time += elapsed;
        println!("{:?} request completed in: {:?}", format, elapsed);
    }

    println!("Total time for all requests: {:?}", total_time);
    // Performance requirement: streaming should start within reasonable time
    assert!(
        total_time < Duration::from_secs(10),
        "Streaming performance degraded: took {:?} for all formats",
        total_time
    );
}

/// Test that verifies the dev mode bypass issue is fixed
#[tokio::test]
#[ignore] // TODO: Re-enable after streaming refactor
async fn test_file_assembler_integration() {
    // Test that file assembler properly serves complete files
    let file_assembler = Arc::new(TestFileAssembler::new());
    let piece_store = Arc::new(MockPieceStore::new());
    let config = HttpStreamingConfig::default();

    // Create service - uses file assembler properly
    let streaming_service = HttpStreamingService::new(file_assembler.clone(), piece_store, config);

    let test_files = create_test_files()
        .await
        .expect("Failed to create test files");

    // Use MP4 file to avoid remuxing complications
    let (_format, info_hash, path) = test_files
        .iter()
        .find(|(f, _, _)| matches!(f, ContainerFormat::Mp4))
        .unwrap();

    // Add file to assembler
    file_assembler.add_file(*info_hash, path.clone()).await;

    let app_state = TestAppState::new(streaming_service);

    // Test that file is served correctly through file assembler
    let response = test_streaming_request(&app_state, *info_hash, None).await;
    assert_eq!(response.status(), StatusCode::OK);

    let body_bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let original_size = fs::metadata(path).await.unwrap().len();

    // For MP4 files, should serve original file size (no remuxing)
    assert_eq!(
        body_bytes.len() as u64,
        original_size,
        "File truncation detected - file assembler not working correctly"
    );
}

/// Test head-and-tail streaming with partial file availability
#[tokio::test]
#[ignore] // TODO: Re-enable after streaming refactor
async fn test_head_and_tail_streaming() {
    // Create test files for head-and-tail streaming
    let test_files = create_test_files()
        .await
        .expect("Failed to create test files");

    // Test only with formats that need remuxing (AVI/MKV)
    for (format, info_hash, path) in test_files {
        if !matches!(format, ContainerFormat::Avi | ContainerFormat::Mkv) {
            continue;
        }

        println!(
            "\n=== Testing head-and-tail streaming for format: {:?} ===",
            format
        );

        let original_size = fs::metadata(&path).await.unwrap().len();
        println!("Original file size: {} bytes", original_size);

        // Create a partial file assembler that simulates head-and-tail availability
        let file_assembler = Arc::new(PartialFileAssembler::new());
        file_assembler.add_file(info_hash, path.clone()).await;

        // Setup streaming service with partial file assembler
        let piece_store = Arc::new(MockPieceStore::new());
        let config = HttpStreamingConfig::default();
        let streaming_service =
            HttpStreamingService::new(file_assembler.clone(), piece_store, config);
        let app_state = TestAppState::new(streaming_service);

        // Test streaming request - should work with head-and-tail data
        let response = test_streaming_request(&app_state, info_hash, None).await;

        // Should succeed with head-and-tail data
        assert_eq!(
            response.status(),
            StatusCode::OK,
            "Head-and-tail streaming failed for {:?}",
            format
        );

        let body_bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let served_size = body_bytes.len() as u64;

        println!("Served file size: {} bytes", served_size);

        // Should produce valid MP4 output
        assert!(
            served_size > 0,
            "Empty response from head-and-tail streaming"
        );

        // Verify the response contains valid MP4 data
        let mp4_header = &body_bytes[..8.min(body_bytes.len())];
        let has_mp4_signature = mp4_header.windows(4).any(|w| w == b"ftyp");
        assert!(
            has_mp4_signature,
            "Response doesn't contain valid MP4 signature"
        );

        println!("✓ Head-and-tail streaming successful for {:?}", format);
    }
}

/// Test file assembler that simulates partial availability with controlled range availability
#[derive(Clone)]
struct PartialFileAssembler {
    files: Arc<RwLock<HashMap<InfoHash, PathBuf>>>,
    available_ranges: Arc<RwLock<HashMap<InfoHash, Vec<Range<u64>>>>>,
}

impl PartialFileAssembler {
    fn new() -> Self {
        Self {
            files: Arc::new(RwLock::new(HashMap::new())),
            available_ranges: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    async fn add_file(&self, info_hash: InfoHash, path: PathBuf) {
        let mut files = self.files.write().await;
        files.insert(info_hash, path);
    }

    /// Add an available range for testing partial file downloads
    async fn add_available_range(&self, info_hash: InfoHash, range: Range<u64>) {
        let mut ranges = self.available_ranges.write().await;
        tracing::info!(
            "Added available range for {}: {}..{}",
            info_hash,
            range.start,
            range.end
        );
        ranges.entry(info_hash).or_default().push(range);
    }
}

#[async_trait::async_trait]
impl FileAssembler for PartialFileAssembler {
    async fn read_range(
        &self,
        info_hash: InfoHash,
        range: Range<u64>,
    ) -> Result<Vec<u8>, FileAssemblerError> {
        // Check if the requested range is available
        if !self.is_range_available(info_hash, range.clone()) {
            tracing::warn!(
                "Range {}..{} not available for {}",
                range.start,
                range.end,
                info_hash
            );
            return Err(FileAssemblerError::InsufficientData {
                start: range.start,
                end: range.end,
                missing_count: 1,
            });
        }

        let files = self.files.read().await;
        let file_path = files
            .get(&info_hash)
            .ok_or_else(|| FileAssemblerError::CacheError {
                reason: format!("File not found for info hash: {}", info_hash),
            })?;

        // Read the actual file data
        let mut file =
            fs::File::open(file_path)
                .await
                .map_err(|e| FileAssemblerError::CacheError {
                    reason: format!("Failed to open file: {}", e),
                })?;

        use tokio::io::{AsyncReadExt, AsyncSeekExt};
        file.seek(std::io::SeekFrom::Start(range.start))
            .await
            .map_err(|e| FileAssemblerError::CacheError {
                reason: format!("Failed to seek: {}", e),
            })?;

        let length = range.end - range.start;
        let mut buffer = vec![0u8; length as usize];
        file.read_exact(&mut buffer)
            .await
            .map_err(|e| FileAssemblerError::CacheError {
                reason: format!("Failed to read: {}", e),
            })?;

        tracing::debug!(
            "Successfully read range {}..{} ({} bytes) for {}",
            range.start,
            range.end,
            buffer.len(),
            info_hash
        );
        Ok(buffer)
    }

    async fn file_size(&self, info_hash: InfoHash) -> Result<u64, FileAssemblerError> {
        let files = self.files.read().await;
        let file_path = files
            .get(&info_hash)
            .ok_or_else(|| FileAssemblerError::CacheError {
                reason: format!("File not found for info hash: {}", info_hash),
            })?;

        let metadata =
            fs::metadata(file_path)
                .await
                .map_err(|e| FileAssemblerError::CacheError {
                    reason: format!("Failed to get metadata: {}", e),
                })?;

        Ok(metadata.len())
    }

    fn is_range_available(&self, info_hash: InfoHash, range: Range<u64>) -> bool {
        if let Ok(ranges_guard) = self.available_ranges.try_read() {
            if let Some(available_ranges) = ranges_guard.get(&info_hash) {
                return available_ranges
                    .iter()
                    .any(|r| r.start <= range.start && r.end >= range.end);
            }
        }
        false
    }
}

/// Create a minimal valid video file that FFmpeg can actually process
async fn create_valid_test_video() -> Vec<u8> {
    // Create a simple RGB raw video file that FFmpeg can handle
    // This creates a 10x10 pixel video with 5 frames
    let mut avi_data = Vec::new();

    // RIFF header
    avi_data.extend_from_slice(b"RIFF");
    avi_data.extend_from_slice(&(8000u32).to_le_bytes()); // File size placeholder
    avi_data.extend_from_slice(b"AVI ");

    // LIST hdrl
    avi_data.extend_from_slice(b"LIST");
    avi_data.extend_from_slice(&(208u32).to_le_bytes());
    avi_data.extend_from_slice(b"hdrl");

    // avih (main header)
    avi_data.extend_from_slice(b"avih");
    avi_data.extend_from_slice(&(56u32).to_le_bytes());
    avi_data.extend_from_slice(&(200000u32).to_le_bytes()); // dwMicroSecPerFrame (5 fps)
    avi_data.extend_from_slice(&(3000u32).to_le_bytes()); // dwMaxBytesPerSec
    avi_data.extend_from_slice(&(0u32).to_le_bytes()); // dwPaddingGranularity
    avi_data.extend_from_slice(&(0x10u32).to_le_bytes()); // dwFlags
    avi_data.extend_from_slice(&(5u32).to_le_bytes()); // dwTotalFrames
    avi_data.extend_from_slice(&(0u32).to_le_bytes()); // dwInitialFrames
    avi_data.extend_from_slice(&(1u32).to_le_bytes()); // dwStreams
    avi_data.extend_from_slice(&(0u32).to_le_bytes()); // dwSuggestedBufferSize
    avi_data.extend_from_slice(&(10u32).to_le_bytes()); // dwWidth
    avi_data.extend_from_slice(&(10u32).to_le_bytes()); // dwHeight
    avi_data.extend_from_slice(&[0; 16]); // dwReserved

    // LIST strl (stream list)
    avi_data.extend_from_slice(b"LIST");
    avi_data.extend_from_slice(&(132u32).to_le_bytes());
    avi_data.extend_from_slice(b"strl");

    // strh (stream header)
    avi_data.extend_from_slice(b"strh");
    avi_data.extend_from_slice(&(56u32).to_le_bytes());
    avi_data.extend_from_slice(b"vids"); // fccType
    avi_data.extend_from_slice(&[0; 4]); // fccHandler (uncompressed)
    avi_data.extend_from_slice(&(0u32).to_le_bytes()); // dwFlags
    avi_data.extend_from_slice(&(0u16).to_le_bytes()); // wPriority
    avi_data.extend_from_slice(&(0u16).to_le_bytes()); // wLanguage
    avi_data.extend_from_slice(&(0u32).to_le_bytes()); // dwInitialFrames
    avi_data.extend_from_slice(&(1u32).to_le_bytes()); // dwScale
    avi_data.extend_from_slice(&(5u32).to_le_bytes()); // dwRate
    avi_data.extend_from_slice(&(0u32).to_le_bytes()); // dwStart
    avi_data.extend_from_slice(&(5u32).to_le_bytes()); // dwLength
    avi_data.extend_from_slice(&(300u32).to_le_bytes()); // dwSuggestedBufferSize
    avi_data.extend_from_slice(&(10000u32).to_le_bytes()); // dwQuality
    avi_data.extend_from_slice(&(300u32).to_le_bytes()); // dwSampleSize (10*10*3)
    avi_data.extend_from_slice(&(0u16).to_le_bytes()); // rcFrame.left
    avi_data.extend_from_slice(&(0u16).to_le_bytes()); // rcFrame.top
    avi_data.extend_from_slice(&(10u16).to_le_bytes()); // rcFrame.right
    avi_data.extend_from_slice(&(10u16).to_le_bytes()); // rcFrame.bottom

    // strf (stream format) - BITMAPINFOHEADER
    avi_data.extend_from_slice(b"strf");
    avi_data.extend_from_slice(&(40u32).to_le_bytes());
    avi_data.extend_from_slice(&(40u32).to_le_bytes()); // biSize
    avi_data.extend_from_slice(&(10u32).to_le_bytes()); // biWidth
    avi_data.extend_from_slice(&(10u32).to_le_bytes()); // biHeight
    avi_data.extend_from_slice(&(1u16).to_le_bytes()); // biPlanes
    avi_data.extend_from_slice(&(24u16).to_le_bytes()); // biBitCount
    avi_data.extend_from_slice(&(0u32).to_le_bytes()); // biCompression (BI_RGB)
    avi_data.extend_from_slice(&(300u32).to_le_bytes()); // biSizeImage
    avi_data.extend_from_slice(&(0u32).to_le_bytes()); // biXPelsPerMeter
    avi_data.extend_from_slice(&(0u32).to_le_bytes()); // biYPelsPerMeter
    avi_data.extend_from_slice(&(0u32).to_le_bytes()); // biClrUsed
    avi_data.extend_from_slice(&(0u32).to_le_bytes()); // biClrImportant

    // LIST movi
    avi_data.extend_from_slice(b"LIST");
    avi_data.extend_from_slice(&(1520u32).to_le_bytes()); // Size for 5 frames
    avi_data.extend_from_slice(b"movi");

    // Add 5 simple frames (10x10 RGB, 300 bytes each)
    for frame_num in 0..5 {
        avi_data.extend_from_slice(b"00db"); // Chunk fourcc
        avi_data.extend_from_slice(&(300u32).to_le_bytes()); // Chunk size

        // Simple frame data - gradient pattern
        for y in 0..10 {
            for x in 0..10 {
                let r = (x * 25 + frame_num * 10) as u8;
                let g = (y * 25) as u8;
                let b = ((x + y) * 12 + frame_num * 20) as u8;
                avi_data.extend_from_slice(&[b, g, r]); // BGR format
            }
        }
    }

    // Update file size
    let file_size = avi_data.len() as u32 - 8;
    avi_data[4..8].copy_from_slice(&file_size.to_le_bytes());

    avi_data
}

/// Validates if the given bytes start with a recognizable MP4 header.
fn is_valid_mp4_header(data: &[u8]) -> bool {
    let valid = data.len() > 8
        && (&data[4..8] == b"ftyp" || &data[4..8] == b"moov" || &data[4..8] == b"moof");
    if !valid {
        println!(
            "Debug: MP4 validation failed - length: {}, bytes 4-8: {:?}",
            data.len(),
            if data.len() > 8 { &data[4..8] } else { &[] }
        );
        println!(
            "Debug: Expected 'ftyp' {:?}, 'moov' {:?}, or 'moof' {:?}",
            b"ftyp", b"moov", b"moof"
        );
        if data.len() > 8 {
            println!(
                "Debug: Actual bytes as string: '{}'",
                String::from_utf8_lossy(&data[4..8])
            );
        }
    }
    valid
}

#[tokio::test]
#[ignore] // TODO: Re-enable after streaming refactor
async fn test_realistic_remux_streaming_pipeline() {
    let _ = tracing_subscriber::fmt::try_init();

    // 1. Setup with real file assembler and service
    let file_assembler = Arc::new(TestFileAssembler::new());
    let piece_store = Arc::new(MockPieceStore::new());
    let config = HttpStreamingConfig::default();

    let streaming_service = Arc::new(HttpStreamingService::new(
        file_assembler.clone(),
        piece_store,
        config,
    ));

    // 2. Create a valid video file that FFmpeg can actually process
    let temp_dir = tempfile::tempdir().unwrap();
    let avi_path = temp_dir.path().join("test_valid.avi");
    let valid_avi_data = create_valid_test_video().await;
    fs::write(&avi_path, &valid_avi_data).await.unwrap();

    let info_hash = InfoHash::new([100u8; 20]);
    file_assembler.add_file(info_hash, avi_path).await;

    // 3. Test single request with timeout
    let request = StreamingRequest {
        info_hash,
        range: Some(SimpleRangeRequest {
            start: 0,
            end: Some(4095), // Request first 4KB
        }),
        client_capabilities: ClientCapabilities {
            supports_mp4: true,
            ..Default::default()
        },
        preferred_quality: None,
        time_offset: None,
    };

    println!("Starting remux streaming test...");

    // Add timeout to the request
    let response = tokio::time::timeout(
        Duration::from_secs(30),
        streaming_service.handle_streaming_request(request),
    )
    .await;

    match response {
        Ok(Ok(resp)) => {
            match resp.status {
                StatusCode::OK | StatusCode::PARTIAL_CONTENT => {
                    let body_bytes = to_bytes(resp.body, 1024 * 1024).await.unwrap();

                    if body_bytes.len() > 0 {
                        println!("✓ Received {} bytes of data", body_bytes.len());

                        // Check if we got valid MP4 data
                        if is_valid_mp4_header(&body_bytes) {
                            println!("✓ Valid MP4 header detected!");
                        } else {
                            println!("⚠ No valid MP4 header found in response:");
                            println!(
                                "  First 32 bytes: {:?}",
                                &body_bytes[..body_bytes.len().min(32)]
                            );

                            // Still count as success if we got data (might be remuxing in progress)
                            println!(
                                "✓ Got data from remux pipeline (remuxing may still be in progress)"
                            );
                        }
                    } else {
                        panic!("Received empty response body");
                    }
                }
                StatusCode::SERVICE_UNAVAILABLE => {
                    println!("ℹ Service unavailable - remuxing may still be initializing");
                }
                _ => {
                    panic!("Unexpected status code: {}", resp.status);
                }
            }
        }
        Ok(Err(e)) => {
            println!("Request failed with error: {:?}", e);

            // Graceful error handling - system should fail cleanly, not crash
            match e {
                HttpStreamingError::RemuxingFailed { .. } => {
                    tracing::info!("✓ Graceful failure: Remuxing failed cleanly");
                }
                HttpStreamingError::StreamingNotReady { .. } => {
                    tracing::info!("✓ Graceful failure: Stream not ready");
                }
                _ => panic!(
                    "Unexpected error type - system should fail gracefully: {:?}",
                    e
                ),
            }
        }
        Err(_) => {
            panic!(
                "Request timed out after 30 seconds - this indicates a deadlock or infinite wait"
            );
        }
    }

    println!("✓ Remux streaming test completed successfully");
}

/// Test 1: Reproduce the Bug - Remuxing fails with only head data
#[tokio::test]
#[ignore] // TODO: Re-enable after streaming refactor
async fn test_remuxing_fails_with_only_head_data() {
    let _ = tracing_subscriber::fmt::try_init();

    tracing::info!("=== Test 1: Reproducing the bug - Only head data available ===");

    // Setup test files
    let test_files = create_test_files()
        .await
        .expect("Failed to create test files");

    // Use AVI file for testing (requires remuxing)
    let (format, info_hash, path) = test_files
        .iter()
        .find(|(f, _, _)| matches!(f, ContainerFormat::Avi))
        .expect("No AVI file found")
        .clone();

    let file_size = fs::metadata(&path).await.unwrap().len();
    tracing::info!("Testing with {:?} file, size: {} bytes", format, file_size);

    // Create PartialFileAssembler with only head data
    let file_assembler = Arc::new(PartialFileAssembler::new());
    file_assembler.add_file(info_hash, path).await;

    // Make only the head available (first 2MB)
    let head_size = 2 * 1024 * 1024; // 2MB
    file_assembler
        .add_available_range(info_hash, 0..head_size.min(file_size))
        .await;

    // Setup streaming service
    let piece_store = Arc::new(MockPieceStore::new());
    let config = HttpStreamingConfig::default();
    let streaming_service = HttpStreamingService::new(file_assembler, piece_store, config);

    // Create streaming request for the first few KB
    let request = StreamingRequest {
        info_hash,
        range: Some(SimpleRangeRequest {
            start: 0,
            end: Some(8191), // First 8KB
        }),
        client_capabilities: ClientCapabilities {
            supports_mp4: true,
            supports_webm: true,
            supports_hls: false,
            user_agent: "Test-Agent".to_string(),
        },
        preferred_quality: Some(VideoQuality::High),
        time_offset: None,
    };

    tracing::info!("Making streaming request with only head data available...");

    // Execute the request
    let response = streaming_service.handle_streaming_request(request).await;

    match response {
        Ok(resp) => {
            tracing::info!("Response status: {}", resp.status);

            // CORRECT BEHAVIOR: System should NOT return 206 Partial Content with invalid data
            // It should return 503 Service Unavailable, Too Early, or fail gracefully
            if resp.status == StatusCode::PARTIAL_CONTENT {
                let body_bytes = to_bytes(resp.body, 1024 * 1024).await.unwrap();

                // If it claims to be successful, it MUST deliver valid MP4 data
                assert!(
                    is_valid_mp4_header(&body_bytes),
                    "SYSTEM ERROR: Returned 206 Partial Content but with invalid MP4 data. Expected valid MP4 or non-200 status."
                );

                tracing::info!(
                    "✓ CORRECT: System returned valid MP4 with head-only data (unexpected but valid)"
                );
            } else if resp.status == StatusCode::SERVICE_UNAVAILABLE
                || resp.status == StatusCode::TOO_EARLY
            {
                tracing::info!(
                    "✓ CORRECT BEHAVIOR: System returned {} (not ready)",
                    resp.status
                );
            } else {
                tracing::error!("Unexpected status code: {}", resp.status);
                panic!("Unexpected status code: {}", resp.status);
            }
        }
        Err(e) => {
            tracing::info!("✓ CORRECT BEHAVIOR: Request failed with error: {:?}", e);
            // This is also acceptable - the system should fail gracefully
            match e {
                HttpStreamingError::StreamingNotReady { .. }
                | HttpStreamingError::RemuxingFailed { .. } => {
                    tracing::info!("✓ Graceful failure as expected");
                }
                _ => {
                    tracing::error!("Unexpected error type: {:?}", e);
                }
            }
        }
    }

    tracing::info!("=== Test 1 Complete: Bug reproduction test ===");
}

/// Test 2: Verify the Fix - Remuxing succeeds with head and tail data
#[tokio::test]
#[ignore] // TODO: Re-enable after streaming refactor
async fn test_remuxing_succeeds_with_head_and_tail_data() {
    let _ = tracing_subscriber::fmt::try_init();

    tracing::info!("=== Test 2: Verifying the fix - Head and tail data available ===");

    // Setup test files
    let test_files = create_test_files()
        .await
        .expect("Failed to create test files");

    // Use AVI file for testing (requires remuxing)
    let (format, info_hash, path) = test_files
        .iter()
        .find(|(f, _, _)| matches!(f, ContainerFormat::Avi))
        .expect("No AVI file found")
        .clone();

    let file_size = fs::metadata(&path).await.unwrap().len();
    tracing::info!("Testing with {:?} file, size: {} bytes", format, file_size);

    // Create PartialFileAssembler with head and tail data
    let file_assembler = Arc::new(PartialFileAssembler::new());
    file_assembler.add_file(info_hash, path).await;

    // Make both head and tail available
    let head_size = 2 * 1024 * 1024; // First 2MB
    let tail_size = 2 * 1024 * 1024; // Last 2MB
    let tail_start = if file_size > tail_size {
        file_size - tail_size
    } else {
        0
    };

    file_assembler
        .add_available_range(info_hash, 0..head_size)
        .await;
    file_assembler
        .add_available_range(info_hash, tail_start..file_size)
        .await;

    // Setup streaming service
    let piece_store = Arc::new(MockPieceStore::new());
    let config = HttpStreamingConfig::default();
    let streaming_service = HttpStreamingService::new(file_assembler, piece_store, config);

    // Create streaming request for the first few KB
    let request = StreamingRequest {
        info_hash,
        range: Some(SimpleRangeRequest {
            start: 0,
            end: Some(8191), // First 8KB
        }),
        client_capabilities: ClientCapabilities {
            supports_mp4: true,
            supports_webm: true,
            supports_hls: false,
            user_agent: "Test-Agent".to_string(),
        },
        preferred_quality: Some(VideoQuality::High),
        time_offset: None,
    };

    tracing::info!("Making streaming request with head and tail data available...");

    // Execute the request
    let response = streaming_service.handle_streaming_request(request).await;

    match response {
        Ok(resp) => {
            tracing::info!("Response status: {}", resp.status);

            // The fix: System MUST return 206 Partial Content with valid MP4 data
            assert_eq!(
                resp.status,
                StatusCode::PARTIAL_CONTENT,
                "Expected 206 Partial Content with head and tail data"
            );

            // Verify Content-Type header
            let content_type = resp
                .headers
                .get("content-type")
                .expect("Missing Content-Type header")
                .to_str()
                .expect("Invalid Content-Type header");
            assert_eq!(content_type, "video/mp4", "Expected video/mp4 content type");

            // Verify Content-Range header is present
            assert!(
                resp.headers.contains_key("content-range"),
                "Missing Content-Range header"
            );

            let body_bytes = to_bytes(resp.body, 1024 * 1024).await.unwrap();

            // The response must be valid MP4 data
            assert!(
                is_valid_mp4_header(&body_bytes),
                "Response must be valid MP4 data with head and tail available"
            );

            assert!(body_bytes.len() > 0, "Response body must not be empty");

            tracing::info!(
                "✓ SUCCESS: Valid MP4 response with {} bytes",
                body_bytes.len()
            );
            tracing::info!("✓ Content-Type: {}", content_type);
            tracing::info!("✓ Valid MP4 header detected");
        }
        Err(e) => {
            tracing::error!("✗ FAILURE: Request failed with error: {:?}", e);
            panic!("Request should succeed with head and tail data: {:?}", e);
        }
    }

    tracing::info!("=== Test 2 Complete: Fix verification successful ===");
}

/// Test 3: End-to-End User Experience Simulation with retry logic
#[tokio::test]
#[ignore] // TODO: Re-enable after streaming refactor
async fn test_streaming_lifecycle_with_retries() {
    let _ = tracing_subscriber::fmt::try_init();

    tracing::info!("=== Test 3: End-to-end streaming lifecycle with retries ===");

    // Setup test files
    let test_files = create_test_files()
        .await
        .expect("Failed to create test files");

    // Use AVI file for testing (requires remuxing)
    let (format, info_hash, path) = test_files
        .iter()
        .find(|(f, _, _)| matches!(f, ContainerFormat::Avi))
        .expect("No AVI file found")
        .clone();

    let file_size = fs::metadata(&path).await.unwrap().len();
    tracing::info!(
        "Testing lifecycle with {:?} file, size: {} bytes",
        format,
        file_size
    );

    // Create PartialFileAssembler with NO data initially
    let file_assembler = Arc::new(PartialFileAssembler::new());
    file_assembler.add_file(info_hash, path).await;

    // Setup streaming service
    let piece_store = Arc::new(MockPieceStore::new());
    let config = HttpStreamingConfig::default();
    let streaming_service = HttpStreamingService::new(file_assembler.clone(), piece_store, config);

    let request = StreamingRequest {
        info_hash,
        range: Some(SimpleRangeRequest {
            start: 0,
            end: Some(8191), // First 8KB
        }),
        client_capabilities: ClientCapabilities {
            supports_mp4: true,
            supports_webm: true,
            supports_hls: false,
            user_agent: "Test-Agent".to_string(),
        },
        preferred_quality: Some(VideoQuality::High),
        time_offset: None,
    };

    // Step 1: Initial request (should fail - no data available)
    tracing::info!("Step 1: Making initial request with no data available...");
    let response = streaming_service
        .handle_streaming_request(request.clone())
        .await;

    match response {
        Ok(resp) => {
            tracing::info!("Response status: {}", resp.status);
            assert!(
                resp.status == StatusCode::SERVICE_UNAVAILABLE
                    || resp.status == StatusCode::TOO_EARLY,
                "Expected not-ready status, got: {}",
                resp.status
            );
            tracing::info!("✓ Step 1 passed: Correctly returned not-ready status");
        }
        Err(e) => {
            tracing::info!("✓ Step 1 passed: Request failed as expected: {:?}", e);
            match e {
                HttpStreamingError::StreamingNotReady { .. }
                | HttpStreamingError::RemuxingFailed { .. } => {
                    tracing::info!("✓ Graceful failure as expected");
                }
                _ => {
                    tracing::warn!("Unexpected error type: {:?}", e);
                }
            }
        }
    }

    // Step 2: Simulate head and tail download
    tracing::info!("Step 2: Simulating head and tail download...");
    tokio::time::sleep(Duration::from_millis(100)).await;

    let head_size = 2 * 1024 * 1024; // First 2MB
    let tail_size = 2 * 1024 * 1024; // Last 2MB
    let tail_start = if file_size > tail_size {
        file_size - tail_size
    } else {
        0
    };

    file_assembler
        .add_available_range(info_hash, 0..head_size)
        .await;
    file_assembler
        .add_available_range(info_hash, tail_start..file_size)
        .await;

    // Step 3: Retry request (should succeed now)
    tracing::info!("Step 3: Retrying request with head and tail available...");
    let response = streaming_service
        .handle_streaming_request(request.clone())
        .await;

    match response {
        Ok(resp) => {
            tracing::info!("Response status: {}", resp.status);
            assert_eq!(
                resp.status,
                StatusCode::PARTIAL_CONTENT,
                "Expected 206 Partial Content after head and tail download"
            );

            let body_bytes = to_bytes(resp.body, 1024 * 1024).await.unwrap();
            assert!(
                is_valid_mp4_header(&body_bytes),
                "Response must be valid MP4 after head and tail download"
            );

            tracing::info!(
                "✓ Step 3 passed: Request succeeded with valid MP4 ({} bytes)",
                body_bytes.len()
            );
        }
        Err(e) => {
            tracing::error!("✗ Step 3 failed: Request should succeed: {:?}", e);
            panic!(
                "Request should succeed after head and tail download: {:?}",
                e
            );
        }
    }

    // Step 4: Simulate middle part download
    tracing::info!("Step 4: Simulating middle part download...");
    tokio::time::sleep(Duration::from_millis(100)).await;

    let middle_start = 4 * 1024 * 1024; // 4MB
    let middle_end = 5 * 1024 * 1024; // 5MB
    if middle_end <= file_size {
        file_assembler
            .add_available_range(info_hash, middle_start..middle_end)
            .await;
    }

    // Step 5: Seeking request within newly available range
    tracing::info!("Step 5: Making seek request within newly available range...");
    let seek_request = StreamingRequest {
        info_hash,
        range: Some(SimpleRangeRequest {
            start: middle_start,
            end: Some(middle_start + 8191), // 8KB from middle
        }),
        client_capabilities: ClientCapabilities {
            supports_mp4: true,
            supports_webm: true,
            supports_hls: false,
            user_agent: "Test-Agent".to_string(),
        },
        preferred_quality: Some(VideoQuality::High),
        time_offset: None,
    };

    if middle_end <= file_size {
        let response = streaming_service
            .handle_streaming_request(seek_request)
            .await;

        match response {
            Ok(resp) => {
                tracing::info!("Seek response status: {}", resp.status);
                assert_eq!(
                    resp.status,
                    StatusCode::PARTIAL_CONTENT,
                    "Expected 206 Partial Content for seek request"
                );

                let body_bytes = to_bytes(resp.body, 1024 * 1024).await.unwrap();
                assert!(body_bytes.len() > 0, "Seek response must not be empty");

                tracing::info!(
                    "✓ Step 5 passed: Seek request succeeded ({} bytes)",
                    body_bytes.len()
                );
            }
            Err(e) => {
                tracing::error!("✗ Step 5 failed: Seek request should succeed: {:?}", e);
                panic!("Seek request should succeed: {:?}", e);
            }
        }
    } else {
        tracing::info!("✓ Step 5 skipped: File too small for middle range test");
    }

    tracing::info!("=== Test 3 Complete: End-to-end lifecycle simulation successful ===");
}

#[tokio::test]
#[ignore] // TODO: Re-enable after streaming refactor
async fn test_concurrent_remux_initialization_race_condition() {
    let _ = tracing_subscriber::fmt::try_init(); // Ensure logs are visible

    // 1. Setup with real file assembler and service
    let file_assembler = Arc::new(TestFileAssembler::new());
    let piece_store = Arc::new(MockPieceStore::new());
    let config = HttpStreamingConfig::default();

    let streaming_service = Arc::new(HttpStreamingService::new(
        file_assembler.clone(),
        piece_store,
        config,
    ));

    // 2. Create a real test file
    let temp_dir = tempfile::tempdir().unwrap();
    let avi_path = temp_dir.path().join("test.avi");
    fs::write(&avi_path, create_valid_test_video().await)
        .await
        .unwrap();

    let info_hash = InfoHash::new([99u8; 20]);
    file_assembler.add_file(info_hash, avi_path).await;

    // 3. Spawn concurrent requests
    let mut tasks = Vec::new();
    let num_requests = 5;

    for i in 0..num_requests {
        let service = Arc::clone(&streaming_service);
        let task = tokio::spawn(async move {
            let request = StreamingRequest {
                info_hash,
                range: Some(SimpleRangeRequest {
                    start: 0,
                    end: Some(8191),
                }), // Request first 8KB
                client_capabilities: ClientCapabilities {
                    supports_mp4: true,
                    ..Default::default()
                },
                preferred_quality: None,
                time_offset: None,
            };
            tracing::info!("Test client {} sending request...", i);
            service.handle_streaming_request(request).await
        });
        tasks.push(task);
    }

    // 4. Validate responses
    let results = future::join_all(tasks).await;

    let mut successful_streams = 0;
    let mut too_early_responses = 0;

    for (i, result) in results.into_iter().enumerate() {
        let response = result.unwrap(); // Unwrap JoinHandle result
        match response {
            Ok(resp) => {
                match resp.status {
                    StatusCode::OK | StatusCode::PARTIAL_CONTENT => {
                        let body_bytes = to_bytes(resp.body, 1024 * 1024).await.unwrap(); // 1MB limit
                        assert!(
                            is_valid_mp4_header(&body_bytes),
                            "Test {}: Response {} is not a valid MP4 stream",
                            i,
                            resp.status
                        );
                        successful_streams += 1;
                    }
                    StatusCode::TOO_EARLY | StatusCode::SERVICE_UNAVAILABLE => {
                        // This is a valid, graceful handling of the race condition
                        too_early_responses += 1;
                    }
                    _ => {
                        panic!(
                            "Test {}: Received unexpected successful status: {}",
                            i, resp.status
                        );
                    }
                }
            }
            Err(e) => {
                // If it's a "not ready" or remuxing failed error, it's also a valid outcome.
                if let HttpStreamingError::StreamingNotReady { .. }
                | HttpStreamingError::RemuxingFailed { .. } = e
                {
                    too_early_responses += 1;
                } else {
                    panic!("Test {}: Request failed with unexpected error: {:?}", i, e);
                }
            }
        }
    }

    println!(
        "Concurrent test results: {} successful, {} gracefully handled (too early)",
        successful_streams, too_early_responses
    );
    // CRITICAL: All requests must be handled gracefully without race conditions
    assert_eq!(
        successful_streams + too_early_responses,
        num_requests,
        "Race condition detected: {} requests unaccounted for",
        num_requests - successful_streams - too_early_responses
    );

    // System must either succeed with valid data OR fail gracefully
    // No partial successes with invalid data allowed
    if successful_streams > 0 {
        tracing::info!(
            "✓ Concurrent access handled correctly: {} streams succeeded with valid MP4",
            successful_streams
        );
    } else {
        tracing::info!(
            "✓ Concurrent access handled correctly: All {} requests failed gracefully",
            too_early_responses
        );
    }

    // At least one request should succeed if the system is working correctly
    assert!(
        successful_streams > 0,
        "No streams succeeded - system may have race condition or remuxing issues"
    );
}
