//! Streaming domain models representing active playback sessions.

use std::fmt;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::domain::library::LibraryId;

/// Unique identifier for streaming sessions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct StreamId(uuid::Uuid);

impl StreamId {
    /// Creates a new random stream ID.
    pub fn new() -> Self {
        Self(uuid::Uuid::new_v4())
    }

    /// Creates a stream ID from a UUID.
    pub fn from_uuid(uuid: uuid::Uuid) -> Self {
        Self(uuid)
    }

    /// Returns the underlying UUID.
    pub fn as_uuid(&self) -> uuid::Uuid {
        self.0
    }
}

impl Default for StreamId {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Display for StreamId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// An active streaming session for media content.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StreamSession {
    id: StreamId,
    library_id: LibraryId,
    client_info: ClientInfo,
    stream_status: StreamStatus,
    current_position: Duration,
    total_duration: Option<Duration>,
    bitrate: u64, // bits per second
    buffer_health: BufferHealth,
    quality_settings: QualitySettings,
    started_at: chrono::DateTime<chrono::Utc>,
    last_activity: chrono::DateTime<chrono::Utc>,
}

impl StreamSession {
    /// Creates a new streaming session.
    pub fn new(
        library_id: LibraryId,
        client_info: ClientInfo,
        quality_settings: QualitySettings,
    ) -> Result<Self, StreamValidationError> {
        let session = Self {
            id: StreamId::new(),
            library_id,
            client_info,
            stream_status: StreamStatus::Initializing,
            current_position: Duration::ZERO,
            total_duration: None,
            bitrate: 0,
            buffer_health: BufferHealth::empty(),
            quality_settings,
            started_at: chrono::Utc::now(),
            last_activity: chrono::Utc::now(),
        };

        session.validate()?;
        Ok(session)
    }

    /// Validates stream session data.
    pub fn validate(&self) -> Result<(), StreamValidationError> {
        if self.client_info.user_agent.trim().is_empty() {
            return Err(StreamValidationError::EmptyUserAgent);
        }

        if self.quality_settings.max_bitrate > 0 && self.quality_settings.max_bitrate < 64_000 {
            return Err(StreamValidationError::BitrateThresholdTooLow {
                bitrate: self.quality_settings.max_bitrate,
            });
        }

        Ok(())
    }

    /// Updates stream position and status.
    pub fn update_position(&mut self, position: Duration) {
        self.current_position = position;
        self.last_activity = chrono::Utc::now();

        // Auto-update status based on position changes
        if matches!(
            self.stream_status,
            StreamStatus::Initializing | StreamStatus::Buffering
        ) {
            self.stream_status = StreamStatus::Playing;
        }
    }

    /// Updates buffer health metrics.
    pub fn update_buffer_health(&mut self, buffer_health: BufferHealth) {
        // Check status transitions before moving the value
        let should_buffer =
            buffer_health.is_starving() && matches!(self.stream_status, StreamStatus::Playing);
        let should_resume =
            buffer_health.is_healthy() && matches!(self.stream_status, StreamStatus::Buffering);

        self.buffer_health = buffer_health;
        self.last_activity = chrono::Utc::now();

        // Update status based on buffer health
        if should_buffer {
            self.stream_status = StreamStatus::Buffering;
        } else if should_resume {
            self.stream_status = StreamStatus::Playing;
        }
    }

    /// Updates streaming bitrate.
    pub fn update_bitrate(&mut self, bitrate: u64) {
        self.bitrate = bitrate;
        self.last_activity = chrono::Utc::now();
    }

    /// Sets total duration when known.
    pub fn set_total_duration(&mut self, duration: Duration) {
        self.total_duration = Some(duration);
    }

    /// Pauses the stream.
    pub fn pause(&mut self) {
        if matches!(
            self.stream_status,
            StreamStatus::Playing | StreamStatus::Buffering
        ) {
            self.stream_status = StreamStatus::Paused;
            self.last_activity = chrono::Utc::now();
        }
    }

