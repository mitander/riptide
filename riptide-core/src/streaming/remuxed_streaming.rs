//! Remuxed streaming for container formats that need transcoding (MKV, AVI)

use std::ops::Range;
use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;

use super::ffmpeg::{FfmpegProcessor, RemuxingOptions};
use super::file_reconstruction::FileReconstructor;
use super::strategy::{
    ContainerDetector, ContainerFormat, StreamingError, StreamingResult, StreamingStrategy,
};
use crate::torrent::{InfoHash, PieceStore};

/// Remuxed streaming strategy for formats requiring container conversion
///
/// Handles formats like MKV that browsers cannot play natively by:
/// 1. Reconstructing the complete file from BitTorrent pieces
/// 2. Remuxing to MP4 using FFmpeg
/// 3. Caching the remuxed result for future requests
/// 4. Streaming the converted MP4 data
pub struct RemuxedStreaming<P: PieceStore, F: FfmpegProcessor> {
    piece_store: Arc<P>,
    ffmpeg_processor: F,
    file_reconstructor: FileReconstructor<P>,
    cache_dir: PathBuf,
    remuxing_options: RemuxingOptions,
}

impl<P: PieceStore, F: FfmpegProcessor> RemuxedStreaming<P, F> {
    /// Create new remuxed streaming strategy
    ///
    /// # Errors
    /// - `StreamingError::IoError` - Failed to create cache directory
    /// - `StreamingError::FfmpegError` - FFmpeg not available
    pub fn new(
        piece_store: Arc<P>,
        ffmpeg_processor: F,
        cache_dir: PathBuf,
    ) -> StreamingResult<Self> {
        // Verify FFmpeg is available
        if !ffmpeg_processor.is_available() {
            return Err(StreamingError::FfmpegError {
                reason: "FFmpeg not available for remuxing".to_string(),
            });
        }

        // Create cache directory if it doesn't exist
        std::fs::create_dir_all(&cache_dir).map_err(|e| StreamingError::IoError {
            operation: "create cache directory".to_string(),
            path: cache_dir.to_string_lossy().to_string(),
            source: e,
        })?;

        let file_reconstructor = FileReconstructor::new(Arc::clone(&piece_store));

        Ok(Self {
            piece_store,
            ffmpeg_processor,
            file_reconstructor,
            cache_dir,
            remuxing_options: RemuxingOptions::default(),
        })
    }

    /// Configure remuxing options
    pub fn with_remuxing_options(mut self, options: RemuxingOptions) -> Self {
        self.remuxing_options = options;
        self
    }

    /// Get cache file path for a torrent
    fn cache_path(&self, info_hash: InfoHash) -> PathBuf {
        self.cache_dir.join(format!("{info_hash}.mp4"))
    }

    /// Get temporary reconstruction path for a torrent
    fn temp_reconstruction_path(&self, info_hash: InfoHash) -> PathBuf {
        self.cache_dir.join(format!("{info_hash}.temp"))
    }

    /// Ensure file is remuxed and cached
    async fn ensure_remuxed(&self, info_hash: InfoHash) -> StreamingResult<PathBuf> {
        let cache_path = self.cache_path(info_hash);

        // Return cached file if it exists
        if cache_path.exists() {
            tracing::debug!("Using cached remuxed file for {}", info_hash);
            return Ok(cache_path);
        }

        tracing::debug!("Starting remuxing process for {}", info_hash);

        // Check if all pieces are available
        if !self.file_reconstructor.can_reconstruct(info_hash)? {
            let missing = self.file_reconstructor.missing_pieces(info_hash)?;
            return Err(StreamingError::MissingPieces {
                missing,
                total: self.piece_store.piece_count(info_hash).map_err(|e| {
                    StreamingError::PieceStorageError {
                        reason: e.to_string(),
                    }
                })?,
            });
        }

        // Reconstruct the original file
        let temp_path = self.temp_reconstruction_path(info_hash);
        let _bytes_written = self
            .file_reconstructor
            .reconstruct_file(info_hash, &temp_path)
            .await?;

        tracing::debug!("File reconstructed, starting remuxing to MP4");

        // Remux to MP4
        let remux_result = self
            .ffmpeg_processor
            .remux_to_mp4(&temp_path, &cache_path, &self.remuxing_options)
            .await?;

        tracing::debug!(
            "Remuxing completed: {} bytes in {:.2}s",
            remux_result.output_size,
            remux_result.processing_time
        );

        // Clean up temporary file
        if let Err(e) = std::fs::remove_file(&temp_path) {
            tracing::warn!("Failed to remove temp file: {}", e);
        }

        Ok(cache_path)
    }

