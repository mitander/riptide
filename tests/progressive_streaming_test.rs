//! Comprehensive tests for progressive remuxing streaming functionality

use std::collections::HashMap;
use std::ops::Range;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use riptide_core::streaming::{
    FileAssembler, FileAssemblerError, ProgressiveRemuxingConfig, ProgressiveRemuxingStrategy,
    SimulationFfmpegProcessor, StreamingStrategy,
};
use riptide_core::torrent::InfoHash;
use tempfile::tempdir;
use tokio::time::sleep;

/// Mock file assembler that simulates progressive piece availability
struct ProgressiveFileAssembler {
    files: HashMap<InfoHash, FileData>,
    available_ranges: HashMap<InfoHash, Vec<Range<u64>>>,
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
            available_ranges: HashMap::new(),
        }
    }

    fn add_file(&mut self, info_hash: InfoHash, data: Vec<u8>) {
        let file_size = data.len() as u64;
        self.files.insert(info_hash, FileData { data, file_size });

        // Initially, no ranges are available
        self.available_ranges.insert(info_hash, Vec::new());
    }

    fn make_head_available(&mut self, info_hash: InfoHash, head_size: u64) {
        let file_size = self.files.get(&info_hash).unwrap().file_size;
        let actual_head_size = head_size.min(file_size);

        self.available_ranges
            .entry(info_hash)
            .or_default()
            .push(0..actual_head_size);
    }

    fn make_tail_available(&mut self, info_hash: InfoHash, tail_size: u64) {
        let file_size = self.files.get(&info_hash).unwrap().file_size;
        if file_size > tail_size {
            let tail_start = file_size - tail_size;
            self.available_ranges
                .entry(info_hash)
                .or_default()
                .push(tail_start..file_size);
        }
    }

    fn make_range_available(&mut self, info_hash: InfoHash, range: Range<u64>) {
        self.available_ranges
            .entry(info_hash)
            .or_default()
            .push(range);
    }

    fn simulate_progressive_download(&mut self, info_hash: InfoHash, chunk_size: u64) {
        let file_size = self.files.get(&info_hash).unwrap().file_size;
        let ranges = self.available_ranges.entry(info_hash).or_default();

        // Find gaps and fill them progressively
        let mut current_pos = 0u64;
        for range in ranges.iter() {
            if current_pos < range.start {
                // Add a chunk before this range
                let chunk_end = (current_pos + chunk_size).min(range.start);
                ranges.push(current_pos..chunk_end);
                break;
            }
            current_pos = range.end;
        }

        // Add chunk at the end if we haven't reached file size
        if current_pos < file_size {
            let chunk_end = (current_pos + chunk_size).min(file_size);
            ranges.push(current_pos..chunk_end);
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
        if let Some(available_ranges) = self.available_ranges.get(&info_hash) {
            available_ranges
                .iter()
                .any(|r| r.start <= range.start && range.end <= r.end)
        } else {
            false
        }
    }
}

/// Create test MKV file data with realistic structure
fn create_test_mkv_data(size: usize) -> Vec<u8> {
    let mut data = Vec::with_capacity(size);

    // EBML header
    data.extend_from_slice(&[0x1A, 0x45, 0xDF, 0xA3]); // EBML signature
    data.extend_from_slice(&[0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x1F]);
    data.extend_from_slice(&[0x42, 0x86, 0x81, 0x01]);
    data.extend_from_slice(b"matroska"); // DocType

    // Pad to requested size
    data.resize(size, 0);

    data
}

/// Create test AVI file data with realistic structure
fn create_test_avi_data(size: usize) -> Vec<u8> {
    let mut data = Vec::with_capacity(size);

    // RIFF header
    data.extend_from_slice(b"RIFF");
    data.extend_from_slice(&(size as u32 - 8).to_le_bytes()); // File size - 8
    data.extend_from_slice(b"AVI ");

    // LIST header
    data.extend_from_slice(b"LIST");
    data.extend_from_slice(&[0x00, 0x01, 0x00, 0x00]); // Size placeholder
    data.extend_from_slice(b"hdrl");

    // Pad to requested size
    data.resize(size, 0);

    data
}

#[tokio::test]
async fn test_progressive_remuxing_creation() {
    let temp_dir = tempdir().unwrap();
    let config = ProgressiveRemuxingConfig {
        temp_dir: temp_dir.path().to_path_buf(),
        min_head_size: 1024 * 1024, // 1MB
        ..Default::default()
    };

    let file_assembler = Arc::new(ProgressiveFileAssembler::new());
    let ffmpeg_processor = SimulationFfmpegProcessor::new();

    let strategy = ProgressiveRemuxingStrategy::new(file_assembler, ffmpeg_processor, config);
    assert!(strategy.is_ok());
}

