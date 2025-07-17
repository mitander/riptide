//! Core abstractions for the streaming pipeline.
//!
//! This module defines the fundamental traits that enable a clean separation
//! between torrent piece management and media streaming. The design creates
//! a deep module boundary where the streaming logic is entirely decoupled
//! from BitTorrent implementation details.

use axum::http::Request;
use axum::response::Response;
use bytes::Bytes;
use thiserror::Error;

/// Provides a file-like async interface over torrent piece storage.
///
/// This trait abstracts the chaotic, non-sequential arrival of torrent pieces
/// into a simple, linear byte-range reading interface. Implementations handle
/// piece fetching, verification, and assembly to fulfill read requests.
///
/// The streaming module requests bytes from a logical file position, and the
/// provider ensures those bytes are available, downloading pieces as needed.
#[async_trait::async_trait]
pub trait PieceProvider: Send + Sync {
    /// Reads a byte range from the logical file representation.
    ///
    /// This method blocks until the required pieces are downloaded and verified.
    /// The offset and length are in terms of the complete file, not individual pieces.
    ///
    /// # Errors
    ///
    /// - `PieceProviderError::NotYetAvailable` - Required pieces are not yet downloaded
    /// - `PieceProviderError::VerificationFailed` - Piece hash verification failed
    /// - `PieceProviderError::StorageError` - Underlying storage operation failed
    /// - `PieceProviderError::InvalidRange` - Requested range exceeds file bounds
    async fn read_at(&self, offset: u64, length: usize) -> Result<Bytes, PieceProviderError>;

    /// Returns the total size of the logical file in bytes.
    async fn size(&self) -> u64;
}

/// Produces streamable HTTP responses from media content.
///
/// This trait abstracts the entire process of generating media bytes, whether
/// through direct pass-through for compatible formats or real-time remuxing
/// for incompatible containers. The web server remains agnostic to the
/// underlying streaming strategy.
#[async_trait::async_trait]
pub trait StreamProducer: Send + Sync {
    /// Produces an HTTP response containing the media stream.
    ///
    /// The response body is a stream of bytes suitable for progressive playback.
    /// For compatible formats (MP4), this may be a direct byte stream. For
    /// incompatible formats (MKV/AVI), this involves real-time remuxing to
    /// fragmented MP4.
    ///
    /// The request parameter allows implementations to handle range requests
    /// and other HTTP semantics appropriately.
    async fn produce_stream(&self, request: Request<()>) -> Response;
}

/// Errors that can occur when reading from a piece provider.
#[derive(Debug, Error)]
pub enum PieceProviderError {
    /// The requested pieces are not yet available for reading.
    ///
    /// This is a transient error indicating the torrent engine hasn't
    /// downloaded the required pieces yet. Callers should retry after
    /// a short delay.
    #[error("requested pieces not yet available")]
    NotYetAvailable,

    /// Piece verification failed due to hash mismatch.
    ///
    /// This indicates data corruption and is typically unrecoverable
    /// for the affected pieces.
    #[error("piece verification failed: {0}")]
    VerificationFailed(String),

    /// An error occurred in the underlying storage system.
    #[error("storage error: {0}")]
    StorageError(String),

    /// The requested byte range is invalid.
    ///
    /// This occurs when the offset + length exceeds the file size.
    #[error("invalid range: offset {offset} + length {length} exceeds file size {file_size}")]
    InvalidRange {
        /// The requested starting offset.
        offset: u64,
        /// The requested read length.
        length: usize,
        /// The actual file size.
        file_size: u64,
    },
}
