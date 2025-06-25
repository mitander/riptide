//! Media domain models representing movies, TV shows, and related metadata.

use std::fmt;

use chrono::Datelike;
use serde::{Deserialize, Serialize};

/// Unique identifier for media content.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct MediaId(uuid::Uuid);

impl MediaId {
    /// Creates a new random media ID.
    pub fn new() -> Self {
        Self(uuid::Uuid::new_v4())
    }

    /// Creates a media ID from a UUID.
    pub fn from_uuid(uuid: uuid::Uuid) -> Self {
        Self(uuid)
    }

    /// Returns the underlying UUID.
    pub fn as_uuid(&self) -> uuid::Uuid {
        self.0
    }
}

impl Default for MediaId {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Display for MediaId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Core media entity representing a movie, TV show, or other content.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Media {
    id: MediaId,
    title: String,
    media_type: MediaType,
    release_year: Option<u16>,
    imdb_id: Option<String>,
    plot_summary: Option<String>,
    genre: Option<String>,
    rating: Option<f32>,
    poster_url: Option<String>,
}

impl Media {
    /// Creates new media with required fields.
    pub fn new(title: String, media_type: MediaType) -> Self {
        Self {
            id: MediaId::new(),
            title,
            media_type,
            release_year: None,
            imdb_id: None,
            plot_summary: None,
            genre: None,
            rating: None,
            poster_url: None,
        }
    }

    /// Creates media with all metadata.
    #[allow(clippy::too_many_arguments)]
    pub fn with_metadata(
        title: String,
        media_type: MediaType,
        release_year: Option<u16>,
        imdb_id: Option<String>,
        plot_summary: Option<String>,
        genre: Option<String>,
        rating: Option<f32>,
        poster_url: Option<String>,
    ) -> Result<Self, MediaValidationError> {
        let media = Self {
            id: MediaId::new(),
            title,
            media_type,
            release_year,
            imdb_id,
            plot_summary,
            genre,
            rating,
            poster_url,
        };

        media.validate()?;
        Ok(media)
    }

    /// Validates media data according to business rules.
    pub fn validate(&self) -> Result<(), MediaValidationError> {
        if self.title.trim().is_empty() {
            return Err(MediaValidationError::EmptyTitle);
        }

        if self.title.len() > 200 {
            return Err(MediaValidationError::TitleTooLong {
                length: self.title.len(),
            });
        }

        if let Some(year) = self.release_year {
            if !Self::is_valid_release_year(year) {
                return Err(MediaValidationError::InvalidReleaseYear { year });
            }
        }

        if let Some(rating) = self.rating {
            if !(0.0..=10.0).contains(&rating) {
                return Err(MediaValidationError::InvalidRating { rating });
            }
        }

        Ok(())
    }

    /// Checks if a release year is valid.
    pub fn is_valid_release_year(year: u16) -> bool {
        // First motion picture was 1888, current year is reasonable upper bound
        let current_year = chrono::Utc::now().year() as u16;
        year >= 1888 && year <= current_year + 2 // Allow for upcoming releases
    }

    /// Checks if this media matches a search query.
    pub fn matches_search(&self, query: &str) -> bool {
        let query_lower = query.to_lowercase();

        // Check title match
        if self.title.to_lowercase().contains(&query_lower) {
            return true;
        }

        // Check genre match
        if let Some(genre) = &self.genre {
            if genre.to_lowercase().contains(&query_lower) {
                return true;
            }
        }

        // Check year match (exact)
        if let Ok(year) = query.parse::<u16>() {
            if self.release_year == Some(year) {
                return true;
            }
        }

        false
    }

    /// Calculates search relevance score for ranking.
    pub fn search_relevance(&self, query: &str) -> f32 {
        let query_lower = query.to_lowercase();
        let title_lower = self.title.to_lowercase();

        // Exact title match
        if title_lower == query_lower {
            return 1.0;
        }

        // Title starts with query
        if title_lower.starts_with(&query_lower) {
            return 0.9;
        }

        // Title contains query
        if title_lower.contains(&query_lower) {
            return 0.7;
        }

        // Genre match
        if let Some(genre) = &self.genre {
            if genre.to_lowercase().contains(&query_lower) {
                return 0.5;
            }
        }

        0.0
    }

    // Getters
    pub fn id(&self) -> MediaId {
        self.id
    }
    pub fn title(&self) -> &str {
        &self.title
    }
    pub fn media_type(&self) -> MediaType {
        self.media_type
    }
    pub fn release_year(&self) -> Option<u16> {
        self.release_year
    }
    pub fn imdb_id(&self) -> Option<&str> {
        self.imdb_id.as_deref()
    }
    pub fn plot_summary(&self) -> Option<&str> {
        self.plot_summary.as_deref()
    }
    pub fn genre(&self) -> Option<&str> {
        self.genre.as_deref()
    }
    pub fn rating(&self) -> Option<f32> {
        self.rating
    }
    pub fn poster_url(&self) -> Option<&str> {
        self.poster_url.as_deref()
    }

    // Setters for metadata enrichment
    pub fn set_imdb_id(&mut self, imdb_id: String) {
        self.imdb_id = Some(imdb_id);
    }

    pub fn set_plot_summary(&mut self, plot: String) {
        self.plot_summary = Some(plot);
    }

    pub fn set_genre(&mut self, genre: String) {
        self.genre = Some(genre);
    }

