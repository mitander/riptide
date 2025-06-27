//! Media search and metadata integration
//!
//! Provides media discovery functionality using Magneto for torrent searches
//! and future IMDb integration for metadata and artwork.

use crate::errors::MediaSearchError;
#[cfg(test)]
use crate::providers::MockProvider;
use crate::providers::{DemoProvider, TorrentSearchProvider};
use crate::types::{MediaSearchResult, TorrentResult};

/// Media search service providing torrent discovery and metadata.
#[derive(Debug)]
pub struct MediaSearchService {
    provider: Box<dyn TorrentSearchProvider>,
}

impl Clone for MediaSearchService {
    fn clone(&self) -> Self {
        // For now, always create a new provider
        // In future, could implement Clone for the trait or use Arc
        Self::new()
    }
}

impl MediaSearchService {
    /// Creates new media search service with real Magneto provider.
    ///
    /// TODO: Currently returns demo data until real Magneto API is implemented.
    pub fn new() -> Self {
        Self {
            provider: Box::new(DemoProvider::new()),
        }
    }

    /// Creates new media search service with demo data for development.
    ///
    /// Uses rich demo data for UI development and testing without external API calls.
    /// Demo data includes multiple quality options and realistic metadata.
    pub fn new_demo() -> Self {
        Self {
            provider: Box::new(DemoProvider::new()),
        }
    }

    /// Creates new media search service with mock provider for testing.
    #[cfg(test)]
    pub fn new_with_mock() -> Self {
        Self {
            provider: Box::new(MockProvider::new()),
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
    async fn test_demo_provider_search() {
        let service = MediaSearchService::new_demo();
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
