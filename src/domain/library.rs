//! Library domain models representing downloaded and available content.

use std::fmt;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::domain::content::ContentId;
use crate::domain::media::MediaId;

/// Unique identifier for library content.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct LibraryId(uuid::Uuid);

impl LibraryId {
    /// Creates a new random library ID.
    pub fn new() -> Self {
        Self(uuid::Uuid::new_v4())
    }

    /// Creates a library ID from a UUID.
    pub fn from_uuid(uuid: uuid::Uuid) -> Self {
        Self(uuid)
    }

    /// Returns the underlying UUID.
    pub fn as_uuid(&self) -> uuid::Uuid {
        self.0
    }
}

impl Default for LibraryId {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Display for LibraryId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Content that has been downloaded or is being downloaded.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LibraryContent {
    id: LibraryId,
    media_id: MediaId,
    content_id: ContentId,
    file_path: PathBuf,
    file_size: u64,
    download_status: DownloadStatus,
    progress_percent: f32,
    download_speed: u64, // bytes per second
    upload_speed: u64,   // bytes per second
    eta_seconds: Option<u64>,
    added_date: chrono::DateTime<chrono::Utc>,
    completed_date: Option<chrono::DateTime<chrono::Utc>>,
}

impl LibraryContent {
    /// Creates new library content entry.
    pub fn new(
        media_id: MediaId,
        content_id: ContentId,
        file_path: PathBuf,
        file_size: u64,
    ) -> Result<Self, LibraryValidationError> {
        let content = Self {
            id: LibraryId::new(),
            media_id,
            content_id,
            file_path,
            file_size,
            download_status: DownloadStatus::Queued,
            progress_percent: 0.0,
            download_speed: 0,
            upload_speed: 0,
            eta_seconds: None,
            added_date: chrono::Utc::now(),
            completed_date: None,
        };

        content.validate()?;
        Ok(content)
    }

    /// Validates library content data.
    pub fn validate(&self) -> Result<(), LibraryValidationError> {
        if self.file_path.as_os_str().is_empty() {
            return Err(LibraryValidationError::EmptyFilePath);
        }

        if self.file_size == 0 {
            return Err(LibraryValidationError::ZeroFileSize);
        }

        if !(0.0..=100.0).contains(&self.progress_percent) {
            return Err(LibraryValidationError::InvalidProgress {
                progress: self.progress_percent,
            });
        }

        Ok(())
    }

    /// Updates download progress.
    pub fn update_progress(
        &mut self,
        progress_percent: f32,
        download_speed: u64,
        upload_speed: u64,
    ) -> Result<(), LibraryValidationError> {
        if !(0.0..=100.0).contains(&progress_percent) {
            return Err(LibraryValidationError::InvalidProgress {
                progress: progress_percent,
            });
        }

        self.progress_percent = progress_percent;
        self.download_speed = download_speed;
        self.upload_speed = upload_speed;

        // Calculate ETA
        if download_speed > 0 && progress_percent < 100.0 {
            let remaining_bytes =
                (self.file_size as f32 * (100.0 - progress_percent) / 100.0) as u64;
            self.eta_seconds = Some(remaining_bytes / download_speed);
        } else {
            self.eta_seconds = None;
        }

        // Update status based on progress
        match progress_percent {
            p if p >= 100.0 => {
                self.download_status = DownloadStatus::Completed;
                if self.completed_date.is_none() {
                    self.completed_date = Some(chrono::Utc::now());
                }
            }
            p if p > 0.0 => {
                if matches!(
                    self.download_status,
                    DownloadStatus::Queued | DownloadStatus::Paused
                ) {
                    self.download_status = DownloadStatus::Downloading;
                }
            }
            _ => {} // Keep current status for 0% progress
        }

        Ok(())
    }

    /// Marks content as failed with error message.
    pub fn mark_failed(&mut self, error_message: String) {
        self.download_status = DownloadStatus::Failed {
            error: error_message,
        };
        self.download_speed = 0;
        self.upload_speed = 0;
        self.eta_seconds = None;
    }

    /// Pauses the download.
    pub fn pause(&mut self) {
        if matches!(self.download_status, DownloadStatus::Downloading) {
            self.download_status = DownloadStatus::Paused;
            self.download_speed = 0;
            self.eta_seconds = None;
        }
    }

    /// Resumes the download.
    pub fn resume(&mut self) {
        if matches!(self.download_status, DownloadStatus::Paused) {
            self.download_status = DownloadStatus::Downloading;
        }
    }

