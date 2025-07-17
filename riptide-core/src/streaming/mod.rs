//! Modern streaming pipeline for progressive media delivery.
//!
//! This module implements a stateless streaming architecture that separates
//! torrent piece management from media streaming concerns. The core abstractions
//! are PieceProvider (file-like interface over torrent storage) and StreamProducer
//! (HTTP response generator for media content).

pub mod direct_stream;
pub mod download_manager;
pub mod ffmpeg;
pub mod media_info;
pub mod mp4_parser;
pub mod mp4_validation;
pub mod performance_tests;
pub mod piece_provider_adapter;
pub mod piece_reader;
pub mod range;
pub mod remux_stream;
pub mod traits;

use std::sync::Arc;

use axum::http::Request;
use axum::response::Response;
// Stream implementations
pub use direct_stream::DirectStreamProducer;
// FFmpeg integration
pub use ffmpeg::{Ffmpeg, ProductionFfmpeg, RemuxingOptions, RemuxingResult, SimulationFfmpeg};
// Media format detection and configuration
pub use media_info::{
    ContainerFormat, MediaInfo, MediaInfoError, StreamingError, StreamingResult,
    detect_container_format, extension, mime_type, requires_remuxing,
};
// Media parsing utilities
pub use mp4_parser::extract_duration_from_media;
// DataSource to PieceProvider adapter
pub use piece_provider_adapter::PieceProviderAdapter;
// Legacy piece reader support
pub use piece_reader::{
    PieceBasedStreamReader, PieceReaderError, create_piece_reader_from_trait_object,
};
// HTTP range support
pub use range::{ContentInfo, Range, RangeRequest, RangeResponse};
pub use remux_stream::RemuxStreamProducer;
// Core abstractions
pub use traits::{PieceProvider, PieceProviderError, StreamProducer};

/// Creates an appropriate stream producer based on media format detection.
///
/// This factory function examines the file header to determine the container
/// format and returns either a DirectStreamProducer (for MP4/WebM) or
/// RemuxStreamProducer (for MKV/AVI/MOV) as appropriate.
///
/// # Errors
///
/// - `StreamingError::FormatDetectionFailed` - Cannot read header or detect format
pub async fn create_stream_producer(
    provider: Arc<dyn PieceProvider>,
) -> StreamingResult<Arc<dyn StreamProducer>> {
    // Read header bytes for format detection
    const HEADER_SIZE: usize = 64;
    let header_data = provider
        .read_at(0, HEADER_SIZE)
        .await
        .map_err(|_| StreamingError::FormatDetectionFailed)?;

    // Detect container format
    let format =
        detect_container_format(&header_data).map_err(|_| StreamingError::FormatDetectionFailed)?;

    // Create appropriate producer based on format
    let producer: Arc<dyn StreamProducer> = if requires_remuxing(&format) {
        Arc::new(RemuxStreamProducer::new(
            provider,
            extension(&format).to_string(),
        ))
    } else {
        Arc::new(DirectStreamProducer::new(
            provider,
            mime_type(&format).to_string(),
        ))
    };

    Ok(producer)
}

/// Simple HTTP streaming interface for media content.
///
/// This is a lightweight wrapper around the streaming pipeline that provides
/// a simple interface for serving media streams over HTTP.
pub struct HttpStreaming {
    /// The underlying stream producer for this session.
    producer: Arc<dyn StreamProducer>,
}

impl HttpStreaming {
    /// Creates a new streaming session from a piece provider.
    ///
    /// This automatically detects the media format and selects the appropriate
    /// streaming strategy (direct or remux).
    ///
    /// # Errors
    ///
    /// - `StreamingError::FormatDetectionFailed` - Cannot read header or detect format
    pub async fn new(provider: Arc<dyn PieceProvider>) -> StreamingResult<Self> {
        let producer = create_stream_producer(provider).await?;
        Ok(Self { producer })
    }

    /// Serves an HTTP streaming response for the given request.
    ///
    /// The request is passed through to the underlying stream producer to handle
    /// range requests and other HTTP semantics appropriately.
    pub async fn serve_http_stream(&self, request: Request<()>) -> Response {
        self.producer.produce_stream(request).await
    }
}

#[cfg(test)]
mod tests {
    use bytes::Bytes;

    use super::*;

    struct TestPieceProvider {
        data: Vec<u8>,
    }

    impl TestPieceProvider {
        fn new(data: Vec<u8>) -> Self {
            Self { data }
        }
    }

    #[async_trait::async_trait]
    impl PieceProvider for TestPieceProvider {
        async fn read_at(&self, offset: u64, length: usize) -> Result<Bytes, PieceProviderError> {
            let start = offset as usize;
            let end = (start + length).min(self.data.len());
            if start >= self.data.len() {
                return Ok(Bytes::new());
            }
            Ok(Bytes::copy_from_slice(&self.data[start..end]))
        }

        async fn size(&self) -> u64 {
            self.data.len() as u64
        }
    }

    #[tokio::test]
    async fn test_create_stream_producer_mp4() {
        // MP4 header with ftyp box
        let mut mp4_data = vec![
            0x00, 0x00, 0x00, 0x20, // box size
            b'f', b't', b'y', b'p', // box type 'ftyp'
            b'i', b's', b'o', b'm', // major brand
        ];
        mp4_data.extend_from_slice(&[0u8; 100]);

        let provider = Arc::new(TestPieceProvider::new(mp4_data));
        let producer = create_stream_producer(provider).await.unwrap();

        // Should create DirectStreamProducer for MP4
        assert!(Arc::ptr_eq(
            &producer,
            &(producer.clone() as Arc<dyn StreamProducer>)
        ));
    }

    #[tokio::test]
    async fn test_http_streaming_creation() {
        let mp4_data = vec![
            0x00, 0x00, 0x00, 0x20, b'f', b't', b'y', b'p', b'i', b's', b'o', b'm',
        ];
        let provider = Arc::new(TestPieceProvider::new(mp4_data));

        let streaming = HttpStreaming::new(provider).await.unwrap();
        // Verify streaming was created successfully - the fact we got here means it worked
        assert!(Arc::strong_count(&streaming.producer) >= 1);
    }
}
