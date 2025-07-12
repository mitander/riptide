//! Error types for media search functionality.

use thiserror::Error;

/// Errors that can occur during media search operations.
#[derive(Debug, Error)]
pub enum MediaSearchError {
    /// Search operation failed with the specified query and reason.
    #[error("Search failed for query '{query}': {reason}")]
    SearchFailed {
        /// The search query that failed
        query: String,
        /// The reason for the failure
        reason: String,
    },

    /// Network communication error occurred during search.
    #[error("Network error: {reason}")]
    NetworkError {
        /// The reason for the network error
        reason: String,
    },

    /// Failed to parse search results or response data.
    #[error("Parse error: {reason}")]
    ParseError {
        /// The reason for the parse error
        reason: String,
    },

    /// Search provider returned an error or is unavailable.
    #[error("Provider error: {reason}")]
    ProviderError {
        /// The reason for the provider error
        reason: String,
    },

    /// Unsupported or invalid media type specified.
    #[error("Invalid media type: {media_type}")]
    InvalidMediaType {
        /// The invalid media type that was specified
        media_type: String,
    },

    /// Failed to fetch metadata for search results.
    #[error("Metadata fetch failed: {reason}")]
    MetadataFetchFailed {
        /// The reason for the metadata fetch failure
        reason: String,
    },
}
