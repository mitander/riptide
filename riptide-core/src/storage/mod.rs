//! Storage layer for torrent data.
//!
//! Defines storage interface for piece data with file-based implementation.
//! Handles piece persistence, verification, and torrent completion tracking.

pub mod data_source;
pub mod file_library;
pub mod file_storage;
#[cfg(any(test, feature = "test-utils"))]
pub mod test_fixtures;

use std::path::PathBuf;

use async_trait::async_trait;
pub use data_source::{
    CacheEntry, CacheKey, CacheStatistics, CacheStats as DataSourceCacheStats, CacheableDataSource,
    DataError, DataResult, DataSource, LocalDataSource, PieceDataSource, RangeAvailability,
    RangeKey, StorageCache, StorageCacheConfig, StorageCacheError,
    create_data_source_from_trait_object, create_local_data_source, validate_range,
    validate_range_bounds,
};
pub use file_library::{FileLibrary, LibraryFile};
pub use file_storage::FileStorage;

// Range calculator is now part of data_source module
// Import is handled through data_source re-exports
use crate::torrent::{InfoHash, PieceIndex};

/// Storage operations for torrent piece data.
///
/// Defines interface for persisting, retrieving, and managing piece data
/// across torrent downloads. Implementations handle storage backend details.
#[async_trait]
pub trait Storage: Send + Sync {
    /// Stores verified piece data to persistent storage.
    ///
    /// # Errors
    ///
    /// - `StorageError::InsufficientSpace` - If not enough disk space
    /// - `StorageError::Io` - If file system operation failed
    async fn store_piece(
        &mut self,
        info_hash: InfoHash,
        index: PieceIndex,
        piece_bytes: &[u8],
    ) -> Result<(), StorageError>;

    /// Loads piece data from storage.
    ///
    /// # Errors
    ///
    /// - `StorageError::PieceNotFound` - If piece not yet downloaded
    /// - `StorageError::Io` - If file system operation failed
    async fn load_piece(
        &self,
        info_hash: InfoHash,
        index: PieceIndex,
    ) -> Result<Vec<u8>, StorageError>;

    /// Checks if piece exists in storage.
    ///
    /// # Errors
    ///
    /// - `StorageError::Io` - If file system operation failed
    async fn has_piece(&self, info_hash: InfoHash, index: PieceIndex)
    -> Result<bool, StorageError>;

    /// Finalizes completed torrent and moves to library.
    ///
    /// Returns final path where torrent data is stored.
    ///
    /// # Errors
    ///
    /// - `StorageError::Io` - If file system operation failed
    async fn finalize_torrent(&mut self, info_hash: InfoHash) -> Result<PathBuf, StorageError>;
}

/// Errors that occur during storage operations.
///
/// Covers file system errors, disk space issues, and data corruption
/// during piece storage and retrieval operations.
#[derive(Debug, thiserror::Error)]
pub enum StorageError {
    /// Requested piece has not been downloaded yet
    #[error("Piece {index} not found")]
    PieceNotFound {
        /// Index of the piece that was not found
        index: PieceIndex,
    },

    /// Not enough disk space available for the operation
    #[error("Insufficient disk space: need {needed} bytes, have {available}")]
    InsufficientSpace {
        /// Number of bytes required for the operation
        needed: u64,
        /// Number of bytes currently available
        available: u64,
    },

    /// File system specific error occurred
    #[error("File system error: {message}")]
    FilesystemError {
        /// Description of the file system error
        message: String,
    },

    /// Standard I/O error occurred
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
}
