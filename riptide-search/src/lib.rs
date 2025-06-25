//! Riptide Search - Media search and discovery
//!
//! Provides search capabilities across multiple media sources.

mod service;

// Re-export main types  
pub use service::{MediaSearchResult, MediaSearchService, TorrentResult};

/// Search-specific errors.
#[derive(Debug, thiserror::Error)]
pub enum SearchError {
    #[error("Search provider failed: {reason}")]
    ProviderFailed { reason: String },

    #[error("Network error: {0}")]
    Network(#[from] reqwest::Error),

    #[error("Parsing error: {reason}")]
    ParsingError { reason: String },
}

pub type Result<T> = std::result::Result<T, SearchError>;