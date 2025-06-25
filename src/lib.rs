//! Riptide - A media server designed for streaming torrented media
//!
//! A Rust-based BitTorrent client optimized for streaming media content.
//! Supports simulation environments for rapid development and testing.

pub mod cli;
pub mod config;
pub mod domain;
pub mod infrastructure;
pub mod media_search;
pub mod services;
pub mod storage;
pub mod streaming;
pub mod torrent;
pub mod web;

// Optional simulation module for development
#[cfg(feature = "simulation")]
pub mod simulation;

pub use torrent::TorrentEngine;

/// Top-level errors for the Riptide application.
///
/// High-level error types that can bubble up from any Riptide subsystem.
/// These represent failures in core application functionality.
#[derive(Debug, thiserror::Error)]
pub enum RiptideError {
    #[error("Domain error: {0}")]
    Domain(#[from] domain::DomainError),

    #[error("Torrent error: {0}")]
    Torrent(#[from] torrent::TorrentError),

    #[error("Storage error: {0}")]
    Storage(#[from] storage::StorageError),

    #[error("Web UI error: {0}")]
    WebUI(#[from] web::WebUIError),

    #[error("Media search error: {0}")]
    MediaSearch(#[from] media_search::MediaSearchError),

    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
}

impl RiptideError {
    /// Returns a user-friendly error message suitable for display in UI.
    pub fn user_message(&self) -> String {
        match self {
            RiptideError::Domain(e) => e.user_message(),
            RiptideError::Torrent(e) => match e {
                torrent::TorrentError::InvalidTorrentFile { reason } => {
                    format!("Invalid torrent file: {reason}")
                }
                torrent::TorrentError::TrackerConnectionFailed { url } => {
                    format!("Could not connect to tracker: {url}")
                }
                torrent::TorrentError::NoPeersAvailable => {
                    "No peers available for download".to_string()
                }
                _ => "Download error occurred".to_string(),
            },
            RiptideError::Storage(_) => "Storage error occurred".to_string(),
            RiptideError::WebUI(_) => "Web interface error occurred".to_string(),
            RiptideError::MediaSearch(_) => "Media search failed".to_string(),
            RiptideError::Io(_) => "File system error occurred".to_string(),
        }
    }

    /// Checks if this error is due to user input validation.
    pub fn is_user_error(&self) -> bool {
        matches!(self, RiptideError::Domain(e) if e.is_validation_error())
    }
}

pub type Result<T> = std::result::Result<T, RiptideError>;
