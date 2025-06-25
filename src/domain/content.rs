//! Content source domain models representing torrents and direct links.

use std::fmt;

use serde::{Deserialize, Serialize};

use crate::domain::media::{MediaId, VideoQuality};

/// Unique identifier for content sources.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ContentId(uuid::Uuid);

impl ContentId {
    /// Creates a new random content ID.
    pub fn new() -> Self {
        Self(uuid::Uuid::new_v4())
    }

    /// Creates a content ID from a UUID.
    pub fn from_uuid(uuid: uuid::Uuid) -> Self {
        Self(uuid)
    }

    /// Returns the underlying UUID.
    pub fn as_uuid(&self) -> uuid::Uuid {
        self.0
    }
}

impl Default for ContentId {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Display for ContentId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// A source of media content (torrent, direct link, etc.).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContentSource {
    id: ContentId,
    media_id: MediaId,
    source_type: SourceType,
    name: String,
    size_bytes: u64,
    quality: VideoQuality,
    health: SourceHealth,
    source_url: String,
    added_date: chrono::DateTime<chrono::Utc>,
}

impl ContentSource {
    /// Creates a new content source.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        media_id: MediaId,
        source_type: SourceType,
        name: String,
        size_bytes: u64,
        quality: VideoQuality,
        health: SourceHealth,
        source_url: String,
    ) -> Result<Self, ContentValidationError> {
        let source = Self {
            id: ContentId::new(),
            media_id,
            source_type,
            name,
            size_bytes,
            quality,
            health,
            source_url,
            added_date: chrono::Utc::now(),
        };

        source.validate()?;
        Ok(source)
    }

    /// Validates content source data.
    pub fn validate(&self) -> Result<(), ContentValidationError> {
        if self.name.trim().is_empty() {
            return Err(ContentValidationError::EmptyName);
        }

        if self.size_bytes == 0 {
            return Err(ContentValidationError::ZeroSize);
        }

        if self.source_url.trim().is_empty() {
            return Err(ContentValidationError::EmptySourceUrl);
        }

        // Validate URL format based on source type
        match self.source_type {
            SourceType::Torrent => {
                if !self.source_url.starts_with("magnet:") && !self.source_url.ends_with(".torrent")
                {
                    return Err(ContentValidationError::InvalidTorrentUrl);
                }
            }
            SourceType::DirectLink => {
                if !self.source_url.starts_with("http://")
                    && !self.source_url.starts_with("https://")
                {
                    return Err(ContentValidationError::InvalidDirectUrl);
                }
            }
        }

        Ok(())
    }

    /// Calculates overall quality score for ranking.
    pub fn quality_score(&self) -> f32 {
        let quality_weight = self.quality.score() as f32 * 10.0;
        let health_weight = self.health.score() * 5.0;
        let size_weight = self.size_preference_score() * 2.0;

        quality_weight + health_weight + size_weight
    }

    /// Calculates size preference score (prefers reasonable sizes).
    fn size_preference_score(&self) -> f32 {
        let gb = self.size_bytes as f64 / (1024.0 * 1024.0 * 1024.0);

        match self.quality {
            VideoQuality::BluRay4K | VideoQuality::Remux => {
                // 4K content: prefer 15-50 GB range
                if (15.0..=50.0).contains(&gb) {
                    10.0
                } else if gb > 50.0 {
                    5.0 // Too large
                } else {
                    2.0 // Too small for 4K
                }
            }
            VideoQuality::BluRay1080p => {
                // 1080p content: prefer 2-15 GB range
                if (2.0..=15.0).contains(&gb) {
                    10.0
                } else if gb > 15.0 {
                    7.0 // Large but acceptable
                } else {
                    3.0 // Small
                }
            }
            VideoQuality::BluRay720p => {
                // 720p content: prefer 1-8 GB range
                if (1.0..=8.0).contains(&gb) { 10.0 } else { 5.0 }
            }
            _ => {
                // Other qualities: size matters less
                if gb > 0.5 { 5.0 } else { 1.0 }
            }
        }
    }

    /// Checks if this source is suitable for streaming.
    pub fn is_streamable(&self) -> bool {
        self.quality.is_streamable() && self.health.is_healthy()
    }

    /// Checks if this source is considered healthy.
    pub fn is_healthy(&self) -> bool {
        self.health.is_healthy()
    }

    /// Formats file size as human-readable string.
    pub fn format_size(&self) -> String {
        let bytes = self.size_bytes as f64;
        if bytes >= 1_073_741_824.0 {
            format!("{:.1} GB", bytes / 1_073_741_824.0)
        } else if bytes >= 1_048_576.0 {
            format!("{:.1} MB", bytes / 1_048_576.0)
        } else {
            format!("{:.0} KB", bytes / 1024.0)
        }
    }

    /// Updates source health based on new metrics.
    pub fn update_health(&mut self, seeders: u32, leechers: u32) {
        self.health = SourceHealth::from_swarm_stats(seeders, leechers);
    }

    // Getters
    pub fn id(&self) -> ContentId {
        self.id
    }
    pub fn media_id(&self) -> MediaId {
        self.media_id
    }
    pub fn source_type(&self) -> SourceType {
        self.source_type
    }
    pub fn name(&self) -> &str {
        &self.name
    }
    pub fn size_bytes(&self) -> u64 {
        self.size_bytes
    }
    pub fn quality(&self) -> VideoQuality {
        self.quality
    }
    pub fn health(&self) -> &SourceHealth {
        &self.health
    }
    pub fn source_url(&self) -> &str {
        &self.source_url
    }
    pub fn added_date(&self) -> chrono::DateTime<chrono::Utc> {
        self.added_date
    }
}

