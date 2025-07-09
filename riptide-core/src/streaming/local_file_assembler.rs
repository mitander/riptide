//! Local file assembler for streaming local media files
//!
//! Provides FileAssembler implementation for files managed by FileLibraryManager,
//! enabling streaming of local files through the unified streaming interface.

use std::ops::Range;
use std::sync::Arc;

use async_trait::async_trait;
use tokio::sync::RwLock;
use tracing::{debug, error};

use super::{FileAssembler, FileAssemblerError};
use crate::storage::FileLibraryManager;
use crate::torrent::InfoHash;

/// FileAssembler implementation for local files managed by FileLibraryManager
pub struct LocalFileAssembler {
    file_manager: Arc<RwLock<FileLibraryManager>>,
}

impl LocalFileAssembler {
    /// Create new local file assembler
    pub fn new(file_manager: Arc<RwLock<FileLibraryManager>>) -> Self {
        Self { file_manager }
    }
}

#[async_trait]
impl FileAssembler for LocalFileAssembler {
    async fn read_range(
        &self,
        info_hash: InfoHash,
        range: Range<u64>,
    ) -> Result<Vec<u8>, FileAssemblerError> {
        // Validate range
        if range.start >= range.end {
            return Err(FileAssemblerError::InvalidRange {
                start: range.start,
                end: range.end,
            });
        }

        let length = range.end - range.start;
        debug!(
            "LocalFileAssembler: Reading range {}..{} (length: {}) for {}",
            range.start, range.end, length, info_hash
        );

        // Read data from file manager
        let file_manager = self.file_manager.read().await;
        match file_manager
            .read_file_segment(info_hash, range.start, length)
            .await
        {
            Ok(data) => {
                debug!(
                    "LocalFileAssembler: Successfully read {} bytes for {}",
                    data.len(),
                    info_hash
                );
                Ok(data)
            }
            Err(e) => {
                error!(
                    "LocalFileAssembler: Failed to read range for {}: {}",
                    info_hash, e
                );
                Err(FileAssemblerError::CacheError {
                    reason: format!("Failed to read local file: {}", e),
                })
            }
        }
    }

    async fn file_size(&self, info_hash: InfoHash) -> Result<u64, FileAssemblerError> {
        debug!("LocalFileAssembler: Getting file size for {}", info_hash);

        let file_manager = self.file_manager.read().await;
        match file_manager.file_by_hash(info_hash) {
            Some(file) => {
                debug!(
                    "LocalFileAssembler: File size for {} is {} bytes",
                    info_hash, file.size
                );
                Ok(file.size)
            }
            None => {
                error!("LocalFileAssembler: File not found for hash {}", info_hash);
                Err(FileAssemblerError::CacheError {
                    reason: format!("Local file not found for hash: {}", info_hash),
                })
            }
        }
    }

    fn is_range_available(&self, info_hash: InfoHash, range: Range<u64>) -> bool {
        // For local files, all ranges are immediately available
        debug!(
            "LocalFileAssembler: Range {}..{} is available for {}",
            range.start, range.end, info_hash
        );
        true
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use tempfile::TempDir;
    use tokio::fs;

    use super::*;

    async fn create_test_file_manager() -> (Arc<RwLock<FileLibraryManager>>, TempDir, InfoHash) {
        let temp_dir = TempDir::new().unwrap();
        let test_file = temp_dir.path().join("test.mp4");

        // Create a test file with some content
        let test_content = b"Test video content for streaming";
        fs::write(&test_file, test_content).await.unwrap();

        let mut file_manager = FileLibraryManager::new();
        file_manager.scan_directory(temp_dir.path()).await.unwrap();

        let file_manager = Arc::new(RwLock::new(file_manager));
        let info_hash = {
            let manager = file_manager.read().await;
            manager.all_files()[0].info_hash
        };

        (file_manager, temp_dir, info_hash)
    }

    #[tokio::test]
    async fn test_file_size() {
        let (file_manager, _temp_dir, info_hash) = create_test_file_manager().await;
        let assembler = LocalFileAssembler::new(file_manager);

        let size = assembler.file_size(info_hash).await.unwrap();
        assert_eq!(size, 32); // Length of test content
    }

    #[tokio::test]
    async fn test_read_range() {
        let (file_manager, _temp_dir, info_hash) = create_test_file_manager().await;
        let assembler = LocalFileAssembler::new(file_manager);

        // Read first 10 bytes
        let data = assembler.read_range(info_hash, 0..10).await.unwrap();
        assert_eq!(data.len(), 10);
        assert_eq!(&data, b"Test video");

        // Read middle portion
        let data = assembler.read_range(info_hash, 5..15).await.unwrap();
        assert_eq!(data.len(), 10);
        assert_eq!(&data, b"video cont");

        // Read to end
        let data = assembler.read_range(info_hash, 20..32).await.unwrap();
        assert_eq!(data.len(), 12);
        assert_eq!(&data, b"r streaming");
    }

    #[tokio::test]
    async fn test_is_range_available() {
        let (file_manager, _temp_dir, info_hash) = create_test_file_manager().await;
        let assembler = LocalFileAssembler::new(file_manager);

        // All ranges should be available for local files
        assert!(assembler.is_range_available(info_hash, 0..10));
        assert!(assembler.is_range_available(info_hash, 10..20));
        assert!(assembler.is_range_available(info_hash, 0..32));
    }

    #[tokio::test]
    async fn test_invalid_range() {
        let (file_manager, _temp_dir, info_hash) = create_test_file_manager().await;
        let assembler = LocalFileAssembler::new(file_manager);

        // Test invalid range (start >= end)
        let result = assembler.read_range(info_hash, 10..10).await;
        assert!(matches!(
            result,
            Err(FileAssemblerError::InvalidRange { .. })
        ));

        let result = assembler.read_range(info_hash, 20..10).await;
        assert!(matches!(
            result,
            Err(FileAssemblerError::InvalidRange { .. })
        ));
    }

    #[tokio::test]
    async fn test_nonexistent_file() {
        let file_manager = Arc::new(RwLock::new(FileLibraryManager::new()));
        let assembler = LocalFileAssembler::new(file_manager);

        let fake_hash = InfoHash::new([1u8; 20]);

        // Test file size for non-existent file
        let result = assembler.file_size(fake_hash).await;
        assert!(matches!(result, Err(FileAssemblerError::CacheError { .. })));

        // Test read range for non-existent file
        let result = assembler.read_range(fake_hash, 0..10).await;
        assert!(matches!(result, Err(FileAssemblerError::CacheError { .. })));
    }

    #[tokio::test]
    async fn test_large_range_read() {
        let (file_manager, _temp_dir, info_hash) = create_test_file_manager().await;
        let assembler = LocalFileAssembler::new(file_manager);

        // Try to read beyond file size
        let result = assembler.read_range(info_hash, 0..100).await;
        // Should not panic, but will return available data
        assert!(result.is_ok());
        let data = result.unwrap();
        assert_eq!(data.len(), 32); // Only available data
    }
}
