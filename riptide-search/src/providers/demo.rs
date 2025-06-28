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
                    "magnet:?xt=urn:btih:1234567890abcdef1234567890abcdef12345678&dn={}",
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
                    "magnet:?xt=urn:btih:abcdef1234567890abcdef1234567890abcdef12&dn={}",
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
                    "magnet:?xt=urn:btih:fedcba0987654321fedcba0987654321fedcba09&dn={}",
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

        // Create realistic demo data based on common movie titles
        let (poster_url, plot, genre, rating, year, imdb_id) = match query.to_lowercase().as_str() {
            q if q.contains("interstellar") => (
                Some("https://m.media-amazon.com/images/M/MV5BZjdkOTU3MDktN2IxOS00OGEyLWFmMjktY2FiMmZkNWIyODZiXkEyXkFqcGdeQXVyMTMxODk2OTU@._V1_SX300.jpg".to_string()),
                Some("A team of explorers travel through a wormhole in space in an attempt to ensure humanity's survival.".to_string()),
                Some("Adventure, Drama, Sci-Fi".to_string()),
                Some(8.6),
                Some(2014),
                Some("tt0816692".to_string())
            ),
            q if q.contains("matrix") => (
                Some("https://m.media-amazon.com/images/M/MV5BNzQzOTk3OTAtNDQ0Zi00ZTVkLWI0MTEtMDllZjNkYzNjNTc4L2ltYWdlXkEyXkFqcGdeQXVyNjU0OTQ0OTY@._V1_SX300.jpg".to_string()),
                Some("A computer hacker learns from mysterious rebels about the true nature of his reality and his role in the war against its controllers.".to_string()),
                Some("Action, Sci-Fi".to_string()),
                Some(8.7),
                Some(1999),
                Some("tt0133093".to_string())
            ),
            q if q.contains("inception") => (
                Some("https://m.media-amazon.com/images/M/MV5BMjAxMzY3NjcxNF5BMl5BanBnXkFtZTcwNTI5OTM0Mw@@._V1_SX300.jpg".to_string()),
                Some("A thief who steals corporate secrets through dream-sharing technology is given the inverse task of planting an idea into the mind of a C.E.O.".to_string()),
                Some("Action, Adventure, Sci-Fi".to_string()),
                Some(8.8),
                Some(2010),
                Some("tt1375666".to_string())
            ),
            q if q.contains("dune") => (
                Some("https://m.media-amazon.com/images/M/MV5BN2FjNmEyNWMtYzM0ZS00NjIyLTg5YzYtYThlMGVjNzE1OGViXkEyXkFqcGdeQXVyMTkxNjUyNQ@@._V1_SX300.jpg".to_string()),
                Some("Paul Atreides leads nomadic tribes in a revolt against the galactic emperor and his father's evil nemesis when they assassinate his father and free their desert world from the emperor's rule.".to_string()),
                Some("Action, Adventure, Drama".to_string()),
                Some(8.0),
                Some(2021),
                Some("tt1160419".to_string())
            ),
            q if q.contains("blade runner") => (
                Some("https://m.media-amazon.com/images/M/MV5BNzQzOTk3OTAtNDQ0Zi00ZTVkLWI0MTEtMDllZjNkYzNjNTc4L2ltYWdlXkEyXkFqcGdeQXVyNjU0OTQ0OTY@._V1_SX300.jpg".to_string()),
                Some("A blade runner must pursue and terminate four replicants who stole a ship in space, and have returned to the earth seeking their creator.".to_string()),
                Some("Action, Drama, Sci-Fi".to_string()),
                Some(8.1),
                Some(1982),
                Some("tt0083658".to_string())
            ),
            _ => (
                None, // No poster for unknown titles - will trigger IMDb lookup
                Some(format!("A thrilling story about {query} with compelling characters and stunning visuals.")),
                Some("Action, Drama".to_string()),
                Some(7.5),
                Some(2023),
                Some("tt1234567".to_string())
            )
        };

        let results = vec![MediaSearchResult {
            title: query.to_string(),
            year,
            media_type: if category == "tv" {
                MediaType::TvShow
            } else {
                MediaType::Movie
            },
            imdb_id,
            poster_url,
            plot,
            genre,
            rating,
            torrents: mock_torrents,
        }];

        Ok(results)
    }
}
