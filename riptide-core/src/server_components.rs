//! Server component abstractions for dependency injection.
//!
//! This module provides shared types for server components that can be
//! configured differently between production and development modes.

use std::collections::HashMap;
use std::sync::Arc;

use tokio::sync::RwLock;

use crate::engine::TorrentEngineHandle;
use crate::storage::FileLibrary;
use crate::streaming::Ffmpeg;
use crate::torrent::PieceStore;

// Type aliases for complex types
type ConversionProgressMap = Arc<RwLock<HashMap<String, ConversionProgress>>>;
type FileLibraryHandle = Arc<RwLock<FileLibrary>>;

/// Progress tracking for background movie conversions.
#[derive(Debug, Clone, serde::Serialize)]
pub struct ConversionProgress {
    /// Title of the movie being converted
    pub movie_title: String,
    /// Current status of the conversion process
    pub status: ConversionStatus,
    /// When the conversion was started
    #[serde(serialize_with = "serialize_instant")]
    pub started_at: std::time::Instant,
    /// When the conversion completed (if finished)
    #[serde(serialize_with = "serialize_optional_instant")]
    pub completed_at: Option<std::time::Instant>,
    /// Error message if conversion failed
    pub error_message: Option<String>,
}

/// Status of background movie conversion to torrent format.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub enum ConversionStatus {
    /// Conversion is queued but not yet started
    Pending,
    /// Conversion is currently in progress
    Converting,
    /// Conversion completed successfully
    Completed,
    /// Conversion failed with an error
    Failed,
}

/// Pre-configured server components for dependency injection.
///
/// Contains all runtime services needed by the web server. Services are created
/// based on runtime mode (Production vs Development) and passed to the web layer
/// which remains completely mode-agnostic.
pub struct ServerComponents {
    /// Handle to the torrent engine for downloading and managing torrents
    pub torrent_engine: TorrentEngineHandle,
    /// File library for movie management (Development mode only)
    pub movie_library: Option<FileLibraryHandle>,
    /// Piece store for torrent piece data (Development mode only)
    pub piece_store: Option<Arc<dyn PieceStore>>,
    /// Progress tracker for background conversions (Development mode only)
    pub conversion_progress: Option<ConversionProgressMap>,
    /// FFmpeg processor for media transcoding and remuxing
    pub ffmpeg: Arc<dyn Ffmpeg>,
}

impl ServerComponents {
    /// Get the torrent engine handle.
    pub fn engine(&self) -> &TorrentEngineHandle {
        &self.torrent_engine
    }

    /// Get the file manager if available (Development mode only).
    ///
    /// # Errors
    ///
    /// - `ServiceError::NotAvailable` - If file manager is not available in this mode
    pub fn file_library(&self) -> Result<&FileLibraryHandle, ServiceError> {
        self.movie_library
            .as_ref()
            .ok_or(ServiceError::NotAvailable(
                "File manager not available in this mode",
            ))
    }

    /// Get the piece store if available (Development mode only).
    ///
    /// # Errors
    ///
    /// - `ServiceError::NotAvailable` - If piece store is not available in this mode
    pub fn piece_store(&self) -> Result<&Arc<dyn PieceStore>, ServiceError> {
        self.piece_store.as_ref().ok_or(ServiceError::NotAvailable(
            "Piece store not available in this mode",
        ))
    }

    /// Get conversion progress tracker if available (Development mode only).
    ///
    /// # Errors
    ///
    /// - `ServiceError::NotAvailable` - If conversion tracking is not available in this mode
    pub fn conversion_progress(&self) -> Result<&ConversionProgressMap, ServiceError> {
        self.conversion_progress
            .as_ref()
            .ok_or(ServiceError::NotAvailable(
                "Conversion tracking not available in this mode",
            ))
    }

    /// Get the FFmpeg processor for remuxing operations.
    pub fn ffmpeg(&self) -> &Arc<dyn Ffmpeg> {
        &self.ffmpeg
    }
}

/// Errors that can occur when accessing server services.
#[derive(Debug, thiserror::Error)]
pub enum ServiceError {
    /// The requested service is not available in the current runtime mode
    #[error("Service not available: {0}")]
    NotAvailable(&'static str),
}

/// Serialize an Instant as elapsed seconds for JSON compatibility.
fn serialize_instant<S>(instant: &std::time::Instant, serializer: S) -> Result<S::Ok, S::Error>
where
    S: serde::Serializer,
{
    let elapsed = instant.elapsed().as_secs();
    serializer.serialize_u64(elapsed)
}

/// Serialize an optional Instant as elapsed seconds for JSON compatibility.
fn serialize_optional_instant<S>(
    instant: &Option<std::time::Instant>,
    serializer: S,
) -> Result<S::Ok, S::Error>
where
    S: serde::Serializer,
{
    match instant {
        Some(i) => {
            let elapsed = i.elapsed().as_secs();
            serializer.serialize_some(&elapsed)
        }
        None => serializer.serialize_none(),
    }
}
