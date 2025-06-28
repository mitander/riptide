//! Error types for media search functionality.

use thiserror::Error;

/// Errors that can occur during media search operations.
#[derive(Debug, Error)]
pub enum MediaSearchError {
    #[error("Search failed for query '{query}': {reason}")]
    SearchFailed { query: String, reason: String },

    #[error("Network error: {reason}")]
    NetworkError { reason: String },

    #[error("Parse error: {reason}")]
    ParseError { reason: String },

    #[error("Provider error: {reason}")]
    ProviderError { reason: String },

    #[error("Invalid media type: {media_type}")]
    InvalidMediaType { media_type: String },

    #[error("Metadata fetch failed: {reason}")]
    MetadataFetchFailed { reason: String },
}
