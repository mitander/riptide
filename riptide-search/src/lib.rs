//! Riptide Search - Media search and discovery

#![deny(missing_docs)]
#![deny(clippy::missing_errors_doc)]
#![deny(clippy::missing_panics_doc)]
#![warn(clippy::too_many_lines)]
//!
//! Provides search capabilities across multiple media sources with fuzzy matching
//! and rich IMDb metadata integration for streaming-optimized movie discovery.

pub mod enhanced_search;
pub mod errors;
pub mod metadata;
pub mod providers;
pub mod search;
pub mod types;

// Re-export main types
pub use enhanced_search::{EnhancedMediaSearch, FuzzySearchConfig, MovieSearchResult};
pub use errors::MediaSearchError;
pub use metadata::{ImdbMetadata, MediaMetadata};
pub use search::MediaSearch;
pub use types::{MediaSearchResult, MediaType, TorrentResult, VideoQuality};

/// Convenience type alias for Results with MediaSearchError.
pub type Result<T> = std::result::Result<T, MediaSearchError>;