#[tokio::test]
async fn test_progressive_remuxing_mkv_head_only() {
    let temp_dir = tempdir().unwrap();
    let config = ProgressiveRemuxingConfig {
        temp_dir: temp_dir.path().to_path_buf(),
        min_head_size: 512 * 1024, // 512KB
        piece_wait_timeout: Duration::from_millis(100),
        ..Default::default()
    };

    let mut file_assembler = ProgressiveFileAssembler::new();
    let info_hash = InfoHash::new([1u8; 20]);

    // Create 10MB MKV file
    let mkv_data = create_test_mkv_data(10 * 1024 * 1024);
    file_assembler.add_file(info_hash, mkv_data);

    // Make head available
    file_assembler.make_head_available(info_hash, 1024 * 1024); // 1MB head

    let strategy = ProgressiveRemuxingStrategy::new(
        Arc::new(file_assembler),
        SimulationFfmpegProcessor::new(),
        config,
    )
    .unwrap();

    // Should be able to get file size
    let file_size = strategy.file_size(info_hash).await.unwrap();
    assert_eq!(file_size, 10 * 1024 * 1024);

    // Should return MP4 format
    let format = strategy.container_format(info_hash).await.unwrap();
    assert_eq!(format, riptide_core::streaming::ContainerFormat::Mp4);
}

#[tokio::test]
async fn test_progressive_remuxing_avi_head_only() {
    let temp_dir = tempdir().unwrap();
    let config = ProgressiveRemuxingConfig {
        temp_dir: temp_dir.path().to_path_buf(),
        min_head_size: 256 * 1024, // 256KB
        piece_wait_timeout: Duration::from_millis(100),
        ..Default::default()
    };

    let mut file_assembler = ProgressiveFileAssembler::new();
    let info_hash = InfoHash::new([2u8; 20]);

    // Create 5MB AVI file
    let avi_data = create_test_avi_data(5 * 1024 * 1024);
    file_assembler.add_file(info_hash, avi_data);

    // Make head available
    file_assembler.make_head_available(info_hash, 512 * 1024); // 512KB head

    let strategy = ProgressiveRemuxingStrategy::new(
        Arc::new(file_assembler),
        SimulationFfmpegProcessor::new(),
        config,
    )
    .unwrap();

    // Should be able to get file size
    let file_size = strategy.file_size(info_hash).await.unwrap();
    assert_eq!(file_size, 5 * 1024 * 1024);

    // Should return MP4 format
    let format = strategy.container_format(info_hash).await.unwrap();
    assert_eq!(format, riptide_core::streaming::ContainerFormat::Mp4);
}

#[tokio::test]
async fn test_progressive_remuxing_no_head_fails() {
    let temp_dir = tempdir().unwrap();
    let config = ProgressiveRemuxingConfig {
        temp_dir: temp_dir.path().to_path_buf(),
        min_head_size: 1024 * 1024, // 1MB
        piece_wait_timeout: Duration::from_millis(100),
        ..Default::default()
    };

    let mut file_assembler = ProgressiveFileAssembler::new();
    let info_hash = InfoHash::new([3u8; 20]);

    // Create file but don't make head available
    let mkv_data = create_test_mkv_data(10 * 1024 * 1024);
    file_assembler.add_file(info_hash, mkv_data);
    // No head data available

    let strategy = ProgressiveRemuxingStrategy::new(
        Arc::new(file_assembler),
        SimulationFfmpegProcessor::new(),
        config,
    )
    .unwrap();

    // Should fail due to missing head
    let result = strategy.stream_range(info_hash, 0..1024).await;
    assert!(result.is_err());
    assert!(matches!(
        result.unwrap_err(),
        riptide_core::streaming::StreamingError::MissingPieces { .. }
    ));
}