    /// Read range from cached MP4 file
    async fn read_range_from_cache(
        &self,
        cache_path: &PathBuf,
        range: Range<u64>,
    ) -> StreamingResult<Vec<u8>> {
        use std::io::{Read, Seek, SeekFrom};

        let mut file = std::fs::File::open(cache_path).map_err(|e| StreamingError::IoError {
            operation: "open cached file".to_string(),
            path: cache_path.to_string_lossy().to_string(),
            source: e,
        })?;

        // Seek to start position
        file.seek(SeekFrom::Start(range.start))
            .map_err(|e| StreamingError::IoError {
                operation: "seek in cached file".to_string(),
                path: cache_path.to_string_lossy().to_string(),
                source: e,
            })?;

        // Calculate read length
        let read_length = (range.end - range.start) as usize;
        let mut buffer = vec![0u8; read_length];

        // Read the range
        file.read_exact(&mut buffer)
            .map_err(|e| StreamingError::IoError {
                operation: "read from cached file".to_string(),
                path: cache_path.to_string_lossy().to_string(),
                source: e,
            })?;

        Ok(buffer)
    }
}

#[async_trait]
impl<P: PieceStore, F: FfmpegProcessor> StreamingStrategy for RemuxedStreaming<P, F> {
    async fn stream_range(
        &self,
        info_hash: InfoHash,
        range: Range<u64>,
    ) -> StreamingResult<Vec<u8>> {
        // Ensure the file is remuxed and cached
        let cache_path = self.ensure_remuxed(info_hash).await?;

        // Read the requested range from the cached MP4
        self.read_range_from_cache(&cache_path, range).await
    }

    async fn file_size(&self, info_hash: InfoHash) -> StreamingResult<u64> {
        // Ensure the file is remuxed and cached
        let cache_path = self.ensure_remuxed(info_hash).await?;

        // Get file size from cached MP4
        let metadata = std::fs::metadata(&cache_path).map_err(|e| StreamingError::IoError {
            operation: "get cached file metadata".to_string(),
            path: cache_path.to_string_lossy().to_string(),
            source: e,
        })?;

        Ok(metadata.len())
    }

    async fn container_format(&self, info_hash: InfoHash) -> StreamingResult<ContainerFormat> {
        // We need to detect the original format from the first piece
        let first_piece_data = self
            .piece_store
            .piece_data(info_hash, crate::torrent::PieceIndex::new(0))
            .await
            .map_err(|e| StreamingError::PieceStorageError {
                reason: e.to_string(),
            })?;

        // Take first 512 bytes for format detection
        let header_bytes = if first_piece_data.len() >= 512 {
            &first_piece_data[..512]
        } else {
            &first_piece_data
        };

        Ok(ContainerDetector::detect_format(header_bytes))
    }

    fn supports_format(&self, format: &ContainerFormat) -> bool {
        // Supports formats that need remuxing to MP4
        matches!(format, ContainerFormat::Mkv | ContainerFormat::Avi)
    }
}

/// Configuration for remuxed streaming operations
#[derive(Debug, Clone)]
pub struct RemuxingConfig {
    /// Directory for storing remuxed files
    pub cache_dir: PathBuf,

    /// Maximum number of concurrent remuxing operations
    pub max_concurrent_jobs: usize,

    /// FFmpeg remuxing options
    pub remuxing_options: RemuxingOptions,
}

