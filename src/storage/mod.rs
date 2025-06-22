//! Storage layer for torrent data

pub mod file_storage;

pub use file_storage::FileStorage;

use crate::torrent::{InfoHash, PieceIndex};
use std::path::PathBuf;

/// Storage operations for torrent data
#[async_trait::async_trait]
pub trait Storage: Send + Sync {
    /// Store a piece of data
    async fn store_piece(&mut self, info_hash: InfoHash, index: PieceIndex, data: &[u8]) -> Result<(), StorageError>;
    
    /// Retrieve a piece of data
    async fn load_piece(&self, info_hash: InfoHash, index: PieceIndex) -> Result<Vec<u8>, StorageError>;
    
    /// Check if piece exists and is verified
    async fn has_piece(&self, info_hash: InfoHash, index: PieceIndex) -> Result<bool, StorageError>;
    
    /// Mark torrent as complete and move to library
    async fn finalize_torrent(&mut self, info_hash: InfoHash) -> Result<PathBuf, StorageError>;
}

/// Storage-related errors
#[derive(Debug, thiserror::Error)]
pub enum StorageError {
    #[error("Piece {index} not found")]
    PieceNotFound { index: PieceIndex },
    
    #[error("Insufficient disk space: need {needed} bytes, have {available}")]
    InsufficientSpace { needed: u64, available: u64 },
    
    #[error("File system error: {message}")]
    FilesystemError { message: String },
    
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
}