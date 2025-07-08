//! Server component abstractions for dependency injection.
//!
//! This module provides shared types for server components that can be
//! configured differently between production and development modes.

use std::collections::HashMap;
use std::sync::Arc;

use tokio::sync::RwLock;

use crate::storage::FileLibraryManager;
use crate::streaming::FfmpegProcessor;
use crate::torrent::{PieceStore, TorrentEngineHandle};

/// Progress tracking for background movie conversions.
#[derive(Debug, Clone, serde::Serialize)]
pub struct ConversionProgress {
    pub movie_title: String,
    pub status: ConversionStatus,
    #[serde(serialize_with = "serialize_instant")]
    pub started_at: std::time::Instant,
    #[serde(serialize_with = "serialize_optional_instant")]
    pub completed_at: Option<std::time::Instant>,
    pub error_message: Option<String>,
}

/// Status of background movie conversion to torrent format.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub enum ConversionStatus {
    Pending,
    Converting,
    Completed,
    Failed,
}

/// Pre-configured server components for dependency injection.
///
/// Contains all runtime services needed by the web server. Services are created
/// based on runtime mode (Production vs Development) and passed to the web layer
/// which remains completely mode-agnostic.
pub struct ServerComponents {
    pub torrent_engine: TorrentEngineHandle,
    pub movie_manager: Option<Arc<RwLock<FileLibraryManager>>>,
    pub piece_store: Option<Arc<dyn PieceStore>>,
    pub conversion_progress: Option<Arc<RwLock<HashMap<String, ConversionProgress>>>>,
    pub ffmpeg_processor: Arc<dyn FfmpegProcessor>,
}

impl ServerComponents {
    /// Get the torrent engine handle.
    pub fn engine(&self) -> &TorrentEngineHandle {
        &self.torrent_engine
    }

    /// Get the file manager if available (Development mode only).
    ///
    /// # Errors
    /// Returns `ServiceError::NotAvailable` if file manager is not available in this mode.
    pub fn file_manager(&self) -> Result<&Arc<RwLock<FileLibraryManager>>, ServiceError> {
        self.movie_manager
            .as_ref()
            .ok_or(ServiceError::NotAvailable(
                "File manager not available in this mode",
            ))
    }

    /// Get the piece store if available (Development mode only).
    ///
    /// # Errors
    /// Returns `ServiceError::NotAvailable` if piece store is not available in this mode.
    pub fn piece_store(&self) -> Result<&Arc<dyn PieceStore>, ServiceError> {
        self.piece_store.as_ref().ok_or(ServiceError::NotAvailable(
            "Piece store not available in this mode",
        ))
    }

    /// Get conversion progress tracker if available (Development mode only).
    ///
    /// # Errors
    /// Returns `ServiceError::NotAvailable` if conversion tracking is not available in this mode.
    pub fn conversion_progress(
        &self,
    ) -> Result<&Arc<RwLock<HashMap<String, ConversionProgress>>>, ServiceError> {
        self.conversion_progress
            .as_ref()
            .ok_or(ServiceError::NotAvailable(
                "Conversion tracking not available in this mode",
            ))
    }

    /// Get the FFmpeg processor for remuxing operations.
    pub fn ffmpeg_processor(&self) -> &Arc<dyn FfmpegProcessor> {
        &self.ffmpeg_processor
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ServiceError {
    #[error("Service not available: {0}")]
    NotAvailable(&'static str),
}

fn serialize_instant<S>(instant: &std::time::Instant, serializer: S) -> Result<S::Ok, S::Error>
where
    S: serde::Serializer,
{
    let elapsed = instant.elapsed().as_secs();
    serializer.serialize_u64(elapsed)
}

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