#[tokio::test]
async fn test_progressive_remuxing_head_and_tail() {
    let temp_dir = tempdir().unwrap();
    let config = ProgressiveRemuxingConfig {
        temp_dir: temp_dir.path().to_path_buf(),
        min_head_size: 512 * 1024, // 512KB
        piece_wait_timeout: Duration::from_millis(200),
        ..Default::default()
    };

    let mut file_assembler = ProgressiveFileAssembler::new();
    let info_hash = InfoHash::new([4u8; 20]);

    // Create 20MB MKV file
    let mkv_data = create_test_mkv_data(20 * 1024 * 1024);
    file_assembler.add_file(info_hash, mkv_data);

    // Make head and tail available (classic torrent streaming scenario)
    file_assembler.make_head_available(info_hash, 2 * 1024 * 1024); // 2MB head
    file_assembler.make_tail_available(info_hash, 3 * 1024 * 1024); // 3MB tail

    let strategy = ProgressiveRemuxingStrategy::new(
        Arc::new(file_assembler),
        SimulationFfmpegProcessor::new(),
        config,
    )
    .unwrap();

    // Should be able to start streaming
    let file_size = strategy.file_size(info_hash).await.unwrap();
    assert_eq!(file_size, 20 * 1024 * 1024);

    // Should return MP4 format
    let format = strategy.container_format(info_hash).await.unwrap();
    assert_eq!(format, riptide_core::streaming::ContainerFormat::Mp4);
}

#[tokio::test]
async fn test_progressive_remuxing_with_gaps() {
    let temp_dir = tempdir().unwrap();
    let config = ProgressiveRemuxingConfig {
        temp_dir: temp_dir.path().to_path_buf(),
        min_head_size: 256 * 1024, // 256KB
        piece_wait_timeout: Duration::from_millis(500),
        input_chunk_size: 64 * 1024, // 64KB chunks
        ..Default::default()
    };

    let mut file_assembler = ProgressiveFileAssembler::new();
    let info_hash = InfoHash::new([5u8; 20]);

    // Create 8MB AVI file
    let avi_data = create_test_avi_data(8 * 1024 * 1024);
    file_assembler.add_file(info_hash, avi_data);

    // Make head available
    file_assembler.make_head_available(info_hash, 1024 * 1024); // 1MB head

    // Add some gaps in the middle
    file_assembler.make_range_available(info_hash, 2 * 1024 * 1024..3 * 1024 * 1024); // 1MB at 2MB
    file_assembler.make_range_available(info_hash, 5 * 1024 * 1024..6 * 1024 * 1024); // 1MB at 5MB

    let strategy = ProgressiveRemuxingStrategy::new(
        Arc::new(file_assembler),
        SimulationFfmpegProcessor::new(),
        config,
    )
    .unwrap();

    // Should be able to start streaming with available data
    let file_size = strategy.file_size(info_hash).await.unwrap();
    assert_eq!(file_size, 8 * 1024 * 1024);
}

#[tokio::test]
async fn test_progressive_remuxing_simulated_download() {
    let temp_dir = tempdir().unwrap();
    let config = ProgressiveRemuxingConfig {
        temp_dir: temp_dir.path().to_path_buf(),
        min_head_size: 512 * 1024, // 512KB
        piece_wait_timeout: Duration::from_millis(1000),
        input_chunk_size: 128 * 1024, // 128KB chunks
        ..Default::default()
    };

    let mut file_assembler = ProgressiveFileAssembler::new();
    let info_hash = InfoHash::new([6u8; 20]);

    // Create 15MB MKV file
    let mkv_data = create_test_mkv_data(15 * 1024 * 1024);
    file_assembler.add_file(info_hash, mkv_data);

    // Make head available
    file_assembler.make_head_available(info_hash, 1024 * 1024); // 1MB head

    let file_assembler = Arc::new(std::sync::RwLock::new(file_assembler));
    let strategy = ProgressiveRemuxingStrategy::new(
        file_assembler.clone(),
        SimulationFfmpegProcessor::new(),
        config,
    )
    .unwrap();

    // Start streaming
    let file_size_task = strategy.file_size(info_hash);

    // Simulate progressive download in background
    let file_assembler_clone = file_assembler.clone();
    let download_task = tokio::spawn(async move {
        // Simulate download progress
        for i in 0..10 {
            sleep(Duration::from_millis(50)).await;
            file_assembler_clone
                .write()
                .unwrap()
                .simulate_progressive_download(
                    info_hash,
                    512 * 1024, // 512KB chunks
                );
        }
    });

    // Should complete successfully
    let file_size = file_size_task.await.unwrap();
    assert_eq!(file_size, 15 * 1024 * 1024);

    download_task.await.unwrap();
}

