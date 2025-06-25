//! Content management service for handling downloads and library operations.

use std::path::PathBuf;

use tracing::{debug, error, info, warn};

use crate::domain::content::{ContentId, ContentSource};
use crate::domain::errors::DomainError;
use crate::domain::library::{LibraryContent, LibraryId, LibraryValidationError};
use crate::domain::media::{Media, MediaId};

/// Errors that can occur during content management operations.
#[derive(Debug, thiserror::Error)]
pub enum ContentManagerError {
    #[error("Download failed for content {content_id}: {reason}")]
    DownloadFailed {
        content_id: ContentId,
        reason: String,
    },

    #[error("Content not found in library: {library_id}")]
    ContentNotFound { library_id: LibraryId },

    #[error("Storage operation failed: {reason}")]
    StorageFailed { reason: String },

    #[error("Domain validation failed")]
    DomainValidation(#[from] DomainError),

    #[error("Library validation failed")]
    LibraryValidation(#[from] LibraryValidationError),

    #[error("Invalid file path: {path}")]
    InvalidPath { path: String },

    #[error("Insufficient disk space: {required} bytes needed, {available} bytes available")]
    InsufficientSpace { required: u64, available: u64 },
}

/// Service for managing content downloads and library operations.
pub struct ContentManager {
    download_client: Box<dyn DownloadClient>,
    storage_service: Box<dyn StorageService>,
    active_downloads: std::collections::HashMap<LibraryId, Box<dyn DownloadHandle>>,
    download_directory: PathBuf,
    max_concurrent_downloads: usize,
}

impl ContentManager {
    /// Creates a new content manager.
    pub fn new(
        download_client: Box<dyn DownloadClient>,
        storage_service: Box<dyn StorageService>,
        download_directory: PathBuf,
    ) -> Self {
        Self {
            download_client,
            storage_service,
            active_downloads: std::collections::HashMap::new(),
            download_directory,
            max_concurrent_downloads: 3,
        }
    }

    /// Starts downloading content from a source.
    ///
    /// Creates a library entry and begins download. The download runs in the background
    /// and progress can be monitored via the returned LibraryContent.
    ///
    /// # Errors
    /// - `ContentManagerError::DownloadFailed` - Download could not be started
    /// - `ContentManagerError::InsufficientSpace` - Not enough disk space
    /// - `ContentManagerError::StorageFailed` - Storage setup failed
    /// - `ContentManagerError::DomainValidation` - Invalid content data
    pub async fn start_download(
        &mut self,
        media: &Media,
        source: &ContentSource,
    ) -> Result<LibraryContent, ContentManagerError> {
        info!(
            "Starting download for media '{}' from source '{}'",
            media.title(),
            source.name()
        );

        // Check download limits
        if self.active_downloads.len() >= self.max_concurrent_downloads {
            return Err(ContentManagerError::DownloadFailed {
                content_id: source.id(),
                reason: "Maximum concurrent downloads reached".to_string(),
            });
        }

        // Check disk space
        let available_space = self.storage_service.available_space().await.map_err(|e| {
            ContentManagerError::StorageFailed {
                reason: e.to_string(),
            }
        })?;

        if source.size_bytes() > available_space {
            return Err(ContentManagerError::InsufficientSpace {
                required: source.size_bytes(),
                available: available_space,
            });
        }

        // Prepare download path
        let file_path = self.generate_download_path(media, source)?;

        // Create library content entry
        let library_content = LibraryContent::new(
            media.id(),
            source.id(),
            file_path.clone(),
            source.size_bytes(),
        )?;

        // Start download
        let download_handle = self
            .download_client
            .start_download(source, file_path.clone())
            .await
            .map_err(|e| ContentManagerError::DownloadFailed {
                content_id: source.id(),
                reason: e.to_string(),
            })?;

        // Track active download
        self.active_downloads
            .insert(library_content.id(), download_handle);

        info!(
            "Download started for library content: {}",
            library_content.id()
        );

        Ok(library_content)
    }

    /// Updates progress for all active downloads.
    ///
    /// Should be called periodically to refresh download status and progress.
    pub async fn update_download_progress(&mut self) -> Vec<LibraryContent> {
        let mut updated_content = Vec::new();
        let mut completed_downloads = Vec::new();

        for (library_id, handle) in &mut self.active_downloads {
            match handle.get_progress().await {
                Ok(progress) => {
                    debug!(
                        "Download progress for {}: {:.1}% at {}/s",
                        library_id,
                        progress.percent,
                        Self::format_speed(progress.download_speed)
                    );

                    // Update library content with progress
                    let mut library_content = LibraryContent::new(
                        progress.media_id,
                        progress.content_id,
                        progress.file_path.clone(),
                        progress.total_size,
                    )
                    .unwrap();

                    if let Err(e) = library_content.update_progress(
                        progress.percent,
                        progress.download_speed,
                        progress.upload_speed,
                    ) {
                        warn!("Failed to update progress for {}: {}", library_id, e);
                        continue;
                    }

                    // Check if download completed
                    if progress.percent >= 100.0 {
                        completed_downloads.push(*library_id);
                        info!("Download completed: {}", library_id);
                    }

                    updated_content.push(library_content);
                }
                Err(e) => {
                    error!("Download failed for {}: {}", library_id, e);

                    // Mark as failed
                    let mut library_content = LibraryContent::new(
                        MediaId::new(), // We'd need to track this properly
                        ContentId::new(),
                        PathBuf::new(),
                        0,
                    )
                    .unwrap();
                    library_content.mark_failed(e.to_string());
                    updated_content.push(library_content);

                    completed_downloads.push(*library_id);
                }
            }
        }

        // Clean up completed downloads
        for library_id in completed_downloads {
            self.active_downloads.remove(&library_id);
        }

        updated_content
    }

    /// Pauses a download.
    pub async fn pause_download(
        &mut self,
        library_id: LibraryId,
    ) -> Result<(), ContentManagerError> {
        if let Some(handle) = self.active_downloads.get_mut(&library_id) {
            handle
                .pause()
                .await
                .map_err(|e| ContentManagerError::DownloadFailed {
                    content_id: ContentId::new(), // We'd need to track this
                    reason: e.to_string(),
                })?;
            info!("Download paused: {}", library_id);
        }
        Ok(())
    }

    /// Resumes a paused download.
    pub async fn resume_download(
        &mut self,
        library_id: LibraryId,
    ) -> Result<(), ContentManagerError> {
        if let Some(handle) = self.active_downloads.get_mut(&library_id) {
            handle
                .resume()
                .await
                .map_err(|e| ContentManagerError::DownloadFailed {
                    content_id: ContentId::new(), // We'd need to track this
                    reason: e.to_string(),
                })?;
            info!("Download resumed: {}", library_id);
        }
        Ok(())
    }

    /// Cancels a download and removes partially downloaded files.
    pub async fn cancel_download(
        &mut self,
        library_id: LibraryId,
    ) -> Result<(), ContentManagerError> {
        if let Some(mut handle) = self.active_downloads.remove(&library_id) {
            handle
                .cancel()
                .await
                .map_err(|e| ContentManagerError::DownloadFailed {
                    content_id: ContentId::new(), // We'd need to track this
                    reason: e.to_string(),
                })?;

            info!("Download cancelled: {}", library_id);
        }
        Ok(())
    }

    /// Removes content from the library and deletes files.
    pub async fn remove_content(
        &mut self,
        library_id: LibraryId,
    ) -> Result<(), ContentManagerError> {
        // Cancel download if active
        if self.active_downloads.contains_key(&library_id) {
            self.cancel_download(library_id).await?;
        }

        // Remove files from storage
        self.storage_service
            .remove_content(library_id)
            .await
            .map_err(|e| ContentManagerError::StorageFailed {
                reason: e.to_string(),
            })?;

        info!("Content removed from library: {}", library_id);
        Ok(())
    }

    /// Gets the number of active downloads.
    pub fn active_download_count(&self) -> usize {
        self.active_downloads.len()
    }

    /// Checks if a download is currently active.
    pub fn is_download_active(&self, library_id: LibraryId) -> bool {
        self.active_downloads.contains_key(&library_id)
    }

    /// Sets maximum concurrent downloads.
    pub fn set_max_concurrent_downloads(&mut self, max: usize) {
        self.max_concurrent_downloads = max;
        info!("Maximum concurrent downloads set to: {}", max);
    }

    /// Generates appropriate file path for downloaded content.
    fn generate_download_path(
        &self,
        media: &Media,
        source: &ContentSource,
    ) -> Result<PathBuf, ContentManagerError> {
        let sanitized_title = Self::sanitize_filename(media.title());
        let extension = Self::guess_file_extension(source.name());

        let filename = if let Some(year) = media.release_year() {
            format!("{sanitized_title} ({year}).{extension}")
        } else {
            format!("{sanitized_title}.{extension}")
        };

        let full_path = self.download_directory.join(filename);

        // Validate path
        if full_path.to_string_lossy().len() > 255 {
            return Err(ContentManagerError::InvalidPath {
                path: full_path.to_string_lossy().to_string(),
            });
        }

        Ok(full_path)
    }

    /// Sanitizes filename by removing invalid characters.
    fn sanitize_filename(input: &str) -> String {
        input
            .chars()
            .map(|c| match c {
                '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
                c => c,
            })
            .collect()
    }

    /// Guesses file extension from source name.
    fn guess_file_extension(source_name: &str) -> &str {
        if source_name.to_lowercase().contains("mkv") {
            "mkv"
        } else if source_name.to_lowercase().contains("avi") {
            "avi"
        } else if source_name.to_lowercase().contains("mov") {
            "mov"
        } else {
            "mp4" // Default
        }
    }

    /// Formats download speed as human-readable string.
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
}

/// Trait for download client implementations.
#[async_trait::async_trait]
pub trait DownloadClient: Send + Sync {
    /// Starts downloading from a content source.
    async fn start_download(
        &self,
        source: &ContentSource,
        destination: PathBuf,
    ) -> Result<Box<dyn DownloadHandle>, DownloadError>;
}

/// Trait for storage service implementations.
#[async_trait::async_trait]
pub trait StorageService: Send + Sync {
    /// Returns available disk space in bytes.
    async fn available_space(&self) -> Result<u64, StorageError>;

    /// Removes content files from storage.
    async fn remove_content(&self, library_id: LibraryId) -> Result<(), StorageError>;

    /// Verifies file integrity.
    async fn verify_file(&self, path: &std::path::Path) -> Result<bool, StorageError>;
}

/// Handle for managing an active download.
#[async_trait::async_trait]
pub trait DownloadHandle: Send + Sync {
    /// Gets current download progress.
    async fn get_progress(&self) -> Result<DownloadProgress, DownloadError>;

    /// Pauses the download.
    async fn pause(&mut self) -> Result<(), DownloadError>;

    /// Resumes the download.
    async fn resume(&mut self) -> Result<(), DownloadError>;

    /// Cancels the download and cleans up files.
    async fn cancel(&mut self) -> Result<(), DownloadError>;
}

/// Download progress information.
#[derive(Debug, Clone)]
pub struct DownloadProgress {
    pub media_id: MediaId,
    pub content_id: ContentId,
    pub file_path: PathBuf,
    pub total_size: u64,
    pub downloaded_size: u64,
    pub percent: f32,
    pub download_speed: u64, // bytes per second
    pub upload_speed: u64,   // bytes per second
    pub eta_seconds: Option<u64>,
}

/// Errors that can occur during download operations.
#[derive(Debug, thiserror::Error)]
pub enum DownloadError {
    #[error("Network error: {reason}")]
    NetworkError { reason: String },

    #[error("File I/O error: {reason}")]
    IoError { reason: String },

    #[error("Download was cancelled")]
    Cancelled,

    #[error("Download failed: {reason}")]
    Failed { reason: String },
}

/// Errors that can occur during storage operations.
#[derive(Debug, thiserror::Error)]
pub enum StorageError {
    #[error("Disk I/O error: {reason}")]
    IoError { reason: String },

    #[error("File not found: {path}")]
    FileNotFound { path: String },

    #[error("Permission denied: {path}")]
    PermissionDenied { path: String },

    #[error("Insufficient disk space")]
    InsufficientSpace,
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use tokio::sync::Mutex;

    use super::*;
    use crate::domain::content::{ContentSource, SourceHealth, SourceType};
    use crate::domain::media::{Media, MediaType, VideoQuality};

    /// Mock download client for testing.
    struct MockDownloadClient {
        should_fail: bool,
    }

    impl MockDownloadClient {
        fn new() -> Self {
            Self { should_fail: false }
        }

        fn with_failure() -> Self {
            Self { should_fail: true }
        }
    }

    #[async_trait::async_trait]
    impl DownloadClient for MockDownloadClient {
        async fn start_download(
            &self,
            source: &ContentSource,
            _destination: PathBuf,
        ) -> Result<Box<dyn DownloadHandle>, DownloadError> {
            if self.should_fail {
                return Err(DownloadError::Failed {
                    reason: "Mock failure".to_string(),
                });
            }

            Ok(Box::new(MockDownloadHandle::new(
                source.id(),
                source.size_bytes(),
            )))
        }
    }

    /// Mock download handle for testing.
    struct MockDownloadHandle {
        content_id: ContentId,
        total_size: u64,
        progress: Arc<Mutex<f32>>,
    }

    impl MockDownloadHandle {
        fn new(content_id: ContentId, total_size: u64) -> Self {
            Self {
                content_id,
                total_size,
                progress: Arc::new(Mutex::new(0.0)),
            }
        }
    }

    #[async_trait::async_trait]
    impl DownloadHandle for MockDownloadHandle {
        async fn get_progress(&self) -> Result<DownloadProgress, DownloadError> {
            let progress = *self.progress.lock().await;
            Ok(DownloadProgress {
                media_id: MediaId::new(),
                content_id: self.content_id,
                file_path: PathBuf::from("/test/path"),
                total_size: self.total_size,
                downloaded_size: (self.total_size as f32 * progress / 100.0) as u64,
                percent: progress,
                download_speed: 1_048_576, // 1 MB/s
                upload_speed: 0,
                eta_seconds: Some(60),
            })
        }

        async fn pause(&mut self) -> Result<(), DownloadError> {
            Ok(())
        }

        async fn resume(&mut self) -> Result<(), DownloadError> {
            Ok(())
        }

        async fn cancel(&mut self) -> Result<(), DownloadError> {
            Ok(())
        }
    }

    /// Mock storage service for testing.
    struct MockStorageService {
        available_space: u64,
    }

    impl MockStorageService {
        fn new(available_space: u64) -> Self {
            Self { available_space }
        }
    }

    #[async_trait::async_trait]
    impl StorageService for MockStorageService {
        async fn available_space(&self) -> Result<u64, StorageError> {
            Ok(self.available_space)
        }

        async fn remove_content(&self, _library_id: LibraryId) -> Result<(), StorageError> {
            Ok(())
        }

        async fn verify_file(&self, _path: &std::path::Path) -> Result<bool, StorageError> {
            Ok(true)
        }
    }

    #[tokio::test]
    async fn test_content_manager_creation() {
        let download_client = Box::new(MockDownloadClient::new());
        let storage_service = Box::new(MockStorageService::new(10_000_000_000)); // 10 GB
        let download_dir = PathBuf::from("/downloads");

        let manager = ContentManager::new(download_client, storage_service, download_dir);
        assert_eq!(manager.active_download_count(), 0);
    }

    #[tokio::test]
    async fn test_start_download_success() {
        let download_client = Box::new(MockDownloadClient::new());
        let storage_service = Box::new(MockStorageService::new(10_000_000_000));
        let download_dir = PathBuf::from("/downloads");

        let mut manager = ContentManager::new(download_client, storage_service, download_dir);

        let media = Media::new("Test Movie".to_string(), MediaType::Movie);
        let source = ContentSource::new(
            media.id(),
            SourceType::Torrent,
            "Test.Movie.2024.1080p.mkv".to_string(),
            2_000_000_000, // 2 GB
            VideoQuality::BluRay1080p,
            SourceHealth::from_swarm_stats(50, 10),
            "magnet:?xt=urn:btih:test".to_string(),
        )
        .unwrap();

        let library_content = manager.start_download(&media, &source).await.unwrap();
        assert_eq!(library_content.media_id(), media.id());
        assert_eq!(library_content.content_id(), source.id());
        assert_eq!(manager.active_download_count(), 1);
    }

    #[tokio::test]
    async fn test_insufficient_space_error() {
        let download_client = Box::new(MockDownloadClient::new());
        let storage_service = Box::new(MockStorageService::new(1_000_000)); // 1 MB only
        let download_dir = PathBuf::from("/downloads");

        let mut manager = ContentManager::new(download_client, storage_service, download_dir);

        let media = Media::new("Test Movie".to_string(), MediaType::Movie);
        let source = ContentSource::new(
            media.id(),
            SourceType::Torrent,
            "Test.Movie.2024.1080p.mkv".to_string(),
            2_000_000_000, // 2 GB - too large
            VideoQuality::BluRay1080p,
            SourceHealth::from_swarm_stats(50, 10),
            "magnet:?xt=urn:btih:test".to_string(),
        )
        .unwrap();

        let result = manager.start_download(&media, &source).await;
        assert!(matches!(
            result,
            Err(ContentManagerError::InsufficientSpace { .. })
        ));
    }

    #[tokio::test]
    async fn test_filename_sanitization() {
        let test_cases = vec![
            ("Normal Movie", "Normal Movie"),
            ("Movie/With\\Bad:Chars", "Movie_With_Bad_Chars"),
            ("Movie*With?\"<>|Special", "Movie_With_____Special"),
        ];

        for (input, expected) in test_cases {
            let result = ContentManager::sanitize_filename(input);
            assert_eq!(result, expected);
        }
    }

    #[tokio::test]
    async fn test_file_extension_guessing() {
        let test_cases = vec![
            ("Movie.2024.1080p.mkv.torrent", "mkv"),
            ("Film.AVI.Release", "avi"),
            ("Video.MOV.Upload", "mov"),
            ("Unknown.Format.File", "mp4"),
        ];

        for (input, expected) in test_cases {
            let result = ContentManager::guess_file_extension(input);
            assert_eq!(result, expected);
        }
    }

    #[tokio::test]
    async fn test_concurrent_download_limit() {
        let download_client = Box::new(MockDownloadClient::new());
        let storage_service = Box::new(MockStorageService::new(10_000_000_000));
        let download_dir = PathBuf::from("/downloads");

        let mut manager = ContentManager::new(download_client, storage_service, download_dir);
        manager.set_max_concurrent_downloads(1); // Only allow 1 download

        let media1 = Media::new("Test Movie 1".to_string(), MediaType::Movie);
        let source1 = ContentSource::new(
            media1.id(),
            SourceType::Torrent,
            "Test.Movie.1.mkv".to_string(),
            1_000_000_000,
            VideoQuality::BluRay1080p,
            SourceHealth::from_swarm_stats(50, 10),
            "magnet:?xt=urn:btih:test1".to_string(),
        )
        .unwrap();

        let media2 = Media::new("Test Movie 2".to_string(), MediaType::Movie);
        let source2 = ContentSource::new(
            media2.id(),
            SourceType::Torrent,
            "Test.Movie.2.mkv".to_string(),
            1_000_000_000,
            VideoQuality::BluRay1080p,
            SourceHealth::from_swarm_stats(50, 10),
            "magnet:?xt=urn:btih:test2".to_string(),
        )
        .unwrap();

        // First download should succeed
        let _result1 = manager.start_download(&media1, &source1).await.unwrap();
        assert_eq!(manager.active_download_count(), 1);

        // Second download should fail due to limit
        let result2 = manager.start_download(&media2, &source2).await;
        assert!(matches!(
            result2,
            Err(ContentManagerError::DownloadFailed { .. })
        ));
    }
}
