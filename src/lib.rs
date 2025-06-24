//! Riptide - A media server designed for streaming torrented media
//!
//! A Rust-based BitTorrent client optimized for streaming media content.
//! Supports simulation environments for rapid development and testing.

pub mod cli;
pub mod config;
pub mod simulation;
pub mod storage;
pub mod streaming;
pub mod torrent;
pub mod web;

pub use torrent::TorrentEngine;

/// Top-level errors for the Riptide application.
///
/// High-level error types that can bubble up from any Riptide subsystem.
/// These represent failures in core application functionality.
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
