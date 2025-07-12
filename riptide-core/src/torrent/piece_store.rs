//! Piece storage abstraction for different storage backends
//!
//! Provides unified interface for accessing torrent pieces from different sources:
//! file system storage, in-memory simulation data, or network peers.

use async_trait::async_trait;

use super::{InfoHash, PieceIndex, TorrentError};

/// Abstract interface for piece storage and retrieval
///
/// Enables different storage backends: file system, simulation data, or network.
/// Used by peer managers to serve piece data in response to requests.
#[async_trait]
pub trait PieceStore: Send + Sync {
    /// Retrieves piece data for specified torrent and piece index
    ///
    /// # Errors
    ///
    /// - `TorrentError::TorrentNotFound` - If unknown info hash
    /// - `TorrentError::PieceHashMismatch` - If invalid piece index
    /// - `TorrentError::Io` - If storage access error
    async fn piece_data(
        &self,
        info_hash: InfoHash,
        piece_index: PieceIndex,
    ) -> Result<Vec<u8>, TorrentError>;

    /// Checks if piece is available without retrieving data
    fn has_piece(&self, info_hash: InfoHash, piece_index: PieceIndex) -> bool;

    /// Returns total number of pieces for a torrent
    ///
    /// # Errors
    ///
    /// - `TorrentError::TorrentNotFound` - If unknown info hash
    fn piece_count(&self, info_hash: InfoHash) -> Result<u32, TorrentError>;
}
