//! Video types and quality definitions
//!
//! Simple video format and quality enums for streaming operations.

/// Video quality levels for streaming
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum VideoQuality {
    /// Low quality (480p or lower)
    Low,
    /// Medium quality (720p)
    Medium,
    /// High quality (1080p)
    High,
    /// Ultra high quality (4K)
    Ultra,
}

impl Default for VideoQuality {
    fn default() -> Self {
        Self::Medium
    }
}

/// Video container formats
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum VideoFormat {
    /// MP4 container format
    Mp4,
    /// WebM container format
    Webm,
    /// HLS streaming format
    Hls,
    /// DASH streaming format
    Dash,
}

impl Default for VideoFormat {
    fn default() -> Self {
        Self::Mp4
    }
}