    /// Resumes the stream.
    pub fn resume(&mut self) {
        if matches!(self.stream_status, StreamStatus::Paused) {
            self.stream_status = StreamStatus::Playing;
            self.last_activity = chrono::Utc::now();
        }
    }

    /// Stops the stream.
    pub fn stop(&mut self) {
        self.stream_status = StreamStatus::Stopped;
        self.last_activity = chrono::Utc::now();
    }

    /// Marks stream as failed with error.
    pub fn mark_failed(&mut self, error_message: String) {
        self.stream_status = StreamStatus::Failed {
            error: error_message,
        };
        self.last_activity = chrono::Utc::now();
    }

    /// Seeks to a specific position.
    pub fn seek(&mut self, position: Duration) -> Result<(), StreamValidationError> {
        if let Some(total) = self.total_duration {
            if position > total {
                return Err(StreamValidationError::SeekBeyondDuration {
                    position: position.as_secs(),
                    duration: total.as_secs(),
                });
            }
        }

        self.current_position = position;
        self.last_activity = chrono::Utc::now();

        // May need to buffer after seeking
        if matches!(self.stream_status, StreamStatus::Playing) {
            self.stream_status = StreamStatus::Buffering;
        }

        Ok(())
    }

    /// Calculates progress percentage.
    pub fn progress_percent(&self) -> f32 {
        match self.total_duration {
            Some(total) if total.as_secs() > 0 => {
                (self.current_position.as_secs_f32() / total.as_secs_f32() * 100.0).min(100.0)
            }
            _ => 0.0,
        }
    }

    /// Checks if stream is currently active.
    pub fn is_active(&self) -> bool {
        matches!(
            self.stream_status,
            StreamStatus::Initializing
                | StreamStatus::Playing
                | StreamStatus::Buffering
                | StreamStatus::Paused
        )
    }

    /// Checks if stream has been idle for too long.
    pub fn is_idle(&self, idle_threshold: Duration) -> bool {
        chrono::Utc::now()
            .signed_duration_since(self.last_activity)
            .to_std()
            .unwrap_or(Duration::ZERO)
            > idle_threshold
    }

    /// Formats current position as human-readable string.
    pub fn format_position(&self) -> String {
        Self::format_duration(self.current_position)
    }

    /// Formats total duration as human-readable string.
    pub fn format_duration_total(&self) -> String {
        match self.total_duration {
            Some(duration) => Self::format_duration(duration),
            None => "Unknown".to_string(),
        }
    }

    /// Formats bitrate as human-readable string.
    pub fn format_bitrate(&self) -> String {
        let kbps = self.bitrate / 1000;
        if kbps >= 1000 {
            format!("{:.1} Mbps", kbps as f64 / 1000.0)
        } else {
            format!("{kbps} kbps")
        }
    }

    fn format_duration(duration: Duration) -> String {
        let total_seconds = duration.as_secs();
        let hours = total_seconds / 3600;
        let minutes = (total_seconds % 3600) / 60;
        let seconds = total_seconds % 60;

        if hours > 0 {
            format!("{hours:02}:{minutes:02}:{seconds:02}")
        } else {
            format!("{minutes:02}:{seconds:02}")
        }
    }

    // Getters
    pub fn id(&self) -> StreamId {
        self.id
    }
    pub fn library_id(&self) -> LibraryId {
        self.library_id
    }
    pub fn client_info(&self) -> &ClientInfo {
        &self.client_info
    }
    pub fn stream_status(&self) -> &StreamStatus {
        &self.stream_status
    }
    pub fn current_position(&self) -> Duration {
        self.current_position
    }
    pub fn total_duration(&self) -> Option<Duration> {
        self.total_duration
    }
    pub fn bitrate(&self) -> u64 {
        self.bitrate
    }
    pub fn buffer_health(&self) -> &BufferHealth {
        &self.buffer_health
    }
    pub fn quality_settings(&self) -> &QualitySettings {
        &self.quality_settings
    }
    pub fn started_at(&self) -> chrono::DateTime<chrono::Utc> {
        self.started_at
    }
    pub fn last_activity(&self) -> chrono::DateTime<chrono::Utc> {
        self.last_activity
    }
}