impl Default for RemuxingConfig {
    fn default() -> Self {
        Self {
            cache_dir: PathBuf::from("/tmp/riptide-cache"),
            max_concurrent_jobs: 2, // Limit CPU usage
            remuxing_options: RemuxingOptions::default(),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use tempfile::tempdir;

    use super::*;
    use crate::streaming::SimulationFfmpegProcessor;
    use crate::torrent::{InfoHash, PieceIndex, TorrentError, TorrentPiece};

    /// Mock piece store for testing
    struct MockPieceStore {
        pieces: HashMap<(InfoHash, u32), Vec<u8>>,
        piece_counts: HashMap<InfoHash, u32>,
    }

    impl MockPieceStore {
        fn new() -> Self {
            Self {
                pieces: HashMap::new(),
                piece_counts: HashMap::new(),
            }
        }

        fn add_pieces(&mut self, info_hash: InfoHash, pieces: Vec<TorrentPiece>) {
            let count = pieces.len() as u32;
            self.piece_counts.insert(info_hash, count);

            for piece in pieces {
                self.pieces.insert((info_hash, piece.index), piece.data);
            }
        }
    }

    #[async_trait::async_trait]
    impl PieceStore for MockPieceStore {
        async fn piece_data(
            &self,
            info_hash: InfoHash,
            piece_index: PieceIndex,
        ) -> Result<Vec<u8>, TorrentError> {
            self.pieces
                .get(&(info_hash, piece_index.as_u32()))
                .cloned()
                .ok_or(TorrentError::PieceHashMismatch { index: piece_index })
        }

        fn has_piece(&self, info_hash: InfoHash, piece_index: PieceIndex) -> bool {
            self.pieces.contains_key(&(info_hash, piece_index.as_u32()))
        }

        fn piece_count(&self, info_hash: InfoHash) -> Result<u32, TorrentError> {
            self.piece_counts
                .get(&info_hash)
                .copied()
                .ok_or(TorrentError::TorrentNotFound { info_hash })
        }
    }

    #[tokio::test]
    async fn test_remuxed_streaming_creation() {
        let temp_dir = tempdir().unwrap();
        let cache_dir = temp_dir.path().join("cache");

        let piece_store = Arc::new(MockPieceStore::new());
        let ffmpeg_processor = SimulationFfmpegProcessor::new().with_speed(100.0); // Fast for tests

        let streaming = RemuxedStreaming::new(piece_store, ffmpeg_processor, cache_dir).unwrap();

        // Test format support
        assert!(streaming.supports_format(&ContainerFormat::Mkv));
        assert!(streaming.supports_format(&ContainerFormat::Avi));
        assert!(!streaming.supports_format(&ContainerFormat::Mp4));
        assert!(!streaming.supports_format(&ContainerFormat::WebM));
    }

    #[tokio::test]
    async fn test_remuxed_streaming_mkv_format_detection() {
        let temp_dir = tempdir().unwrap();
        let cache_dir = temp_dir.path().join("cache");
        let info_hash = InfoHash::new([1u8; 20]);

        // Create MKV data for first piece
        let mkv_data = vec![
            0x1A, 0x45, 0xDF, 0xA3, // EBML signature
            0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x1F, 0x42, 0x86, // DocType element
            0x81, 0x01, // size 1
            b'm', // start of "matroska"
        ];

        let pieces = vec![TorrentPiece {
            index: 0,
            hash: [0u8; 20],
            data: mkv_data,
        }];

        let mut piece_store = MockPieceStore::new();
        piece_store.add_pieces(info_hash, pieces);

        let ffmpeg_processor = SimulationFfmpegProcessor::new();
        let streaming =
            RemuxedStreaming::new(Arc::new(piece_store), ffmpeg_processor, cache_dir).unwrap();

        // Test format detection
        let format = streaming.container_format(info_hash).await.unwrap();
        assert_eq!(format, ContainerFormat::Mkv);
    }

    #[tokio::test]
    async fn test_remuxed_streaming_missing_pieces() {
        let temp_dir = tempdir().unwrap();
        let cache_dir = temp_dir.path().join("cache");
        let info_hash = InfoHash::new([2u8; 20]);

        // Create incomplete pieces (missing piece 1)
        let pieces = vec![TorrentPiece {
            index: 0,
            hash: [0u8; 20],
            data: vec![0u8; 1024],
        }];

        let mut piece_store = MockPieceStore::new();
        piece_store.add_pieces(info_hash, pieces);
        // Set piece count to 2 even though we only have 1 piece
        piece_store.piece_counts.insert(info_hash, 2);

        let ffmpeg_processor = SimulationFfmpegProcessor::new();
        let streaming =
            RemuxedStreaming::new(Arc::new(piece_store), ffmpeg_processor, cache_dir).unwrap();

        // Test that streaming fails due to missing pieces
        let result = streaming.stream_range(info_hash, 0..100).await;
        assert!(matches!(result, Err(StreamingError::MissingPieces { .. })));
    }

    #[tokio::test]
    async fn test_remuxed_streaming_end_to_end() {
        let temp_dir = tempdir().unwrap();
        let cache_dir = temp_dir.path().join("cache");
        let info_hash = InfoHash::new([3u8; 20]);

        // Create MKV data pieces
        let piece1_data = vec![
            0x1A, 0x45, 0xDF, 0xA3, // EBML signature
            0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x1F, 0x42, 0x86, // DocType element
        ];
        let mut full_piece1 = piece1_data.clone();
        full_piece1.resize(512, 0); // Pad to 512 bytes

        let pieces = vec![
            TorrentPiece {
                index: 0,
                hash: [0u8; 20],
                data: full_piece1,
            },
            TorrentPiece {
                index: 1,
                hash: [0u8; 20],
                data: vec![0u8; 512],
            },
        ];

        let mut piece_store = MockPieceStore::new();
        piece_store.add_pieces(info_hash, pieces);

        let ffmpeg_processor = SimulationFfmpegProcessor::new().with_speed(100.0); // Fast for tests
        let streaming =
            RemuxedStreaming::new(Arc::new(piece_store), ffmpeg_processor, cache_dir).unwrap();

        // Test streaming - this should fail since we have invalid MKV data
        let result = streaming.stream_range(info_hash, 0..100).await;
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            StreamingError::FfmpegError { .. }
        ));
    }

    #[test]
    fn test_remuxing_config_defaults() {
        let config = RemuxingConfig::default();
        assert_eq!(config.cache_dir, PathBuf::from("/tmp/riptide-cache"));
        assert_eq!(config.max_concurrent_jobs, 2);
    }
}
