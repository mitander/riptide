//! Riptide - Production-grade torrent media server
//!
//! A Rust-based BitTorrent client optimized for streaming media content.
//! Supports simulation environments for rapid development and testing.

pub mod cli;
pub mod simulation;
pub mod storage;
pub mod torrent;

pub use torrent::TorrentEngine;

/// Common error type used throughout Riptide
#[derive(Debug, thiserror::Error)]
pub enum RiptideError {
    #[error("Torrent error: {0}")]
    Torrent(#[from] torrent::TorrentError),
    
    #[error("Storage error: {0}")]
    Storage(#[from] storage::StorageError),
    
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
}

pub type Result<T> = std::result::Result<T, RiptideError>;