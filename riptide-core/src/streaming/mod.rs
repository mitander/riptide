//! Direct streaming service with HTTP range requests
//!
//! Provides media streaming capabilities that integrate with the
//! peer management system for streaming performance.

pub mod ffmpeg;
pub mod file_assembler;
pub mod file_reconstruction;
pub mod mp4_validation;
pub mod performance_tests;
pub mod piece_reader;

pub mod range_handler;
pub mod remux;

pub mod storage_cache;

use std::collections::HashMap;
use std::sync::Arc;

pub use ffmpeg::{
    FfmpegProcessor, ProductionFfmpegProcessor, RemuxingOptions, RemuxingResult,
    SimulationFfmpegProcessor,
};
pub use file_assembler::{CacheStats, FileAssembler, FileAssemblerError, PieceFileAssembler};
pub use file_reconstruction::{FileReconstructor, create_file_reconstructor_from_trait_object};
pub use piece_reader::{
    PieceBasedStreamReader, PieceReaderError, create_piece_reader_from_trait_object,
};
pub use range_handler::{ContentInfo, RangeHandler, RangeRequest, RangeResponse};
// Legacy aliases for backward compatibility
pub use remux::RemuxStreamStrategy as RemuxStreamingStrategy;
pub use remux::types::RemuxConfig as RemuxStreamingConfig;
pub use remux::{
    ContainerFormat, DirectStreamStrategy, RemuxError, RemuxProgress, RemuxSessionManager,
    RemuxState, RemuxStreamStrategy, StrategyError, StreamData, StreamHandle, StreamReadiness,
    StreamingError, StreamingResult, StreamingStatus, StreamingStrategy,
};
pub use storage_cache::{
    CacheEntry, CacheKey, CacheStatistics, StorageCache, StorageCacheConfig, StorageCacheError,
};

use crate::config::RiptideConfig;
use crate::torrent::TorrentEngineHandle;

/// Streaming service coordinator integrating multiple streaming strategies.
///
/// Acts as a lightweight coordinator that delegates to appropriate streaming
/// strategies based on container format detection.
pub struct DirectStreamingService {
    strategies: HashMap<ContainerFormat, Arc<dyn StreamingStrategy>>,
    session_manager: Arc<RemuxSessionManager>,
    torrent_engine: TorrentEngineHandle,
    file_assembler: Arc<dyn FileAssembler>,
}

impl Clone for DirectStreamingService {
    fn clone(&self) -> Self {
        Self {
            strategies: self.strategies.clone(),
            session_manager: self.session_manager.clone(),
            torrent_engine: self.torrent_engine.clone(),
            file_assembler: self.file_assembler.clone(),
        }
    }
}

impl DirectStreamingService {
    /// Create new streaming service with default strategies
    pub fn new(
        torrent_engine: TorrentEngineHandle,
        file_assembler: Arc<dyn FileAssembler>,
        config: RiptideConfig,
    ) -> Self {
        let session_manager =
            RemuxSessionManager::new(Default::default(), Arc::clone(&file_assembler));

        let mut strategies: HashMap<ContainerFormat, Arc<dyn StreamingStrategy>> = HashMap::new();

        // Direct streaming strategies
        strategies.insert(
            ContainerFormat::Mp4,
            Arc::new(DirectStreamStrategy::new(Arc::clone(&file_assembler))),
        );
        strategies.insert(
            ContainerFormat::WebM,
            Arc::new(DirectStreamStrategy::new(Arc::clone(&file_assembler))),
        );

        // Remux streaming strategies
        let remux_strategy = RemuxStreamStrategy::new(session_manager.clone());
        strategies.insert(ContainerFormat::Avi, Arc::new(remux_strategy.clone()));
        strategies.insert(ContainerFormat::Mkv, Arc::new(remux_strategy.clone()));
        strategies.insert(ContainerFormat::Mov, Arc::new(remux_strategy));

        Self {
            strategies,
            session_manager: Arc::new(session_manager),
            torrent_engine,
            file_assembler,
        }
    }

    /// Handle streaming request by delegating to appropriate strategy
    pub async fn handle_stream_request(
        &self,
        info_hash: crate::torrent::InfoHash,
        range: std::ops::Range<u64>,
    ) -> StreamingResult<StreamData> {
        // Detect container format
        let format = self.detect_container_format(info_hash).await?;

        // Get appropriate strategy
        let strategy =
            self.strategies
                .get(&format)
                .ok_or_else(|| StrategyError::UnsupportedFormat {
                    format: format!("{:?}", format),
                })?;

        // Prepare stream handle
        let handle = strategy.prepare_stream(info_hash, format).await?;

        // Serve the requested range
        strategy.serve_range(&handle, range).await
    }

    /// Check if stream is ready for the given range
    pub async fn check_stream_readiness(
        &self,
        info_hash: crate::torrent::InfoHash,
    ) -> StreamingResult<StreamingStatus> {
        // Detect container format
        let format = self.detect_container_format(info_hash).await?;

        // Get appropriate strategy
        let strategy =
            self.strategies
                .get(&format)
                .ok_or_else(|| StrategyError::UnsupportedFormat {
                    format: format!("{:?}", format),
                })?;

        // Prepare stream handle
        let handle = strategy.prepare_stream(info_hash, format).await?;

        // Get status
        strategy.status(&handle).await
    }

    /// Detect container format from file headers
    async fn detect_container_format(
        &self,
        info_hash: crate::torrent::InfoHash,
    ) -> StreamingResult<ContainerFormat> {
        // Read first few bytes to detect format
        const HEADER_SIZE: u64 = 32;
        let header_data = self
            .file_assembler
            .read_range(info_hash, 0..HEADER_SIZE)
            .await
            .map_err(|_| StrategyError::FormatDetectionFailed)?;

        // Simple format detection
        if header_data.starts_with(b"ftyp") || header_data[4..8] == *b"ftyp" {
            Ok(ContainerFormat::Mp4)
        } else if header_data.starts_with(b"\x1A\x45\xDF\xA3") {
            Ok(ContainerFormat::Mkv)
        } else if header_data.starts_with(b"RIFF") && header_data[8..12] == *b"AVI " {
            Ok(ContainerFormat::Avi)
        } else if header_data.starts_with(b"\x1a\x45\xdf\xa3") {
            Ok(ContainerFormat::WebM)
        } else {
            Ok(ContainerFormat::Unknown)
        }
    }

    /// Get streaming statistics
    pub async fn statistics(&self) -> StreamingServiceStats {
        StreamingServiceStats {
            active_sessions: 0,           // TODO: Implement
            total_bytes_streamed: 0,      // TODO: Implement
            concurrent_remux_sessions: 0, // TODO: Implement
        }
    }
}

/// Streaming service statistics
#[derive(Debug, Clone)]
pub struct StreamingServiceStats {
    pub active_sessions: usize,
    pub total_bytes_streamed: u64,
    pub concurrent_remux_sessions: usize,
}