/// Information about the streaming client.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClientInfo {
    pub user_agent: String,
    pub ip_address: String,
    pub supported_codecs: Vec<String>,
    pub max_resolution: Option<Resolution>,
}

impl ClientInfo {
    /// Creates new client info.
    pub fn new(user_agent: String, ip_address: String) -> Self {
        Self {
            user_agent,
            ip_address,
            supported_codecs: vec!["h264".to_string(), "h265".to_string()],
            max_resolution: None,
        }
    }

    /// Checks if client supports a specific codec.
    pub fn supports_codec(&self, codec: &str) -> bool {
        self.supported_codecs
            .iter()
            .any(|c| c.eq_ignore_ascii_case(codec))
    }
}

/// Video resolution.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Resolution {
    pub width: u32,
    pub height: u32,
}

impl Resolution {
    pub fn new(width: u32, height: u32) -> Self {
        Self { width, height }
    }

    /// Common resolution presets.
    pub fn hd_720p() -> Self {
        Self {
            width: 1280,
            height: 720,
        }
    }
    pub fn hd_1080p() -> Self {
        Self {
            width: 1920,
            height: 1080,
        }
    }
    pub fn uhd_4k() -> Self {
        Self {
            width: 3840,
            height: 2160,
        }
    }

    /// Calculates total pixels.
    pub fn pixel_count(&self) -> u64 {
        (self.width as u64) * (self.height as u64)
    }

    /// Checks if this resolution is higher than another.
    pub fn is_higher_than(&self, other: &Resolution) -> bool {
        self.pixel_count() > other.pixel_count()
    }
}

impl fmt::Display for Resolution {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}x{}", self.width, self.height)
    }
}

/// Quality settings for streaming.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QualitySettings {
    pub target_resolution: Option<Resolution>,
    pub max_bitrate: u64, // bits per second
    pub adaptive_quality: bool,
    pub hardware_acceleration: bool,
}

impl QualitySettings {
    /// Creates quality settings optimized for bandwidth.
    pub fn for_bandwidth(max_bitrate: u64) -> Self {
        let target_resolution = if max_bitrate >= 25_000_000 {
            Some(Resolution::uhd_4k())
        } else if max_bitrate >= 8_000_000 {
            Some(Resolution::hd_1080p())
        } else if max_bitrate >= 3_000_000 {
            Some(Resolution::hd_720p())
        } else {
            None // Let adaptive quality decide
        };

        Self {
            target_resolution,
            max_bitrate,
            adaptive_quality: true,
            hardware_acceleration: true,
        }
    }

    /// Creates high quality settings.
    pub fn high_quality() -> Self {
        Self {
            target_resolution: Some(Resolution::hd_1080p()),
            max_bitrate: 15_000_000, // 15 Mbps
            adaptive_quality: false,
            hardware_acceleration: true,
        }
    }

    /// Creates settings optimized for mobile.
    pub fn mobile_optimized() -> Self {
        Self {
            target_resolution: Some(Resolution::hd_720p()),
            max_bitrate: 3_000_000, // 3 Mbps
            adaptive_quality: true,
            hardware_acceleration: false,
        }
    }
}

impl Default for QualitySettings {
    fn default() -> Self {
        Self::for_bandwidth(8_000_000) // 8 Mbps default
    }
}

/// Buffer health metrics.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BufferHealth {
    pub buffer_duration: Duration,
    pub target_buffer: Duration,
    pub buffer_percent: f32,
}

impl BufferHealth {
    /// Creates new buffer health metrics.
    pub fn new(buffer_duration: Duration, target_buffer: Duration) -> Self {
        let buffer_percent = if target_buffer.as_secs() > 0 {
            (buffer_duration.as_secs_f32() / target_buffer.as_secs_f32() * 100.0).min(100.0)
        } else {
            0.0
        };

        Self {
            buffer_duration,
            target_buffer,
            buffer_percent,
        }
    }

