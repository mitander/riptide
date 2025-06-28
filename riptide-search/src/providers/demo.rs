//! Demo provider implementation for development and testing.

use async_trait::async_trait;

use super::TorrentSearchProvider;
use crate::errors::MediaSearchError;
use crate::types::{MediaSearchResult, MediaType, TorrentResult, VideoQuality};

/// Demo provider for development and testing.
///
/// Returns realistic demo data for UI development without external API calls.
/// Includes multiple quality options, realistic file sizes, and seeder counts.
#[derive(Debug)]
pub struct DemoProvider;

impl Default for DemoProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl DemoProvider {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl TorrentSearchProvider for DemoProvider {
    async fn search_torrents(
        &self,
        query: &str,
        category: &str,
    ) -> Result<Vec<MediaSearchResult>, MediaSearchError> {
        // TEMPORARY: Using demo data while we implement a working API
        // This allows the complete search UI workflow to be tested and demonstrated
        let mock_torrents = vec![
            TorrentResult {
                name: format!("{}.2024.1080p.BluRay.x264-DEMO", query.replace(' ', ".")),
                magnet_link: format!(
                    "magnet:?xt=urn:btih:demo123&dn={}",
                    urlencoding::encode(query)
                ),
                size: 1_500_000_000,
                seeders: 150,
                leechers: 25,
                quality: VideoQuality::BluRay1080p,
                source: "Demo".to_string(),
                added_date: chrono::Utc::now(),
            },
            TorrentResult {
                name: format!("{}.2024.720p.WEB-DL.x264-DEMO", query.replace(' ', ".")),
                magnet_link: format!(
                    "magnet:?xt=urn:btih:demo456&dn={}",
                    urlencoding::encode(query)
                ),
                size: 800_000_000,
                seeders: 95,
                leechers: 15,
                quality: VideoQuality::BluRay720p,
                source: "Demo".to_string(),
                added_date: chrono::Utc::now(),
            },
            TorrentResult {
                name: format!(
                    "{}.2024.2160p.UHD.BluRay.x265-DEMO",
                    query.replace(' ', ".")
                ),
                magnet_link: format!(
                    "magnet:?xt=urn:btih:demo789&dn={}",
                    urlencoding::encode(query)
                ),
                size: 4_500_000_000,
                seeders: 45,
                leechers: 8,
                quality: VideoQuality::BluRay4K,
                source: "Demo".to_string(),
                added_date: chrono::Utc::now(),
            },
        ];

        let results = vec![MediaSearchResult {
            title: query.to_string(),
            year: Some(2024),
            media_type: if category == "tv" {
                MediaType::TvShow
            } else {
                MediaType::Movie
            },
            imdb_id: Some("tt0133093".to_string()),
            poster_url: Some("https://m.media-amazon.com/images/M/MV5BNzQzOTk3OTAtNDQ0Zi00ZTVkLWI0MTEtMDllZjNkYzNjNTc4L2ltYWdlXkEyXkFqcGdeQXVyNjU0OTQ0OTY@._V1_SX300.jpg".to_string()),
            plot: Some(format!("An action-packed thriller about {query} featuring cutting-edge special effects and compelling storylines.")),
            genre: Some("Action, Sci-Fi".to_string()),
            rating: Some(8.7),
            torrents: mock_torrents,
        }];

        Ok(results)
    }
}
