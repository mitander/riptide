//! Media search and metadata integration
//!
//! Provides media discovery functionality using Magneto for torrent searches
//! and future IMDb integration for metadata and artwork.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Media search service providing torrent discovery and metadata.
#[derive(Debug)]
pub struct MediaSearchService {
    provider: Box<dyn TorrentSearchProvider>,
}

impl Clone for MediaSearchService {
    fn clone(&self) -> Self {
        // For now, always create a new Magneto provider
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

        // Sort by quality and seeders
        torrents.sort_by(|a, b| {
            b.seeders
                .cmp(&a.seeders)
                .then_with(|| b.quality_score().cmp(&a.quality_score()))
        });

        Ok(torrents)
    }
}

impl Default for MediaSearchService {
    fn default() -> Self {
        Self::new()
    }
}

/// Trait for torrent search providers.
#[async_trait]
pub trait TorrentSearchProvider: Send + Sync + std::fmt::Debug {
    /// Search for torrents by query and category.
    async fn search_torrents(
        &self,
        query: &str,
        category: &str,
    ) -> Result<Vec<MediaSearchResult>, MediaSearchError>;
}

/// Demo provider for development and testing.
///
/// Returns realistic demo data for UI development without external API calls.
/// Includes multiple quality options, realistic file sizes, and seeder counts.
#[derive(Debug)]
struct DemoProvider {
    #[allow(dead_code)] // For future demo data configuration
    demo_config: DemoConfig,
}

#[derive(Debug)]
struct DemoConfig {
    #[allow(dead_code)] // For future demo configuration options
    include_4k: bool,
    #[allow(dead_code)] // For future demo configuration options
    realistic_seeders: bool,
}

impl DemoProvider {
    fn new() -> Self {
        Self {
            demo_config: DemoConfig {
                include_4k: true,
                realistic_seeders: true,
            },
        }
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
            imdb_id: Some("tt1234567".to_string()),
            poster_url: None, // Will be populated by IMDb integration
            plot: Some(format!("Demo description for {query}")),
            genre: Some("Action".to_string()),
            rating: Some(8.5),
            torrents: mock_torrents,
        }];

        Ok(results)
    }
}

/// Mock provider for testing.
#[cfg(test)]
#[derive(Debug)]
struct MockProvider;

#[cfg(test)]
impl MockProvider {
    fn new() -> Self {
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

/// Media search result with metadata and available torrents.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MediaSearchResult {
    pub title: String,
    pub year: Option<u16>,
    pub media_type: MediaType,
    pub imdb_id: Option<String>,
    pub poster_url: Option<String>,
    pub plot: Option<String>,
    pub genre: Option<String>,
    pub rating: Option<f32>,
    pub torrents: Vec<TorrentResult>,
}

/// Individual torrent result for media.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TorrentResult {
    pub name: String,
    pub magnet_link: String,
    pub size: u64,
    pub seeders: u32,
    pub leechers: u32,
    pub quality: VideoQuality,
    pub source: String,
    pub added_date: chrono::DateTime<chrono::Utc>,
}

impl TorrentResult {
    /// Calculate quality score for sorting.
    fn quality_score(&self) -> u32 {
        match self.quality {
            VideoQuality::Unknown => 0,
            VideoQuality::CamRip => 1,
            VideoQuality::TeleSync => 2,
            VideoQuality::WebRip => 3,
            VideoQuality::DVD => 4,
            VideoQuality::BluRay720p => 5,
            VideoQuality::BluRay1080p => 6,
            VideoQuality::BluRay4K => 7,
            VideoQuality::Remux => 8,
        }
    }

    /// Format file size as human readable string.
    pub fn format_size(&self) -> String {
        let bytes = self.size as f64;
        if bytes >= 1_073_741_824.0 {
            format!("{:.1} GB", bytes / 1_073_741_824.0)
        } else if bytes >= 1_048_576.0 {
            format!("{:.1} MB", bytes / 1_048_576.0)
        } else {
            format!("{:.0} KB", bytes / 1024.0)
        }
    }
}

