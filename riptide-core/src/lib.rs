//! Riptide Core - Essential BitTorrent and streaming functionality
//!
//! This crate provides the fundamental building blocks for BitTorrent-based
//! media streaming: torrent protocol implementation, file storage, streaming
//! services, and configuration management.

pub mod config;
pub mod storage;
pub mod streaming;
pub mod torrent;

// Optional simulation module for development
#[cfg(feature = "simulation")]
pub mod simulation;

// Re-export main types for convenient access
pub use config::{RiptideConfig, SimulationConfig};
pub use storage::{FileStorage, StorageError};
pub use streaming::{DirectStreamingService, StreamingError};
pub use torrent::{TorrentEngine, TorrentError};

/// Core errors that can bubble up from any Riptide subsystem.
///
/// High-level error types representing failures in core functionality.
#[derive(Debug, thiserror::Error)]
pub enum RiptideError {
    #[error("Torrent error: {0}")]
    Torrent(#[from] TorrentError),

    #[error("Storage error: {0}")]
    Storage(#[from] StorageError),

    #[error("Streaming error: {0}")]
    Streaming(#[from] StreamingError),

    #[error("Configuration error: {reason}")]
    Configuration { reason: String },

    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
}

impl RiptideError {
    /// Returns a user-friendly error message suitable for display.
    pub fn user_message(&self) -> String {
        match self {
            RiptideError::Torrent(e) => match e {
                TorrentError::InvalidTorrentFile { reason } => {
                    format!("Invalid torrent file: {reason}")
                }
                TorrentError::TrackerConnectionFailed { url } => {
                    format!("Could not connect to tracker: {url}")
                }
                TorrentError::NoPeersAvailable => "No peers available for download".to_string(),
                _ => "Download error occurred".to_string(),
            },
            RiptideError::Storage(_) => "Storage error occurred".to_string(),
            RiptideError::Streaming(_) => "Streaming error occurred".to_string(),
            RiptideError::Configuration { .. } => "Configuration error occurred".to_string(),
            RiptideError::Io(_) => "File system error occurred".to_string(),
        }
    }

    /// Checks if this error is due to user input validation.
    pub fn is_user_error(&self) -> bool {
        matches!(
            self,
            RiptideError::Configuration { .. }
                | RiptideError::Torrent(TorrentError::InvalidTorrentFile { .. })
        )
    }
}

pub type Result<T> = std::result::Result<T, RiptideError>;
