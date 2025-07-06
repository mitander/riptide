//! Server component abstractions for dependency injection.
//!
//! This module provides shared types for server components that can be
//! configured differently between production and development modes.

use std::collections::HashMap;
use std::sync::Arc;

use tokio::sync::RwLock;

use crate::storage::FileLibraryManager;
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
/// This struct contains all components that vary between production and development modes.
pub struct ServerComponents {
    pub torrent_engine: TorrentEngineHandle,
    pub movie_manager: Option<Arc<RwLock<FileLibraryManager>>>,
    pub piece_store: Option<Arc<dyn PieceStore>>,
    pub conversion_progress: Option<Arc<RwLock<HashMap<String, ConversionProgress>>>>,
}

// Serde helper functions for Instant serialization
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