/// Media type classification.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum MediaType {
    Movie,
    TvShow,
    Music,
    Other,
}

/// Video quality levels for sorting and filtering.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum VideoQuality {
    Unknown,
    CamRip,
    TeleSync,
    WebRip,
    DVD,
    BluRay720p,
    BluRay1080p,
    BluRay4K,
    Remux,
}

/// Media search service errors.
#[derive(Debug, Error)]
pub enum MediaSearchError {
    #[error("Search failed for query '{query}': {reason}")]
    SearchFailed { query: String, reason: String },

    #[error("Network error: {reason}")]
    NetworkError { reason: String },

    #[error("Parsing error: {reason}")]
    ParseError { reason: String },
}

// Note: Magneto API structs removed as we're using demo data
// Will be re-added when we implement a working torrent search API

/// Extract clean media title from torrent name.
#[allow(dead_code)] // Will be used when implementing real API
fn extract_media_title(name: &str) -> String {
    let mut title = name.to_string();

    // Remove brackets and parentheses content first
    if let Ok(re) = regex::Regex::new(r"[\[\(].*?[\]\)]") {
        title = re.replace_all(&title, "").to_string();
    }

    // Remove year pattern (4 digits)
    if let Ok(re) = regex::Regex::new(r"\b(19|20)\d{2}\b") {
        title = re.replace(&title, "").to_string();
    }

    // Remove common quality and format patterns (case insensitive)
    let patterns = [
        ".2160p", ".1080p", ".720p", ".480p", ".BluRay", ".BRRip", ".WebRip", ".DVDRip", ".CAMRip",
        ".x264", ".x265", ".h264", ".h265", ".AAC", ".AC3", ".DTS", "-YIFY", "-FGT", "-RARBG",
        "-GROUP", " BluRay", " BRRip", " WebRip", " DVDRip", " CAMRip", // Space variants
        "BluRay", "BRRip", "WebRip", "DVDRip", "CAMRip", // No prefix variants
    ];

    let title_lower = title.to_lowercase();
    for pattern in &patterns {
        let pattern_lower = pattern.to_lowercase();
        if let Some(pos) = title_lower.find(&pattern_lower) {
            title.truncate(pos);
            break; // Stop at first match to avoid over-truncation
        }
    }

    // Clean up dots, underscores, and extra spaces
    title
        .replace(['.', '_'], " ")
        .split_whitespace()
        .collect::<Vec<&str>>()
        .join(" ")
}

/// Extract year from torrent name.
#[allow(dead_code)] // Will be used when implementing real API
fn extract_year(name: &str) -> Option<u16> {
    if let Ok(re) = regex::Regex::new(r"\b(19|20)\d{2}\b") {
        if let Some(capture) = re.find(name) {
            if let Ok(year) = capture.as_str().parse::<u16>() {
                if (1900..=2030).contains(&year) {
                    return Some(year);
                }
            }
        }
    }
    None
}

/// Extract video quality from torrent name.
#[allow(dead_code)] // Will be used when implementing real API
fn extract_quality(name: &str) -> VideoQuality {
    let name_lower = name.to_lowercase();

    if name_lower.contains("remux") {
        VideoQuality::Remux
    } else if name_lower.contains("2160p") || name_lower.contains("4k") {
        VideoQuality::BluRay4K
    } else if name_lower.contains("1080p") {
        VideoQuality::BluRay1080p
    } else if name_lower.contains("720p") {
        VideoQuality::BluRay720p
    } else if name_lower.contains("dvd") {
        VideoQuality::DVD
    } else if name_lower.contains("webrip") || name_lower.contains("web-dl") {
        VideoQuality::WebRip
    } else if name_lower.contains("ts") || name_lower.contains("telesync") {
        VideoQuality::TeleSync
    } else if name_lower.contains("cam") || name_lower.contains("camrip") {
        VideoQuality::CamRip
    } else {
        VideoQuality::Unknown
    }
}