/// Type of content source.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SourceType {
    /// BitTorrent magnet link or .torrent file
    Torrent,
    /// Direct HTTP/HTTPS download link
    DirectLink,
}

impl fmt::Display for SourceType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SourceType::Torrent => write!(f, "Torrent"),
            SourceType::DirectLink => write!(f, "Direct Link"),
        }
    }
}

/// Health status of a content source.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceHealth {
    seeders: u32,
    leechers: u32,
    health_score: f32,
}

impl SourceHealth {
    /// Creates source health from seeder/leecher counts.
    pub fn from_swarm_stats(seeders: u32, leechers: u32) -> Self {
        let health_score = if seeders == 0 {
            0.0
        } else if leechers == 0 {
            1.0 // Perfect health - only seeders
        } else {
            let ratio = seeders as f32 / (seeders + leechers) as f32;
            // Apply logarithmic scaling to favor higher seeder counts
            let seeder_bonus = (seeders as f32).ln().max(0.0) / 10.0;
            (ratio + seeder_bonus).min(1.0)
        };

        Self {
            seeders,
            leechers,
            health_score,
        }
    }

    /// Creates health for direct links (always healthy if accessible).
    pub fn direct_link_health() -> Self {
        Self {
            seeders: 1, // Conceptual "seeder"
            leechers: 0,
            health_score: 1.0,
        }
    }

    /// Returns health score (0.0 to 1.0).
    pub fn score(&self) -> f32 {
        self.health_score
    }

    /// Checks if source is considered healthy for downloading.
    pub fn is_healthy(&self) -> bool {
        self.seeders > 0 && self.health_score > 0.1
    }

    /// Returns descriptive health status.
    pub fn status_description(&self) -> &'static str {
        if self.health_score >= 0.8 {
            "Excellent"
        } else if self.health_score >= 0.6 {
            "Good"
        } else if self.health_score >= 0.3 {
            "Fair"
        } else if self.health_score > 0.0 {
            "Poor"
        } else {
            "Dead"
        }
    }

    // Getters
    pub fn seeders(&self) -> u32 {
        self.seeders
    }
    pub fn leechers(&self) -> u32 {
        self.leechers
    }
}

/// Errors that can occur during content source validation.
#[derive(Debug, thiserror::Error)]
pub enum ContentValidationError {
    #[error("Content source name cannot be empty")]
    EmptyName,

    #[error("Content source size cannot be zero")]
    ZeroSize,

    #[error("Source URL cannot be empty")]
    EmptySourceUrl,

    #[error("Invalid torrent URL (must be magnet: link or .torrent file)")]
    InvalidTorrentUrl,

    #[error("Invalid direct URL (must start with http:// or https://)")]
    InvalidDirectUrl,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::media::{Media, MediaType};

