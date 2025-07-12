//! Riptide Core - BitTorrent and streaming functionality

#![deny(missing_docs)]
#![deny(clippy::missing_errors_doc)]
#![deny(clippy::missing_panics_doc)]
#![warn(clippy::too_many_lines)]
//!
//! This crate provides the fundamental building blocks for BitTorrent-based
//! media streaming: torrent protocol implementation, file storage, streaming
//! services, and configuration management.

pub mod config;
pub mod engine;
pub mod mode;
pub mod network;
pub mod server_components;
pub mod storage;
pub mod streaming;
pub mod torrent;
pub mod tracing_setup;
pub mod video;

// Re-export main types for convenient access
pub use config::RiptideConfig;
pub use mode::RuntimeMode;
pub use server_components::{ConversionProgress, ConversionStatus, ServerComponents};
pub use storage::{FileLibrary, FileStorage, LibraryFile, StorageError};
pub use streaming::{HttpStreaming, StreamingError};
pub use torrent::{EngineStats, TorrentCreator, TorrentEngineHandle, TorrentError};
pub use tracing_setup::{CliLogLevel, init_tracing};

/// Core errors that can bubble up from any Riptide subsystem.
///
/// High-level error types representing failures in core functionality.
#[derive(Debug, thiserror::Error)]
pub enum RiptideError {
    /// Torrent-related errors (downloading, parsing, peer communication)
    #[error("Torrent error: {0}")]
    Torrent(#[from] TorrentError),

    /// Storage layer errors (file I/O, piece verification, disk space)
    #[error("Storage error: {0}")]
    Storage(#[from] StorageError),

    /// Streaming service errors (remuxing, transcoding, protocol issues)
    #[error("Streaming error: {0}")]
    Streaming(#[from] StreamingError),

    /// Configuration parsing or validation errors
    #[error("Configuration error: {reason}")]
    Configuration {
        /// Human-readable description of the configuration error
        reason: String,
    },

    /// Standard I/O errors from filesystem operations
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    /// Web interface errors (templating, routing, asset serving)
    #[error("Web UI error: {reason}")]
    WebUI {
        /// Human-readable description of the web UI error
        reason: String,
    },
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
                TorrentError::TorrentNotFound { info_hash } => {
                    format!("Torrent {info_hash} not found")
                }
                TorrentError::NoPeersAvailable => "No peers available for download".to_string(),
                _ => "Download error occurred".to_string(),
            },
            RiptideError::Storage(_) => "Storage error occurred".to_string(),
            RiptideError::Streaming(_) => "Streaming error occurred".to_string(),
            RiptideError::Configuration { .. } => "Configuration error occurred".to_string(),
            RiptideError::Io(_) => "File system error occurred".to_string(),
            RiptideError::WebUI { reason } => format!("Web interface error: {reason}"),
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

/// Convenience Result type using RiptideError as the error type
pub type Result<T> = std::result::Result<T, RiptideError>;

/// Convert WebUIError to RiptideError
impl RiptideError {
    /// Convert a web UI error to RiptideError.
    ///
    /// Wraps any display-able error from the web UI layer into a RiptideError::WebUI variant.
    pub fn from_web_ui_error(error: impl std::fmt::Display) -> Self {
        RiptideError::WebUI {
            reason: error.to_string(),
        }
    }
}