    /// Checks if content is available for streaming.
    pub fn is_streamable(&self) -> bool {
        matches!(self.download_status, DownloadStatus::Completed)
            || (matches!(self.download_status, DownloadStatus::Downloading)
                && self.progress_percent >= 5.0)
    }

    /// Checks if download is currently active.
    pub fn is_active(&self) -> bool {
        matches!(self.download_status, DownloadStatus::Downloading)
    }

    /// Checks if download is completed.
    pub fn is_completed(&self) -> bool {
        matches!(self.download_status, DownloadStatus::Completed)
    }

    /// Formats download speed as human-readable string.
    pub fn format_download_speed(&self) -> String {
        Self::format_speed(self.download_speed)
    }

    /// Formats upload speed as human-readable string.
    pub fn format_upload_speed(&self) -> String {
        Self::format_speed(self.upload_speed)
    }

    /// Formats ETA as human-readable string.
    pub fn format_eta(&self) -> String {
        match self.eta_seconds {
            Some(seconds) => {
                if seconds < 60 {
                    format!("{seconds}s")
                } else if seconds < 3600 {
                    format!("{}m", seconds / 60)
                } else {
                    format!("{}h {}m", seconds / 3600, (seconds % 3600) / 60)
                }
            }
            None => "Unknown".to_string(),
        }
    }

    fn format_speed(bytes_per_second: u64) -> String {
        let speed = bytes_per_second as f64;
        if speed >= 1_048_576.0 {
            format!("{:.1} MB/s", speed / 1_048_576.0)
        } else if speed >= 1024.0 {
            format!("{:.1} KB/s", speed / 1024.0)
        } else {
            format!("{speed} B/s")
        }
    }

    // Getters
    pub fn id(&self) -> LibraryId {
        self.id
    }
    pub fn media_id(&self) -> MediaId {
        self.media_id
    }
    pub fn content_id(&self) -> ContentId {
        self.content_id
    }
    pub fn file_path(&self) -> &PathBuf {
        &self.file_path
    }
    pub fn file_size(&self) -> u64 {
        self.file_size
    }
    pub fn download_status(&self) -> &DownloadStatus {
        &self.download_status
    }
    pub fn progress_percent(&self) -> f32 {
        self.progress_percent
    }
    pub fn download_speed(&self) -> u64 {
        self.download_speed
    }
    pub fn upload_speed(&self) -> u64 {
        self.upload_speed
    }
    pub fn eta_seconds(&self) -> Option<u64> {
        self.eta_seconds
    }
    pub fn added_date(&self) -> chrono::DateTime<chrono::Utc> {
        self.added_date
    }
    pub fn completed_date(&self) -> Option<chrono::DateTime<chrono::Utc>> {
        self.completed_date
    }
}

/// Status of a download in the library.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum DownloadStatus {
    /// Download is queued but not started
    Queued,
    /// Currently downloading
    Downloading,
    /// Download is paused
    Paused,
    /// Download completed successfully
    Completed,
    /// Download failed with error
    Failed { error: String },
}

impl fmt::Display for DownloadStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DownloadStatus::Queued => write!(f, "Queued"),
            DownloadStatus::Downloading => write!(f, "Downloading"),
            DownloadStatus::Paused => write!(f, "Paused"),
            DownloadStatus::Completed => write!(f, "Completed"),
            DownloadStatus::Failed { error } => write!(f, "Failed: {error}"),
        }
    }
}

/// Errors that can occur during library content validation.
#[derive(Debug, thiserror::Error)]
pub enum LibraryValidationError {
    #[error("File path cannot be empty")]
    EmptyFilePath,

    #[error("File size cannot be zero")]
    ZeroFileSize,

    #[error("Invalid progress percentage: {progress} (must be between 0.0 and 100.0)")]
    InvalidProgress { progress: f32 },
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::content::{ContentSource, SourceHealth, SourceType};
    use crate::domain::media::{Media, MediaType, VideoQuality};

    #[test]
    fn test_library_content_creation() {
        let media = Media::new("Test Movie".to_string(), MediaType::Movie);
        let health = SourceHealth::from_swarm_stats(10, 5);
        let content = ContentSource::new(
            media.id(),
            SourceType::Torrent,
            "Test".to_string(),
            1000,
            VideoQuality::BluRay1080p,
            health,
            "magnet:?xt=urn:btih:test".to_string(),
        )
        .unwrap();

        let library_content = LibraryContent::new(
            media.id(),
            content.id(),
            PathBuf::from("/downloads/test_movie.mp4"),
            1_500_000_000,
        )
        .unwrap();

        assert_eq!(library_content.media_id(), media.id());
        assert_eq!(library_content.content_id(), content.id());
        assert!(!library_content.is_streamable()); // 0% progress
    }

