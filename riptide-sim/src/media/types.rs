//! Media file types and analysis for streaming simulation

use std::path::PathBuf;
use std::time::Duration;

/// Media file types found in movie folders.
#[derive(Debug, Clone, PartialEq)]
pub enum MediaFileType {
    Video {
        codec: String,
        bitrate: u64,
        duration: Duration,
    },
    Subtitle {
        language: String,
        format: String,
    },
    Metadata {
        format: String,
    },
    Audio {
        codec: String,
        bitrate: u64,
    },
    Image {
        purpose: String,
    }, // Cover art, thumbnails, etc.
}

/// Individual media file within a movie folder.
#[derive(Debug, Clone)]
pub struct MediaFile {
    pub path: PathBuf,
    pub size: u64,
    pub file_type: MediaFileType,
    pub priority: StreamingPriority,
}

/// Streaming priority levels for different file types.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum StreamingPriority {
    Critical,   // Video file start - must have for playback
    High,       // Video continuation, primary subtitles
    Medium,     // Secondary subtitles, metadata
    Low,        // Cover art, thumbnails
    Background, // Extra content, commentary tracks
}

/// Complete movie folder content analysis.
#[derive(Debug, Clone)]
pub struct MovieFolder {
    pub path: PathBuf,
    pub name: String,
    pub total_size: u64,
    pub files: Vec<MediaFile>,
    pub primary_video: Option<usize>, // Index into files vec
    pub subtitle_files: Vec<usize>,   // Indices of subtitle files
    pub streaming_profile: StreamingProfile,
}

/// Streaming characteristics for different content types.
#[derive(Debug, Clone)]
pub struct StreamingProfile {
    pub bitrate: u64,                      // Target playback bitrate
    pub buffer_duration: Duration,         // How much to buffer ahead
    pub startup_buffer: Duration,          // Initial buffer before playback
    pub seek_buffer: Duration,             // Buffer for seeking operations
    pub subtitle_sync_tolerance: Duration, // Subtitle timing tolerance
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
