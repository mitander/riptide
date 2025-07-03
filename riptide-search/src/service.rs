//! Media search and metadata integration
//!
//! Provides media discovery functionality using Magneto for torrent searches
//! and future IMDb integration for metadata and artwork.

use crate::errors::MediaSearchError;
use crate::metadata::ImdbMetadataService;
#[cfg(test)]
use crate::providers::MockProvider;
use crate::providers::{DevelopmentProvider, MagnetoProvider, TorrentSearchProvider};
use crate::types::{MediaSearchResult, TorrentResult};

/// Media search service providing torrent discovery and metadata.
#[derive(Debug)]
pub struct MediaSearchService {
    provider: Box<dyn TorrentSearchProvider>,
    metadata_service: ImdbMetadataService,
    is_development: bool,
}

impl Clone for MediaSearchService {
    fn clone(&self) -> Self {
        if self.is_development {
            Self::new_development()
        } else {
            Self::new()
        }
    }
}

impl MediaSearchService {
    /// Creates new media search service with production providers.
    ///
    /// Uses MagnetoProvider with PirateBay and YTS indexers.
    pub fn new() -> Self {
        Self {
            provider: Box::new(MagnetoProvider::new()),
            metadata_service: ImdbMetadataService::new(),
            is_development: false,
        }
    }

    /// Creates new media search service with development data for offline development.
    ///
    /// Uses rich development data for UI development and testing without external API calls.
    /// Development data includes multiple quality options and realistic metadata.
    /// Same interface as production but works completely offline.
    pub fn new_development() -> Self {
        Self {
            provider: Box::new(DevelopmentProvider::new()),
            metadata_service: ImdbMetadataService::new(),
            is_development: true,
        }
    }

    /// Creates new media search service based on runtime mode.
    ///
    /// Uses runtime mode to choose between development and production providers.
    /// Production mode uses fallback provider for reliability.
    pub fn from_runtime_mode(mode: riptide_core::RuntimeMode) -> Self {
        if mode.is_development() {
            Self::new_development()
        } else {
            let omdb_api_key = std::env::var("OMDB_API_KEY").ok();

            let provider = Box::new(MagnetoProvider::new());
            let metadata_service = ImdbMetadataService::with_api_key(omdb_api_key);

            Self {
                provider,
                metadata_service,
                is_development: false,
            }
        }
    }

    /// Creates new media search service with mock provider for testing.
    #[cfg(test)]
    pub fn new_with_mock() -> Self {
        Self {
            provider: Box::new(MockProvider::new()),
            metadata_service: ImdbMetadataService::new(),
            is_development: true,
        }
    }

    /// Search for movies using query string.
    ///
    /// # Errors
    /// - `MediaSearchError::SearchFailed` - Failed to query provider
    /// - `MediaSearchError::NetworkError` - Network connectivity issues
    pub async fn search_movies(
        &self,
        query: &str,
    ) -> Result<Vec<MediaSearchResult>, MediaSearchError> {
        self.provider.search_torrents(query, "movie").await
    }

    /// Search for TV shows using query string.
    ///
    /// # Errors
    /// - `MediaSearchError::SearchFailed` - Failed to query provider
    /// - `MediaSearchError::NetworkError` - Network connectivity issues
    pub async fn search_tv_shows(
        &self,
        query: &str,
    ) -> Result<Vec<MediaSearchResult>, MediaSearchError> {
        self.provider.search_torrents(query, "tv").await
    }

    /// Search for any media type using query string.
    ///
    /// # Errors
    /// - `MediaSearchError::SearchFailed` - Failed to query provider
    /// - `MediaSearchError::NetworkError` - Network connectivity issues
    pub async fn search_all(
        &self,
        query: &str,
    ) -> Result<Vec<MediaSearchResult>, MediaSearchError> {
        self.provider.search_torrents(query, "all").await
    }