    /// Creates empty buffer health.
    pub fn empty() -> Self {
        Self::new(Duration::ZERO, Duration::from_secs(10))
    }

    /// Checks if buffer is healthy for smooth playback.
    pub fn is_healthy(&self) -> bool {
        self.buffer_percent >= 50.0
    }

    /// Checks if buffer is starving (needs immediate attention).
    pub fn is_starving(&self) -> bool {
        self.buffer_duration < Duration::from_secs(2)
    }

    /// Returns buffer status description.
    pub fn status_description(&self) -> &'static str {
        if self.is_starving() {
            "Starving"
        } else if self.buffer_percent >= 80.0 {
            "Excellent"
        } else if self.buffer_percent >= 50.0 {
            "Good"
        } else if self.buffer_percent >= 20.0 {
            "Low"
        } else {
            "Critical"
        }
    }
}

/// Status of a streaming session.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum StreamStatus {
    /// Stream is being initialized
    Initializing,
    /// Currently playing
    Playing,
    /// Temporarily paused by user
    Paused,
    /// Buffering data
    Buffering,
    /// Stream has been stopped
    Stopped,
    /// Stream failed with error
    Failed { error: String },
}

impl fmt::Display for StreamStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            StreamStatus::Initializing => write!(f, "Initializing"),
            StreamStatus::Playing => write!(f, "Playing"),
            StreamStatus::Paused => write!(f, "Paused"),
            StreamStatus::Buffering => write!(f, "Buffering"),
            StreamStatus::Stopped => write!(f, "Stopped"),
            StreamStatus::Failed { error } => write!(f, "Failed: {error}"),
        }
    }
}

/// Errors that can occur during stream validation.
#[derive(Debug, thiserror::Error)]
pub enum StreamValidationError {
    #[error("User agent cannot be empty")]
    EmptyUserAgent,

    #[error("Bitrate threshold too low: {bitrate} bps (minimum 64 kbps)")]
    BitrateThresholdTooLow { bitrate: u64 },

    #[error("Seek position {position}s exceeds duration {duration}s")]
    SeekBeyondDuration { position: u64, duration: u64 },
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::library::LibraryId;

    #[test]
    fn test_stream_session_creation() {
        let library_id = LibraryId::new();
        let client_info = ClientInfo::new("TestAgent/1.0".to_string(), "192.168.1.100".to_string());
        let quality_settings = QualitySettings::default();

        let session = StreamSession::new(library_id, client_info, quality_settings).unwrap();

        assert_eq!(session.library_id(), library_id);
        assert!(session.is_active());
        assert_eq!(session.progress_percent(), 0.0);
    }

    #[test]
    fn test_stream_position_updates() {
        let library_id = LibraryId::new();
        let client_info = ClientInfo::new("TestAgent/1.0".to_string(), "192.168.1.100".to_string());
        let quality_settings = QualitySettings::default();

        let mut session = StreamSession::new(library_id, client_info, quality_settings).unwrap();
        session.set_total_duration(Duration::from_secs(3600)); // 1 hour

        // Update position to 30 minutes
        session.update_position(Duration::from_secs(1800));
        assert_eq!(session.progress_percent(), 50.0);
        assert!(matches!(session.stream_status(), StreamStatus::Playing));
    }

    #[test]
    fn test_buffer_health_status() {
        let healthy_buffer = BufferHealth::new(Duration::from_secs(8), Duration::from_secs(10));
        assert!(healthy_buffer.is_healthy());
        assert!(!healthy_buffer.is_starving());
        assert_eq!(healthy_buffer.status_description(), "Excellent");

        let starving_buffer = BufferHealth::new(Duration::from_secs(1), Duration::from_secs(10));
        assert!(!starving_buffer.is_healthy());
        assert!(starving_buffer.is_starving());
        assert_eq!(starving_buffer.status_description(), "Starving");
    }

