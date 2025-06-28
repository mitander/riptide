//! Riptide Search - Media search and discovery
//!
//! Provides search capabilities across multiple media sources.

pub mod errors;
pub mod metadata;
pub mod providers;
pub mod service;
pub mod types;

// Re-export main types
pub use errors::MediaSearchError;
pub use metadata::{ImdbMetadataService, MediaMetadata};
pub use service::MediaSearchService;
pub use types::{MediaSearchResult, MediaType, TorrentResult, VideoQuality};

// Type alias for convenience
pub type Result<T> = std::result::Result<T, MediaSearchError>;