    /// Search with enhanced IMDb metadata integration.
    ///
    /// Performs torrent search and enriches results with IMDb data including
    /// posters, ratings, plot summaries, and detailed metadata.
    ///
    /// # Errors
    /// - `MediaSearchError::SearchFailed` - Failed to query provider
    /// - `MediaSearchError::NetworkError` - Network connectivity issues
    pub async fn search_with_metadata(
        &self,
        query: &str,
    ) -> Result<Vec<MediaSearchResult>, MediaSearchError> {
        let mut results = self.search_all(query).await?;

        // Enhance each result with IMDb metadata
        for result in &mut results {
            // Skip if we already have poster and plot from provider
            if result.poster_url.is_some() && result.plot.is_some() {
                continue;
            }

            if let Ok(metadata) = self
                .metadata_service
                .search_by_title(&result.title, result.year)
                .await
            {
                // Only update fields that are empty
                if result.imdb_id.is_none() {
                    result.imdb_id = metadata.imdb_id;
                }
                if result.poster_url.is_none() {
                    result.poster_url = metadata.poster_url;
                }
                if result.plot.is_none() {
                    result.plot = metadata.plot;
                }
                if result.genre.is_none() {
                    result.genre = metadata.genre;
                }
                if result.rating.is_none() {
                    result.rating = metadata.rating;
                }
            }
            // Continue even if metadata fetch fails - we still have torrent data
        }

        Ok(results)
    }

    /// Get detailed torrent results for media.
    ///
    /// # Errors
    /// - `MediaSearchError::SearchFailed` - Failed to retrieve torrent details
    pub async fn get_media_torrents(
        &self,
        media_title: &str,
    ) -> Result<Vec<TorrentResult>, MediaSearchError> {
        let results = self.search_all(media_title).await?;

        let mut torrents = Vec::new();
        for result in results {
            torrents.extend(result.torrents);
        }

        // Sort by priority score (combines quality and seeders)
        torrents.sort_by_key(|b| std::cmp::Reverse(b.priority_score()));

        Ok(torrents)
    }
}

impl Default for MediaSearchService {
    fn default() -> Self {
        Self::new()
    }
}

/// Helper function to extract clean media title from search query.
///
/// Removes common search artifacts like year, quality indicators, etc.
pub fn extract_media_title(query: &str) -> String {
    // Simple implementation - can be enhanced with regex patterns
    query
        .replace("1080p", "")
        .replace("720p", "")
        .replace("4K", "")
        .replace("BluRay", "")
        .replace("WEB-DL", "")
        .replace("HDTV", "")
        .trim()
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_development_provider_search() {
        let service = MediaSearchService::new_development();
        let results = service.search_movies("Test Movie").await.unwrap();

        assert!(!results.is_empty());
        assert_eq!(results[0].title, "Test Movie");
        assert!(!results[0].torrents.is_empty());
    }

    #[tokio::test]
    async fn test_torrent_result_format_size() {
        let torrent = TorrentResult {
            name: "Test.Movie.1080p.BluRay.x264".to_string(),
            magnet_link: "magnet:?xt=urn:btih:test123".to_string(),
            size: 1_500_000_000,
            seeders: 0,
            leechers: 0,
            quality: crate::types::VideoQuality::BluRay1080p,
            source: "test".to_string(),
            added_date: chrono::Utc::now(),
        };

        assert_eq!(torrent.format_size(), "1.4 GB");
    }

    #[tokio::test]
    async fn test_mock_provider() {
        let service = MediaSearchService::new_with_mock();
        let results = service.search_movies("Test Movie").await.unwrap();

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].title, "Test Movie");
        assert_eq!(results[0].torrents.len(), 2);
    }

    #[test]
    fn test_extract_media_title() {
        assert_eq!(
            extract_media_title("Movie Title 1080p BluRay"),
            "Movie Title"
        );
        assert_eq!(
            extract_media_title("Show Name S01E01 720p"),
            "Show Name S01E01"
        );
        assert_eq!(extract_media_title("Clean Title"), "Clean Title");
    }
}