    #[test]
    fn test_content_source_creation() {
        let media = Media::new("Test Movie".to_string(), MediaType::Movie);
        let health = SourceHealth::from_swarm_stats(50, 10);

        let source = ContentSource::new(
            media.id(),
            SourceType::Torrent,
            "Test.Movie.2024.1080p.BluRay.x264".to_string(),
            1_500_000_000,
            VideoQuality::BluRay1080p,
            health,
            "magnet:?xt=urn:btih:test".to_string(),
        )
        .unwrap();

        assert_eq!(source.media_id(), media.id());
        assert_eq!(source.quality(), VideoQuality::BluRay1080p);
        assert!(source.is_healthy());
    }

    #[test]
    fn test_source_health_calculation() {
        // Excellent health
        let health = SourceHealth::from_swarm_stats(100, 10);
        assert!(health.score() > 0.8);
        assert_eq!(health.status_description(), "Excellent");

        // Dead source
        let health = SourceHealth::from_swarm_stats(0, 50);
        assert_eq!(health.score(), 0.0);
        assert_eq!(health.status_description(), "Dead");
        assert!(!health.is_healthy());
    }

    #[test]
    fn test_quality_score_calculation() {
        let media = Media::new("Test".to_string(), MediaType::Movie);
        let health = SourceHealth::from_swarm_stats(50, 10);

        let source_4k = ContentSource::new(
            media.id(),
            SourceType::Torrent,
            "Test.4K".to_string(),
            25_000_000_000, // 25 GB - good size for 4K
            VideoQuality::BluRay4K,
            health.clone(),
            "magnet:?xt=urn:btih:test4k".to_string(),
        )
        .unwrap();

        let source_1080p = ContentSource::new(
            media.id(),
            SourceType::Torrent,
            "Test.1080p".to_string(),
            5_000_000_000, // 5 GB - good size for 1080p
            VideoQuality::BluRay1080p,
            health,
            "magnet:?xt=urn:btih:test1080p".to_string(),
        )
        .unwrap();

        assert!(source_4k.quality_score() > source_1080p.quality_score());
    }

    #[test]
    fn test_size_formatting() {
        let media = Media::new("Test".to_string(), MediaType::Movie);
        let health = SourceHealth::from_swarm_stats(10, 5);

        let source = ContentSource::new(
            media.id(),
            SourceType::Torrent,
            "Test".to_string(),
            1_500_000_000, // 1.5 GB
            VideoQuality::BluRay1080p,
            health,
            "magnet:?xt=urn:btih:test".to_string(),
        )
        .unwrap();

        assert_eq!(source.format_size(), "1.4 GB");
    }

    #[test]
    fn test_url_validation() {
        let media = Media::new("Test".to_string(), MediaType::Movie);
        let health = SourceHealth::from_swarm_stats(10, 5);

        // Valid magnet URL
        let result = ContentSource::new(
            media.id(),
            SourceType::Torrent,
            "Test".to_string(),
            1000,
            VideoQuality::DVD,
            health.clone(),
            "magnet:?xt=urn:btih:abcd1234".to_string(),
        );
        assert!(result.is_ok());

        // Invalid torrent URL
        let result = ContentSource::new(
            media.id(),
            SourceType::Torrent,
            "Test".to_string(),
            1000,
            VideoQuality::DVD,
            health.clone(),
            "http://example.com/file.mp4".to_string(),
        );
        assert!(result.is_err());

        // Valid direct URL
        let result = ContentSource::new(
            media.id(),
            SourceType::DirectLink,
            "Test".to_string(),
            1000,
            VideoQuality::DVD,
            health,
            "https://example.com/file.mp4".to_string(),
        );
        assert!(result.is_ok());
    }

    #[test]
    fn test_streamable_classification() {
        let media = Media::new("Test".to_string(), MediaType::Movie);
        let good_health = SourceHealth::from_swarm_stats(50, 10);
        let bad_health = SourceHealth::from_swarm_stats(0, 10);

        let good_source = ContentSource::new(
            media.id(),
            SourceType::Torrent,
            "Test".to_string(),
            1000,
            VideoQuality::BluRay1080p,
            good_health,
            "magnet:?xt=urn:btih:test".to_string(),
        )
        .unwrap();

        let bad_source = ContentSource::new(
            media.id(),
            SourceType::Torrent,
            "Test".to_string(),
            1000,
            VideoQuality::CamRip, // Poor quality
            bad_health,
            "magnet:?xt=urn:btih:test".to_string(),
        )
        .unwrap();

        assert!(good_source.is_streamable());
        assert!(!bad_source.is_streamable());
    }
}
