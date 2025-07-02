//! Strategy manager for coordinating between different streaming approaches

use std::ops::Range;
use std::sync::Arc;

use super::ffmpeg::FfmpegProcessor;
use super::remuxed_streaming::{RemuxedStreaming, RemuxingConfig};
use super::strategy::{
    ContainerDetector, ContainerFormat, StreamingError, StreamingResult, StreamingStrategy,
};
use crate::torrent::{InfoHash, PieceStore};

/// Manager that selects appropriate streaming strategy based on container format
pub struct StreamingStrategyManager<
    P: PieceStore,
    F: FfmpegProcessor = super::ffmpeg::SimulationFfmpegProcessor,
> {
    piece_store: Arc<P>,
    remuxed_streaming: Option<RemuxedStreaming<P, F>>,
}

impl<P: PieceStore, F: FfmpegProcessor> StreamingStrategyManager<P, F> {
    /// Create new strategy manager with piece store (direct streaming only)
    pub fn new(piece_store: Arc<P>) -> Self {
        Self {
            piece_store,
            remuxed_streaming: None,
        }
    }

    /// Create strategy manager with remuxing support
    pub fn with_remuxing(
        piece_store: Arc<P>,
        ffmpeg_processor: F,
        remuxing_config: RemuxingConfig,
    ) -> StreamingResult<Self> {
        let remuxed_streaming = RemuxedStreaming::new(
            Arc::clone(&piece_store),
            ffmpeg_processor,
            remuxing_config.cache_dir,
        )?;

        Ok(Self {
            piece_store,
            remuxed_streaming: Some(remuxed_streaming),
        })
    }

    /// Get appropriate strategy for the given torrent
    async fn select_strategy(
        &self,
        info_hash: InfoHash,
    ) -> StreamingResult<&dyn StreamingStrategy> {
        // Detect container format from file headers
        let format = self.detect_container_format(info_hash).await?;

        // Choose strategy based on format compatibility
        if format.is_browser_compatible() {
            // For MP4/WebM, we'd use DirectPieceStreaming
            // For now, return an error since DirectPieceStreaming isn't implemented yet
            Err(StreamingError::UnsupportedFormat {
                format: format!("Direct streaming for {format:?} not yet implemented"),
            })
        } else if let Some(ref remuxed) = self.remuxed_streaming {
            if remuxed.supports_format(&format) {
                Ok(remuxed as &dyn StreamingStrategy)
            } else {
                Err(StreamingError::UnsupportedFormat {
                    format: format!("{format:?}"),
                })
            }
        } else {
            Err(StreamingError::UnsupportedFormat {
                format: format!("{format:?} (remuxing not configured)"),
            })
        }
    }

    /// Detect container format from first piece
    async fn detect_container_format(
        &self,
        info_hash: InfoHash,
    ) -> StreamingResult<ContainerFormat> {
        // Get first piece to detect format
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

    /// Stream video data for the requested byte range
    pub async fn stream_range(
        &self,
        info_hash: InfoHash,
        range: Range<u64>,
    ) -> StreamingResult<Vec<u8>> {
        let strategy = self.select_strategy(info_hash).await?;
        strategy.stream_range(info_hash, range).await
    }

    /// Get total file size for Content-Length headers
    pub async fn file_size(&self, info_hash: InfoHash) -> StreamingResult<u64> {
        let strategy = self.select_strategy(info_hash).await?;
        strategy.file_size(info_hash).await
    }

    /// Get container format and MIME type for Content-Type headers
    pub async fn content_info(
        &self,
        info_hash: InfoHash,
    ) -> StreamingResult<(ContainerFormat, &'static str)> {
        let strategy = self.select_strategy(info_hash).await?;
        let format = strategy.container_format(info_hash).await?;
        let mime_type = format.mime_type();
        Ok((format, mime_type))
    }

    /// Check if torrent can be streamed with current configuration
    pub async fn can_stream(&self, info_hash: InfoHash) -> bool {
        self.select_strategy(info_hash).await.is_ok()
    }
}

/// Information about streaming capability for a torrent
#[derive(Debug, Clone)]
pub struct StreamingCapability {
    pub can_stream: bool,
    pub container_format: ContainerFormat,
    pub requires_remuxing: bool,
    pub estimated_delay: Option<std::time::Duration>,
}

impl<P: PieceStore, F: FfmpegProcessor> StreamingStrategyManager<P, F> {
    /// Get detailed streaming capability information
    pub async fn get_streaming_capability(
        &self,
        info_hash: InfoHash,
    ) -> StreamingResult<StreamingCapability> {
        let format = self.detect_container_format(info_hash).await?;

        let (can_stream, requires_remuxing, estimated_delay) = if format.is_browser_compatible() {
            // Direct streaming would be supported if implemented
            (false, false, None) // Set to false until DirectPieceStreaming is implemented
        } else if let Some(ref remuxed) = self.remuxed_streaming {
            if remuxed.supports_format(&format) {
                // Check if all pieces are available for remuxing
                if let Ok(file_reconstructor) = self.get_file_reconstructor() {
                    let can_reconstruct = file_reconstructor.can_reconstruct(info_hash)?;
                    if can_reconstruct {
                        (true, true, Some(std::time::Duration::from_secs(60))) // Estimate
                    } else {
                        (false, true, None) // Missing pieces
                    }
                } else {
                    (false, true, None)
                }
            } else {
                (false, false, None)
            }
        } else {
            (false, false, None)
        };

        Ok(StreamingCapability {
            can_stream,
            container_format: format,
            requires_remuxing,
            estimated_delay,
        })
    }

    /// Get file reconstructor for checking piece availability
    fn get_file_reconstructor(
        &self,
    ) -> StreamingResult<super::file_reconstruction::FileReconstructor<P>> {
        Ok(super::file_reconstruction::FileReconstructor::new(
            Arc::clone(&self.piece_store),
        ))
    }
}