    #[test]
    fn test_progress_updates() {
        let media = Media::new("Test".to_string(), MediaType::Movie);
        let health = SourceHealth::from_swarm_stats(10, 5);
        let content = ContentSource::new(
            media.id(),
            SourceType::Torrent,
            "Test".to_string(),
            1000,
            VideoQuality::BluRay1080p,
            health,
            "magnet:?xt=urn:btih:test".to_string(),
        )
        .unwrap();

        let mut library_content = LibraryContent::new(
            media.id(),
            content.id(),
            PathBuf::from("/test.mp4"),
            1_000_000_000, // 1 GB
        )
        .unwrap();

        // Update to 10% progress with 1 MB/s speed
        library_content.update_progress(10.0, 1_048_576, 0).unwrap();
        assert!(library_content.is_streamable()); // >5% progress
        assert!(library_content.is_active());
        assert_eq!(library_content.eta_seconds(), Some(858)); // ~14 minutes

        // Complete download
        library_content.update_progress(100.0, 0, 0).unwrap();
        assert!(library_content.is_completed());
        assert!(library_content.is_streamable());
        assert!(!library_content.is_active());
    }

    #[test]
    fn test_download_status_transitions() {
        let media = Media::new("Test".to_string(), MediaType::Movie);
        let health = SourceHealth::from_swarm_stats(10, 5);
        let content = ContentSource::new(
            media.id(),
            SourceType::Torrent,
            "Test".to_string(),
            1000,
            VideoQuality::BluRay1080p,
            health,
            "magnet:?xt=urn:btih:test".to_string(),
        )
        .unwrap();

        let mut library_content =
            LibraryContent::new(media.id(), content.id(), PathBuf::from("/test.mp4"), 1000)
                .unwrap();

        // Start as queued
        assert!(matches!(
            library_content.download_status(),
            DownloadStatus::Queued
        ));

        // Pause (should stay queued since not downloading)
        library_content.pause();
        assert!(matches!(
            library_content.download_status(),
            DownloadStatus::Queued
        ));

        // Start downloading
        library_content.update_progress(10.0, 1000, 0).unwrap();
        assert!(matches!(
            library_content.download_status(),
            DownloadStatus::Downloading
        ));

        // Pause downloading
        library_content.pause();
        assert!(matches!(
            library_content.download_status(),
            DownloadStatus::Paused
        ));

        // Resume
        library_content.resume();
        assert!(matches!(
            library_content.download_status(),
            DownloadStatus::Downloading
        ));

        // Fail
        library_content.mark_failed("Test error".to_string());
        assert!(matches!(
            library_content.download_status(),
            DownloadStatus::Failed { .. }
        ));
    }

    #[test]
    fn test_speed_formatting() {
        let media = Media::new("Test".to_string(), MediaType::Movie);
        let health = SourceHealth::from_swarm_stats(10, 5);
        let content = ContentSource::new(
            media.id(),
            SourceType::Torrent,
            "Test".to_string(),
            1000,
            VideoQuality::BluRay1080p,
            health,
            "magnet:?xt=urn:btih:test".to_string(),
        )
        .unwrap();

        let mut library_content =
            LibraryContent::new(media.id(), content.id(), PathBuf::from("/test.mp4"), 1000)
                .unwrap();

        library_content
            .update_progress(50.0, 1_048_576, 524_288)
            .unwrap(); // 1 MB/s down, 512 KB/s up
        assert_eq!(library_content.format_download_speed(), "1.0 MB/s");
        assert_eq!(library_content.format_upload_speed(), "512.0 KB/s");
    }

    #[test]
    fn test_eta_formatting() {
        let media = Media::new("Test".to_string(), MediaType::Movie);
        let health = SourceHealth::from_swarm_stats(10, 5);
        let content = ContentSource::new(
            media.id(),
            SourceType::Torrent,
            "Test".to_string(),
            1000,
            VideoQuality::BluRay1080p,
            health,
            "magnet:?xt=urn:btih:test".to_string(),
        )
        .unwrap();

        let mut library_content =
            LibraryContent::new(media.id(), content.id(), PathBuf::from("/test.mp4"), 1000)
                .unwrap();

        // Test different ETA ranges
        library_content.eta_seconds = Some(30);
        assert_eq!(library_content.format_eta(), "30s");

        library_content.eta_seconds = Some(150);
        assert_eq!(library_content.format_eta(), "2m");

        library_content.eta_seconds = Some(3900);
        assert_eq!(library_content.format_eta(), "1h 5m");
    }
}
