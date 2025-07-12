//! Media file types and analysis for streaming simulation

use std::path::PathBuf;
use std::time::Duration;

/// Media file types found in movie folders.
#[derive(Debug, Clone, PartialEq)]
pub enum MediaFileType {
    /// Video content with encoding information
    Video {
        /// Video codec (H.264, H.265, etc.)
        codec: String,
        /// Video bitrate in bits per second
        bitrate: u64,
        /// Total video duration
        duration: Duration,
    },
    /// Subtitle track with language information
    Subtitle {
        /// Language code (en, es, fr, etc.)
        language: String,
        /// Subtitle format (SRT, ASS, VTT, etc.)
        format: String,
    },
    /// Metadata file containing movie information
    Metadata {
        /// Metadata format (NFO, JSON, etc.)
        format: String,
    },
    /// Audio track information
    Audio {
        /// Audio codec (AAC, MP3, etc.)
        codec: String,
        /// Audio bitrate in bits per second
        bitrate: u64,
    },
    /// Image files for artwork and thumbnails
    Image {
        /// Purpose of image (cover, thumbnail, poster, etc.)
        purpose: String,
    },
}

/// Individual media file within a movie folder.
#[derive(Debug, Clone)]
pub struct MediaFile {
    /// File system path to the media file
    pub path: PathBuf,
    /// File size in bytes
    pub size: u64,
    /// Type and characteristics of the media content
    pub file_type: MediaFileType,
    /// Streaming priority for download ordering
    pub priority: StreamingPriority,
}

/// Streaming priority levels for different file types.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum StreamingPriority {
    /// Video file start - must have for playback
    Critical,
    /// Video continuation, primary subtitles
    High,
    /// Secondary subtitles, metadata
    Medium,
    /// Cover art, thumbnails
    Low,
    /// Extra content, commentary tracks
    Background,
}

/// Complete movie folder content analysis.
#[derive(Debug, Clone)]
pub struct MovieFolder {
    /// File system path to the movie folder
    pub path: PathBuf,
    /// Display name of the movie
    pub name: String,
    /// Total size of all files in bytes
    pub total_size: u64,
    /// All media files found in the folder
    pub files: Vec<MediaFile>,
    /// Index of primary video file in files vector
    pub primary_video: Option<usize>,
    /// Indices of subtitle files in files vector
    pub subtitle_files: Vec<usize>,
    /// Streaming configuration for this content
    pub streaming_profile: StreamingProfile,
}

/// Streaming characteristics for different content types.
#[derive(Debug, Clone)]
pub struct StreamingProfile {
    /// Target playback bitrate in bits per second
    pub bitrate: u64,
    /// How much content to buffer ahead during playback
    pub buffer_duration: Duration,
    /// Initial buffer required before starting playback
    pub startup_buffer: Duration,
    /// Buffer duration reserved for seeking operations
    pub seek_buffer: Duration,
    /// Acceptable timing tolerance for subtitle synchronization
    pub subtitle_sync_tolerance: Duration,
}

impl Default for StreamingProfile {
    fn default() -> Self {
        Self {
            bitrate: 5_000_000,                                  // 5 Mbps
            buffer_duration: Duration::from_secs(30),            // 30 second buffer
            startup_buffer: Duration::from_secs(5),              // 5 second startup
            seek_buffer: Duration::from_secs(10),                // 10 second seek buffer
            subtitle_sync_tolerance: Duration::from_millis(200), // 200ms sync tolerance
        }
    }
}