/// Detect media type from torrent name.
#[allow(dead_code)] // Will be used when implementing real API
fn detect_media_type(name: &str) -> MediaType {
    let name_lower = name.to_lowercase();

    if name_lower.contains("s01e")
        || name_lower.contains("season")
        || name_lower.contains("episode")
    {
        MediaType::TvShow
    } else if name_lower.contains(".mp3")
        || name_lower.contains(".flac")
        || name_lower.contains("album")
    {
        MediaType::Music
    } else {
        MediaType::Movie // Default assumption
    }
}

/// Check if category matches content type.
#[allow(dead_code)] // Will be used when implementing real API
fn category_matches(category: &str, _title: &str) -> bool {
    // For now, accept all results regardless of category
    // In future, could add more sophisticated filtering
    category == "movie" || category == "tv" || category == "all"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_media_title() {
        assert_eq!(
            extract_media_title("The.Matrix.1999.1080p.BluRay.x264-GROUP"),
            "The Matrix"
        );
        assert_eq!(
            extract_media_title("Inception (2010) [1080p] BluRay"),
            "Inception"
        );
        assert_eq!(
            extract_media_title("Breaking.Bad.S01E01.720p.WEB-DL"),
            "Breaking Bad S01E01"
        );
        // Test that it handles quality markers not in brackets
        assert_eq!(
            extract_media_title("Movie.Title.BluRay.x264"),
            "Movie Title"
        );
    }

    #[test]
    fn test_extract_year() {
        assert_eq!(extract_year("The.Matrix.1999.1080p.BluRay"), Some(1999));
        assert_eq!(extract_year("Inception.2010.720p.WebRip"), Some(2010));
        assert_eq!(extract_year("NoYear.Movie.720p"), None);
    }

    #[test]
    fn test_extract_quality() {
        assert_eq!(
            extract_quality("Movie.2020.1080p.BluRay"),
            VideoQuality::BluRay1080p
        );
        assert_eq!(
            extract_quality("Movie.2020.720p.WebRip"),
            VideoQuality::BluRay720p
        );
        assert_eq!(extract_quality("Movie.2020.4K.REMUX"), VideoQuality::Remux);
        assert_eq!(extract_quality("Movie.CAMRip"), VideoQuality::CamRip);
    }

    #[test]
    fn test_detect_media_type() {
        assert_eq!(
            detect_media_type("Movie.2020.1080p.BluRay"),
            MediaType::Movie
        );
        assert_eq!(
            detect_media_type("Breaking.Bad.S01E01.720p"),
            MediaType::TvShow
        );
        assert_eq!(
            detect_media_type("Album.Artist.2020.FLAC"),
            MediaType::Music
        );
    }

    #[test]
    fn test_torrent_quality_score() {
        let torrent_4k = TorrentResult {
            name: "Movie.4K".to_string(),
            magnet_link: "magnet:".to_string(),
            size: 0,
            seeders: 0,
            leechers: 0,
            quality: VideoQuality::BluRay4K,
            source: "test".to_string(),
            added_date: chrono::Utc::now(),
        };

        let torrent_720p = TorrentResult {
            name: "Movie.720p".to_string(),
            magnet_link: "magnet:".to_string(),
            size: 0,
            seeders: 0,
            leechers: 0,
            quality: VideoQuality::BluRay720p,
            source: "test".to_string(),
            added_date: chrono::Utc::now(),
        };

        assert!(torrent_4k.quality_score() > torrent_720p.quality_score());
    }

    #[test]
    fn test_format_size() {
        let torrent = TorrentResult {
            name: "test".to_string(),
            magnet_link: "magnet:".to_string(),
            size: 1_500_000_000,
            seeders: 0,
            leechers: 0,
            quality: VideoQuality::BluRay1080p,
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
}