    #[test]
    fn test_quality_settings_presets() {
        let high_quality = QualitySettings::high_quality();
        assert_eq!(high_quality.max_bitrate, 15_000_000);
        assert!(!high_quality.adaptive_quality);

        let mobile = QualitySettings::mobile_optimized();
        assert_eq!(mobile.max_bitrate, 3_000_000);
        assert!(mobile.adaptive_quality);

        let bandwidth_limited = QualitySettings::for_bandwidth(5_000_000);
        assert_eq!(
            bandwidth_limited.target_resolution,
            Some(Resolution::hd_720p())
        );
    }

    #[test]
    fn test_resolution_comparison() {
        let hd = Resolution::hd_1080p();
        let uhd = Resolution::uhd_4k();

        assert!(uhd.is_higher_than(&hd));
        assert!(!hd.is_higher_than(&uhd));
        assert_eq!(hd.to_string(), "1920x1080");
    }

    #[test]
    fn test_stream_status_transitions() {
        let library_id = LibraryId::new();
        let client_info = ClientInfo::new("TestAgent/1.0".to_string(), "192.168.1.100".to_string());
        let quality_settings = QualitySettings::default();

        let mut session = StreamSession::new(library_id, client_info, quality_settings).unwrap();

        // Start as initializing
        assert!(matches!(
            session.stream_status(),
            StreamStatus::Initializing
        ));

        // Update position should move to playing
        session.update_position(Duration::from_secs(10));
        assert!(matches!(session.stream_status(), StreamStatus::Playing));

        // Pause
        session.pause();
        assert!(matches!(session.stream_status(), StreamStatus::Paused));

        // Resume
        session.resume();
        assert!(matches!(session.stream_status(), StreamStatus::Playing));

        // Stop
        session.stop();
        assert!(matches!(session.stream_status(), StreamStatus::Stopped));
        assert!(!session.is_active());
    }

    #[test]
    fn test_seek_validation() {
        let library_id = LibraryId::new();
        let client_info = ClientInfo::new("TestAgent/1.0".to_string(), "192.168.1.100".to_string());
        let quality_settings = QualitySettings::default();

        let mut session = StreamSession::new(library_id, client_info, quality_settings).unwrap();
        session.set_total_duration(Duration::from_secs(3600)); // 1 hour

        // Valid seek
        let result = session.seek(Duration::from_secs(1800)); // 30 minutes
        assert!(result.is_ok());
        assert_eq!(session.current_position(), Duration::from_secs(1800));

        // Invalid seek beyond duration
        let result = session.seek(Duration::from_secs(4000));
        assert!(result.is_err());
    }

    #[test]
    fn test_client_codec_support() {
        let mut client_info =
            ClientInfo::new("TestAgent/1.0".to_string(), "192.168.1.100".to_string());
        client_info.supported_codecs = vec!["h264".to_string(), "av1".to_string()];

        assert!(client_info.supports_codec("h264"));
        assert!(client_info.supports_codec("H264")); // Case insensitive
        assert!(client_info.supports_codec("av1"));
        assert!(!client_info.supports_codec("h265"));
    }

    #[test]
    fn test_duration_formatting() {
        let library_id = LibraryId::new();
        let client_info = ClientInfo::new("TestAgent/1.0".to_string(), "192.168.1.100".to_string());
        let quality_settings = QualitySettings::default();

        let mut session = StreamSession::new(library_id, client_info, quality_settings).unwrap();
        session.set_total_duration(Duration::from_secs(3665)); // 1:01:05
        session.update_position(Duration::from_secs(125)); // 2:05

        assert_eq!(session.format_position(), "02:05");
        assert_eq!(session.format_duration_total(), "01:01:05");
    }

    #[test]
    fn test_idle_detection() {
        let library_id = LibraryId::new();
        let client_info = ClientInfo::new("TestAgent/1.0".to_string(), "192.168.1.100".to_string());
        let quality_settings = QualitySettings::default();

        let session = StreamSession::new(library_id, client_info, quality_settings).unwrap();

        // Should not be idle immediately
        assert!(!session.is_idle(Duration::from_secs(300)));

        // Test would require manual time manipulation for true idle state
    }
}
