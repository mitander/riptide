//! Provider implementations for torrent search functionality.

use async_trait::async_trait;

use crate::errors::MediaSearchError;
use crate::types::MediaSearchResult;

pub mod development;
pub mod magneto;
pub mod mock;

pub use development::DevelopmentProvider;
pub use magneto::MagnetoProvider;
#[cfg(test)]
pub use mock::MockProvider;

/// Trait for torrent search providers.
///
/// Implementations provide media search functionality through different backends
/// (development data, real APIs, mock providers for testing).
#[async_trait]
pub trait TorrentSearchProvider: Send + Sync + std::fmt::Debug {
    /// Search for torrents by query and optional category filter.
    ///
    /// # Errors
    /// - `MediaSearchError::SearchFailed` - Search operation failed
    /// - `MediaSearchError::NetworkError` - Network connectivity issues
    /// - `MediaSearchError::ProviderError` - Provider-specific error
    async fn search_torrents(
        &self,
        query: &str,
        category: &str,
    ) -> Result<Vec<MediaSearchResult>, MediaSearchError>;
}
