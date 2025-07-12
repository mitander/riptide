//! Mock provider implementation for testing.

#[cfg(test)]
use async_trait::async_trait;

#[cfg(test)]
use super::TorrentSearchProvider;
#[cfg(test)]
use crate::errors::MediaSearchError;
#[cfg(test)]
use crate::types::{MediaSearchResult, MediaType, TorrentResult, VideoQuality};

/// Mock provider for testing.
#[cfg(test)]
#[derive(Debug, Default)]
pub struct MockProvider;

#[cfg(test)]
impl MockProvider {
    /// Creates a new mock provider for testing.
    pub fn new() -> Self {
        Self
    }
}

#[cfg(test)]
#[async_trait]
impl TorrentSearchProvider for MockProvider {
    async fn search_torrents(
        &self,
        query: &str,
        _category: &str,
    ) -> Result<Vec<MediaSearchResult>, MediaSearchError> {
        // Return mock data based on query
        let mock_torrents = vec![
            TorrentResult {
                name: format!("{}.2024.1080p.BluRay.x264-GROUP", query.replace(' ', ".")),
                magnet_link: "magnet:?xt=urn:btih:mock123".to_string(),
                size: 1_500_000_000,
                seeders: 50,
                leechers: 10,
                quality: VideoQuality::BluRay1080p,
                source: "Mock".to_string(),
                added_date: chrono::Utc::now(),
            },
            TorrentResult {
                name: format!("{}.2024.720p.WebRip.x264", query.replace(' ', ".")),
                magnet_link: "magnet:?xt=urn:btih:mock456".to_string(),
                size: 800_000_000,
                seeders: 25,
                leechers: 5,
                quality: VideoQuality::BluRay720p,
                source: "Mock".to_string(),
                added_date: chrono::Utc::now(),
            },
        ];

        Ok(vec![MediaSearchResult {
            title: query.to_string(),
            year: Some(2024),
            media_type: MediaType::Movie,
            imdb_id: Some("tt1234567".to_string()),
            poster_url: Some("/static/posters/mock.jpg".to_string()),
            plot: Some("Mock plot description".to_string()),
            genre: Some("Action".to_string()),
            rating: Some(8.5),
            torrents: mock_torrents,
        }])
    }
}