    pub fn set_rating(&mut self, rating: f32) -> Result<(), MediaValidationError> {
        if !(0.0..=10.0).contains(&rating) {
            return Err(MediaValidationError::InvalidRating { rating });
        }
        self.rating = Some(rating);
        Ok(())
    }

    pub fn set_poster_url(&mut self, url: String) {
        self.poster_url = Some(url);
    }
}

/// Type of media content.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MediaType {
    Movie,
    TvShow,
    Documentary,
    ShortFilm,
}

impl fmt::Display for MediaType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            MediaType::Movie => write!(f, "Movie"),
            MediaType::TvShow => write!(f, "TV Show"),
            MediaType::Documentary => write!(f, "Documentary"),
            MediaType::ShortFilm => write!(f, "Short Film"),
        }
    }
}

/// Video quality levels for content sources.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum VideoQuality {
    Unknown = 0,
    CamRip = 1,
    TeleSync = 2,
    WebRip = 3,
    DVD = 4,
    BluRay720p = 5,
    BluRay1080p = 6,
    BluRay4K = 7,
    Remux = 8,
}

impl VideoQuality {
    /// Returns quality score for ranking (higher is better).
    pub fn score(&self) -> u8 {
        *self as u8
    }

    /// Checks if this quality is considered high definition.
    pub fn is_hd(&self) -> bool {
        matches!(
            self,
            VideoQuality::BluRay720p
                | VideoQuality::BluRay1080p
                | VideoQuality::BluRay4K
                | VideoQuality::Remux
        )
    }

    /// Checks if this quality is suitable for streaming.
    pub fn is_streamable(&self) -> bool {
        !matches!(self, VideoQuality::Unknown | VideoQuality::CamRip)
    }
}

impl fmt::Display for VideoQuality {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            VideoQuality::Unknown => write!(f, "Unknown"),
            VideoQuality::CamRip => write!(f, "CAM"),
            VideoQuality::TeleSync => write!(f, "TS"),
            VideoQuality::WebRip => write!(f, "WebRip"),
            VideoQuality::DVD => write!(f, "DVD"),
            VideoQuality::BluRay720p => write!(f, "720p"),
            VideoQuality::BluRay1080p => write!(f, "1080p"),
            VideoQuality::BluRay4K => write!(f, "4K"),
            VideoQuality::Remux => write!(f, "Remux"),
        }
    }
}

/// Errors that can occur during media validation.
#[derive(Debug, thiserror::Error)]
pub enum MediaValidationError {
    #[error("Media title cannot be empty")]
    EmptyTitle,

    #[error("Media title too long: {length} characters (max 200)")]
    TitleTooLong { length: usize },

    #[error("Invalid release year: {year} (must be between 1888 and current year + 2)")]
    InvalidReleaseYear { year: u16 },

    #[error("Invalid rating: {rating} (must be between 0.0 and 10.0)")]
    InvalidRating { rating: f32 },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_media_creation() {
        let media = Media::new("The Matrix".to_string(), MediaType::Movie);
        assert_eq!(media.title(), "The Matrix");
        assert_eq!(media.media_type(), MediaType::Movie);
    }

    #[test]
    fn test_media_validation() {
        // Valid media
        let media = Media::new("Valid Title".to_string(), MediaType::Movie);
        assert!(media.validate().is_ok());

        // Empty title
        let media = Media::new("".to_string(), MediaType::Movie);
        assert!(matches!(
            media.validate(),
            Err(MediaValidationError::EmptyTitle)
        ));

        // Invalid year
        let media = Media::with_metadata(
            "Old Movie".to_string(),
            MediaType::Movie,
            Some(1800), // Too old
            None,
            None,
            None,
            None,
            None,
        );
        assert!(media.is_err());
    }

    #[test]
    fn test_search_matching() {
        let media = Media::with_metadata(
            "The Dark Knight".to_string(),
            MediaType::Movie,
            Some(2008),
            None,
            None,
            Some("Action".to_string()),
            None,
            None,
        )
        .unwrap();

        assert!(media.matches_search("dark"));
        assert!(media.matches_search("knight"));
        assert!(media.matches_search("action"));
        assert!(media.matches_search("2008"));
        assert!(!media.matches_search("comedy"));
    }

    #[test]
    fn test_search_relevance() {
        let media = Media::new("The Matrix".to_string(), MediaType::Movie);

        assert_eq!(media.search_relevance("the matrix"), 1.0);
        assert_eq!(media.search_relevance("the"), 0.9);
        assert_eq!(media.search_relevance("matrix"), 0.7);
    }

    #[test]
    fn test_video_quality_ordering() {
        assert!(VideoQuality::BluRay4K > VideoQuality::BluRay1080p);
        assert!(VideoQuality::BluRay1080p > VideoQuality::DVD);
        assert!(VideoQuality::Remux.score() > VideoQuality::BluRay4K.score());
    }

    #[test]
    fn test_video_quality_classification() {
        assert!(VideoQuality::BluRay1080p.is_hd());
        assert!(!VideoQuality::DVD.is_hd());
        assert!(VideoQuality::BluRay720p.is_streamable());
        assert!(!VideoQuality::CamRip.is_streamable());
    }

    #[test]
    fn test_release_year_validation() {
        assert!(Media::is_valid_release_year(1888)); // First motion picture
        assert!(Media::is_valid_release_year(2024)); // Current
        assert!(!Media::is_valid_release_year(1800)); // Too old
        assert!(!Media::is_valid_release_year(3000)); // Too far future
    }
}
