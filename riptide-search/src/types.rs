//! Data types for media search functionality.

use serde::{Deserialize, Serialize};

/// Search result containing media information and available torrents.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MediaSearchResult {
    /// Title of the media item
    pub title: String,
    /// Release year of the media
    pub year: Option<u16>,
    /// Type classification (movie, TV show, etc.)
    pub media_type: MediaType,
    /// IMDb identifier for the media
    pub imdb_id: Option<String>,
    /// URL to poster image
    pub poster_url: Option<String>,
    /// Plot summary or description
    pub plot: Option<String>,
    /// Genre classification
    pub genre: Option<String>,
    /// Rating score (typically 0-10)
    pub rating: Option<f32>,
    /// Available torrent downloads for this media
    pub torrents: Vec<TorrentResult>,
}

/// Individual torrent result for a media item.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TorrentResult {
    /// Human-readable name of the torrent
    pub name: String,
    /// Magnet link for downloading the torrent
    pub magnet_link: String,
    /// Total size of the torrent in bytes
    pub size: u64,
    /// Number of active seeders
    pub seeders: u32,
    /// Number of active leechers
    pub leechers: u32,
    /// Video quality classification
    pub quality: VideoQuality,
    /// Source tracker or indexer name
    pub source: String,
    /// When this torrent was added to the tracker
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
    /// Feature film or movie
    Movie,
    /// Television series or episode
    TvShow,
    /// Audio or music content
    Music,
    /// Other media types not classified above
    Other,
}

/// Video quality enumeration for torrent results.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum VideoQuality {
    /// Quality classification unknown or unspecified
    Unknown,
    /// Camera recording in theater (lowest quality)
    CamRip,
    /// Telecine or telesync recording
    TeleSync,
    /// Web rip from streaming service
    WebRip,
    /// Web download 720p resolution
    WebDl720p,
    /// Web download 1080p resolution
    WebDl1080p,
    /// HDTV broadcast 720p resolution
    Hdtv720p,
    /// HDTV broadcast 1080p resolution
    Hdtv1080p,
    /// DVD source quality
    Dvd,
    /// Blu-ray 720p resolution
    BluRay720p,
    /// Blu-ray 1080p resolution
    BluRay1080p,
    /// Blu-ray 4K/UHD resolution
    BluRay4K,
    /// Lossless remux from disc (highest quality)
    Remux,
}
