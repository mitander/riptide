//! Riptide Search - Media search and discovery
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

// Type alias for convenience
pub type Result<T> = std::result::Result<T, MediaSearchError>;
