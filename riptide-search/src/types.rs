//! Data types for media search functionality.

use serde::{Deserialize, Serialize};

/// Search result containing media information and available torrents.
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

/// Individual torrent result for a media item.
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
    /// Format file size in human-readable format.
    pub fn format_size(&self) -> String {
        const GB: u64 = 1024 * 1024 * 1024;
        const MB: u64 = 1024 * 1024;

        if self.size >= GB {
            format!("{:.1} GB", self.size as f64 / GB as f64)
        } else if self.size >= MB {
            format!("{:.1} MB", self.size as f64 / MB as f64)
        } else {
            format!("{:.1} KB", self.size as f64 / 1024.0)
        }
    }

    /// Calculate quality score for sorting.
    pub fn quality_score(&self) -> u32 {
        match self.quality {
            VideoQuality::Unknown => 0,
            VideoQuality::CamRip => 1,
            VideoQuality::TeleSync => 2,
            VideoQuality::WebRip => 3,
            VideoQuality::Hdtv720p => 4,
            VideoQuality::WebDl720p => 5,
            VideoQuality::Dvd => 6,
            VideoQuality::BluRay720p => 7,
            VideoQuality::Hdtv1080p => 8,
            VideoQuality::WebDl1080p => 9,
            VideoQuality::BluRay1080p => 10,
            VideoQuality::BluRay4K => 11,
            VideoQuality::Remux => 12,
        }
    }

    /// Calculate download priority score based on seeders and quality.
    pub fn priority_score(&self) -> u32 {
        // Combine quality score with seeder count (capped at 50 to prevent overwhelming quality)
        self.quality_score() * 10 + std::cmp::min(self.seeders, 50)
    }
}

/// Media type classification for search results.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum MediaType {
    Movie,
    TvShow,
    Music,
    Other,
}

/// Video quality enumeration for torrent results.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum VideoQuality {
    Unknown,
    CamRip,
    TeleSync,
    WebRip,
    WebDl720p,
    WebDl1080p,
    Hdtv720p,
    Hdtv1080p,
    Dvd,
    BluRay720p,
    BluRay1080p,
    BluRay4K,
    Remux,
}