#[tokio::test]
async fn test_progressive_remuxing_format_support() {
    let temp_dir = tempdir().unwrap();
    let config = ProgressiveRemuxingConfig {
        temp_dir: temp_dir.path().to_path_buf(),
        ..Default::default()
    };

    let file_assembler = Arc::new(ProgressiveFileAssembler::new());
    let strategy =
        ProgressiveRemuxingStrategy::new(file_assembler, SimulationFfmpegProcessor::new(), config)
            .unwrap();

    // Test format support
    assert!(strategy.supports_format(&riptide_core::streaming::ContainerFormat::Mkv));
    assert!(strategy.supports_format(&riptide_core::streaming::ContainerFormat::Avi));
    assert!(!strategy.supports_format(&riptide_core::streaming::ContainerFormat::Mp4));
    assert!(!strategy.supports_format(&riptide_core::streaming::ContainerFormat::WebM));
}

#[tokio::test]
async fn test_progressive_remuxing_multiple_sessions() {
    let temp_dir = tempdir().unwrap();
    let config = ProgressiveRemuxingConfig {
        temp_dir: temp_dir.path().to_path_buf(),
        min_head_size: 256 * 1024, // 256KB
        ..Default::default()
    };

    let mut file_assembler = ProgressiveFileAssembler::new();

    // Create multiple files
    let info_hash1 = InfoHash::new([7u8; 20]);
    let info_hash2 = InfoHash::new([8u8; 20]);

    let mkv_data1 = create_test_mkv_data(5 * 1024 * 1024);
    let avi_data2 = create_test_avi_data(8 * 1024 * 1024);

    file_assembler.add_file(info_hash1, mkv_data1);
    file_assembler.add_file(info_hash2, avi_data2);

    // Make heads available
    file_assembler.make_head_available(info_hash1, 512 * 1024);
    file_assembler.make_head_available(info_hash2, 1024 * 1024);

    let strategy = ProgressiveRemuxingStrategy::new(
        Arc::new(file_assembler),
        SimulationFfmpegProcessor::new(),
        config,
    )
    .unwrap();

    // Should handle multiple sessions
    let file_size1 = strategy.file_size(info_hash1).await.unwrap();
    let file_size2 = strategy.file_size(info_hash2).await.unwrap();

    assert_eq!(file_size1, 5 * 1024 * 1024);
    assert_eq!(file_size2, 8 * 1024 * 1024);

    // Both should return MP4 format
    let format1 = strategy.container_format(info_hash1).await.unwrap();
    let format2 = strategy.container_format(info_hash2).await.unwrap();

    assert_eq!(format1, riptide_core::streaming::ContainerFormat::Mp4);
    assert_eq!(format2, riptide_core::streaming::ContainerFormat::Mp4);
}

#[tokio::test]
async fn test_progressive_remuxing_error_handling() {
    let temp_dir = tempdir().unwrap();
    let config = ProgressiveRemuxingConfig {
        temp_dir: temp_dir.path().to_path_buf(),
        min_head_size: 1024 * 1024,                    // 1MB
        piece_wait_timeout: Duration::from_millis(50), // Short timeout
        ..Default::default()
    };

    let mut file_assembler = ProgressiveFileAssembler::new();
    let info_hash = InfoHash::new([9u8; 20]);

    // Create file with insufficient head data
    let mkv_data = create_test_mkv_data(10 * 1024 * 1024);
    file_assembler.add_file(info_hash, mkv_data);

    // Make only partial head available (less than required)
    file_assembler.make_head_available(info_hash, 512 * 1024); // Only 512KB

    let strategy = ProgressiveRemuxingStrategy::new(
        Arc::new(file_assembler),
        SimulationFfmpegProcessor::new(),
        config,
    )
    .unwrap();

    // Should fail due to insufficient head data
    let result = strategy.stream_range(info_hash, 0..1024).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_progressive_remuxing_config_validation() {
    let temp_dir = tempdir().unwrap();

    // Test with invalid temp directory
    let invalid_config = ProgressiveRemuxingConfig {
        temp_dir: PathBuf::from("/invalid/nonexistent/path"),
        ..Default::default()
    };

    let file_assembler = Arc::new(ProgressiveFileAssembler::new());
    let result = ProgressiveRemuxingStrategy::new(
        file_assembler,
        SimulationFfmpegProcessor::new(),
        invalid_config,
    );

    assert!(result.is_err());

    // Test with valid config
    let valid_config = ProgressiveRemuxingConfig {
        temp_dir: temp_dir.path().to_path_buf(),
        ..Default::default()
    };

    let file_assembler = Arc::new(ProgressiveFileAssembler::new());
    let result = ProgressiveRemuxingStrategy::new(
        file_assembler,
        SimulationFfmpegProcessor::new(),
        valid_config,
    );

    assert!(result.is_ok());
}
