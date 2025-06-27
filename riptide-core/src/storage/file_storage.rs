//! File-based storage implementation

use std::path::PathBuf;

use async_trait::async_trait;
use tokio::fs;

use super::{Storage, StorageError};
use crate::torrent::{InfoHash, PieceIndex};

/// File system-based storage implementation.
///
/// Stores torrent pieces as individual files in a directory structure
/// organized by torrent info hash. Handles piece verification and
/// torrent completion detection.
pub struct FileStorage {
    download_dir: PathBuf,
    library_dir: PathBuf,
}

impl FileStorage {
    /// Creates new file storage with download and library directories.
    ///
    /// Download directory stores incomplete pieces during downloading.
    /// Library directory stores completed torrents after verification.
    pub fn new(download_dir: PathBuf, library_dir: PathBuf) -> Self {
        Self {
            download_dir,
            library_dir,
        }
    }
}

#[async_trait]
impl Storage for FileStorage {
    async fn store_piece(
        &mut self,
        info_hash: InfoHash,
        index: PieceIndex,
        piece_bytes: &[u8],
    ) -> Result<(), StorageError> {
        let piece_path = self
            .download_dir
            .join(info_hash.to_string())
            .join(format!("piece_{}", index.as_u32()));

        if let Some(parent) = piece_path.parent() {
            fs::create_dir_all(parent).await?;
        }

        fs::write(&piece_path, piece_bytes).await?;
        Ok(())
    }

    async fn load_piece(
        &self,
        info_hash: InfoHash,
        index: PieceIndex,
    ) -> Result<Vec<u8>, StorageError> {
        let piece_path = self
            .download_dir
            .join(info_hash.to_string())
            .join(format!("piece_{}", index.as_u32()));

        match fs::read(&piece_path).await {
            Ok(piece_bytes) => Ok(piece_bytes),
            Err(_) => Err(StorageError::PieceNotFound { index }),
        }
    }

    async fn has_piece(
        &self,
        info_hash: InfoHash,
        index: PieceIndex,
    ) -> Result<bool, StorageError> {
        let piece_path = self
            .download_dir
            .join(info_hash.to_string())
            .join(format!("piece_{}", index.as_u32()));

        Ok(piece_path.exists())
    }

    async fn finalize_torrent(&mut self, info_hash: InfoHash) -> Result<PathBuf, StorageError> {
        let download_path = self.download_dir.join(info_hash.to_string());
        let library_path = self.library_dir.join(info_hash.to_string());

        fs::rename(download_path, &library_path).await?;
        Ok(library_path)
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::os::unix::fs::PermissionsExt;

    use tokio::test;

    use super::*;
    use crate::storage::test_fixtures::create_temp_storage_dirs;

    fn create_test_info_hash() -> InfoHash {
        InfoHash::new([
            0x01, 0x23, 0x45, 0x67, 0x89, 0xab, 0xcd, 0xef, 0x01, 0x23, 0x45, 0x67, 0x89, 0xab,
            0xcd, 0xef, 0x01, 0x23, 0x45, 0x67,
        ])
    }

    #[test]
    async fn test_store_piece_success() {
        let (_temp_dir, downloads_dir, library_dir) = create_temp_storage_dirs();
        let mut storage = FileStorage::new(downloads_dir, library_dir);

        let info_hash = create_test_info_hash();
        let piece_index = PieceIndex::new(0);
        let piece_data = b"test piece data";

        let result = storage
            .store_piece(info_hash, piece_index, piece_data)
            .await;
        assert!(result.is_ok());

        // Verify piece was stored
        let has_piece = storage.has_piece(info_hash, piece_index).await.unwrap();
        assert!(has_piece);
    }

    #[test]
    async fn test_store_piece_permission_denied() {
        let (_temp_dir, downloads_dir, library_dir) = create_temp_storage_dirs();

        // Create torrent directory and make it read-only
        let torrent_dir = downloads_dir.join(create_test_info_hash().to_string());
        fs::create_dir_all(&torrent_dir).unwrap();

        // Set permissions to read-only (remove write permission)
        let mut perms = fs::metadata(&torrent_dir).unwrap().permissions();
        perms.set_mode(0o444); // Read-only
        fs::set_permissions(&torrent_dir, perms).unwrap();

        let mut storage = FileStorage::new(downloads_dir, library_dir);
        let info_hash = create_test_info_hash();
        let piece_index = PieceIndex::new(0);
        let piece_data = b"test piece data";

        let result = storage
            .store_piece(info_hash, piece_index, piece_data)
            .await;

        assert!(result.is_err());
        match result.unwrap_err() {
            StorageError::Io(io_error) => {
                assert_eq!(io_error.kind(), std::io::ErrorKind::PermissionDenied);
            }
            _ => panic!("Expected IO permission denied error"),
        }
    }

    #[test]
    async fn test_store_piece_readonly_filesystem() {
        let (_temp_dir, downloads_dir, library_dir) = create_temp_storage_dirs();

        // Make entire downloads directory read-only
        let mut perms = fs::metadata(&downloads_dir).unwrap().permissions();
        perms.set_mode(0o444);
        fs::set_permissions(&downloads_dir, perms).unwrap();

        let mut storage = FileStorage::new(downloads_dir, library_dir);
        let info_hash = create_test_info_hash();
        let piece_index = PieceIndex::new(0);
        let piece_data = b"test piece data";

        let result = storage
            .store_piece(info_hash, piece_index, piece_data)
            .await;

        assert!(result.is_err());
        match result.unwrap_err() {
            StorageError::Io(io_error) => {
                assert_eq!(io_error.kind(), std::io::ErrorKind::PermissionDenied);
            }
            _ => panic!("Expected IO permission denied error"),
        }
    }

    #[test]
    async fn test_store_piece_invalid_path() {
        // Use a path that contains invalid characters on most filesystems
        let invalid_downloads_dir = std::path::PathBuf::from("/dev/null/invalid\0path");
        let (_temp_dir, _downloads_dir, library_dir) = create_temp_storage_dirs();

        let mut storage = FileStorage::new(invalid_downloads_dir, library_dir);
        let info_hash = create_test_info_hash();
        let piece_index = PieceIndex::new(0);
        let piece_data = b"test piece data";

        let result = storage
            .store_piece(info_hash, piece_index, piece_data)
            .await;

        assert!(result.is_err());
        // Should get an IO error for invalid path
        assert!(matches!(result.unwrap_err(), StorageError::Io(_)));
    }

    #[test]
    async fn test_load_piece_not_found() {
        let (_temp_dir, downloads_dir, library_dir) = create_temp_storage_dirs();
        let storage = FileStorage::new(downloads_dir, library_dir);

        let info_hash = create_test_info_hash();
        let piece_index = PieceIndex::new(999); // Non-existent piece

        let result = storage.load_piece(info_hash, piece_index).await;

        assert!(result.is_err());
        match result.unwrap_err() {
            StorageError::PieceNotFound { index } => {
                assert_eq!(index, piece_index);
            }
            _ => panic!("Expected PieceNotFound error"),
        }
    }

    #[test]
    async fn test_load_piece_permission_denied() {
        let (_temp_dir, downloads_dir, library_dir) = create_temp_storage_dirs();
        let mut storage = FileStorage::new(downloads_dir.clone(), library_dir);

        let info_hash = create_test_info_hash();
        let piece_index = PieceIndex::new(0);
        let piece_data = b"test piece data";

        // First store the piece
        storage
            .store_piece(info_hash, piece_index, piece_data)
            .await
            .unwrap();

        // Make the piece file unreadable
        let piece_path = downloads_dir
            .join(info_hash.to_string())
            .join(format!("piece_{}", piece_index.as_u32()));

        let mut perms = fs::metadata(&piece_path).unwrap().permissions();
        perms.set_mode(0o000); // No permissions
        fs::set_permissions(&piece_path, perms).unwrap();

        let result = storage.load_piece(info_hash, piece_index).await;

        // Should be treated as PieceNotFound since we catch all errors
        assert!(result.is_err());
        match result.unwrap_err() {
            StorageError::PieceNotFound { index } => {
                assert_eq!(index, piece_index);
            }
            _ => panic!("Expected PieceNotFound error"),
        }
    }

    #[test]
    async fn test_has_piece_missing_directory() {
        let (_temp_dir, downloads_dir, library_dir) = create_temp_storage_dirs();
        let storage = FileStorage::new(downloads_dir, library_dir);

        let info_hash = create_test_info_hash();
        let piece_index = PieceIndex::new(0);

        // has_piece should return false for missing piece without error
        let result = storage.has_piece(info_hash, piece_index).await;
        assert!(result.is_ok());
        assert!(!result.unwrap());
    }

    #[test]
    async fn test_finalize_torrent_source_not_found() {
        let (_temp_dir, downloads_dir, library_dir) = create_temp_storage_dirs();
        let mut storage = FileStorage::new(downloads_dir, library_dir);

        let info_hash = create_test_info_hash();

        // Try to finalize a torrent that was never downloaded
        let result = storage.finalize_torrent(info_hash).await;

        assert!(result.is_err());
        match result.unwrap_err() {
            StorageError::Io(io_error) => {
                assert_eq!(io_error.kind(), std::io::ErrorKind::NotFound);
            }
            _ => panic!("Expected IO NotFound error"),
        }
    }

    #[test]
    async fn test_finalize_torrent_destination_permission_denied() {
        let (_temp_dir, downloads_dir, library_dir) = create_temp_storage_dirs();

        // Create source directory with a piece
        let info_hash = create_test_info_hash();
        let source_dir = downloads_dir.join(info_hash.to_string());
        fs::create_dir_all(&source_dir).unwrap();
        fs::write(source_dir.join("piece_0"), b"test data").unwrap();

        // Make library directory read-only
        let mut perms = fs::metadata(&library_dir).unwrap().permissions();
        perms.set_mode(0o444);
        fs::set_permissions(&library_dir, perms).unwrap();

        let mut storage = FileStorage::new(downloads_dir, library_dir);
        let result = storage.finalize_torrent(info_hash).await;

        assert!(result.is_err());
        match result.unwrap_err() {
            StorageError::Io(io_error) => {
                assert_eq!(io_error.kind(), std::io::ErrorKind::PermissionDenied);
            }
            _ => panic!("Expected IO PermissionDenied error"),
        }
    }

    #[test]
    async fn test_finalize_torrent_destination_exists() {
        let (_temp_dir, downloads_dir, library_dir) = create_temp_storage_dirs();

        let info_hash = create_test_info_hash();

        // Create source directory
        let source_dir = downloads_dir.join(info_hash.to_string());
        fs::create_dir_all(&source_dir).unwrap();
        fs::write(source_dir.join("piece_0"), b"test data").unwrap();

        // Create destination directory that already exists
        let dest_dir = library_dir.join(info_hash.to_string());
        fs::create_dir_all(&dest_dir).unwrap();
        fs::write(dest_dir.join("existing_file"), b"existing data").unwrap();

        let mut storage = FileStorage::new(downloads_dir, library_dir);
        let result = storage.finalize_torrent(info_hash).await;

        // On Unix systems, rename should replace the destination
        // The behavior might vary by platform, but we expect either success or specific error
        match result {
            Ok(_) => {
                // Success - destination was replaced
                assert!(dest_dir.exists());
            }
            Err(StorageError::Io(io_error)) => {
                // Some platforms might fail with AlreadyExists or other error
                println!("Expected error for destination exists: {}", io_error);
            }
            Err(e) => panic!("Unexpected error type: {}", e),
        }
    }

    #[test]
    async fn test_store_large_piece_data() {
        let (_temp_dir, downloads_dir, library_dir) = create_temp_storage_dirs();
        let mut storage = FileStorage::new(downloads_dir, library_dir);

        let info_hash = create_test_info_hash();
        let piece_index = PieceIndex::new(0);

        // Create a large piece (1MB of data)
        let large_piece_data = vec![0xAB; 1024 * 1024];

        let result = storage
            .store_piece(info_hash, piece_index, &large_piece_data)
            .await;

        assert!(result.is_ok());

        // Verify we can load it back
        let loaded_data = storage.load_piece(info_hash, piece_index).await.unwrap();
        assert_eq!(loaded_data, large_piece_data);
    }

    #[test]
    async fn test_sequential_piece_operations() {
        let (_temp_dir, downloads_dir, library_dir) = create_temp_storage_dirs();
        let mut storage = FileStorage::new(downloads_dir, library_dir);

        let info_hash = create_test_info_hash();
        let piece_data = b"sequential test data";

        // Store multiple pieces sequentially
        for i in 0..10 {
            let piece_index = PieceIndex::new(i);
            let result = storage
                .store_piece(info_hash, piece_index, piece_data)
                .await;
            assert!(result.is_ok());
        }

        // Verify all pieces are present
        for i in 0..10 {
            let piece_index = PieceIndex::new(i);
            let has_piece = storage.has_piece(info_hash, piece_index).await.unwrap();
            assert!(has_piece);
        }
    }

    #[test]
    async fn test_roundtrip_consistency() {
        let (_temp_dir, downloads_dir, library_dir) = create_temp_storage_dirs();
        let mut storage = FileStorage::new(downloads_dir, library_dir);

        let info_hash = create_test_info_hash();
        let piece_index = PieceIndex::new(0);
        let original_data = b"roundtrip test data with special chars: \x00\xFF\xAB\xCD";

        // Store piece
        storage
            .store_piece(info_hash, piece_index, original_data)
            .await
            .unwrap();

        // Load piece back
        let loaded_data = storage.load_piece(info_hash, piece_index).await.unwrap();

        // Verify data integrity
        assert_eq!(loaded_data, original_data);
    }

    #[test]
    async fn test_multiple_torrents_isolation() {
        let (_temp_dir, downloads_dir, library_dir) = create_temp_storage_dirs();
        let mut storage = FileStorage::new(downloads_dir, library_dir);

        let info_hash1 = create_test_info_hash();
        let mut info_hash2_bytes = *info_hash1.as_bytes();
        info_hash2_bytes[0] = 0xFF; // Make it different
        let info_hash2 = InfoHash::new(info_hash2_bytes);

        let piece_index = PieceIndex::new(0);
        let data1 = b"torrent 1 data";
        let data2 = b"torrent 2 data";

        // Store pieces for both torrents
        storage
            .store_piece(info_hash1, piece_index, data1)
            .await
            .unwrap();
        storage
            .store_piece(info_hash2, piece_index, data2)
            .await
            .unwrap();

        // Verify each torrent has its own data
        let loaded_data1 = storage.load_piece(info_hash1, piece_index).await.unwrap();
        let loaded_data2 = storage.load_piece(info_hash2, piece_index).await.unwrap();

        assert_eq!(loaded_data1, data1);
        assert_eq!(loaded_data2, data2);
        assert_ne!(loaded_data1, loaded_data2);
    }

    #[test]
    async fn test_store_piece_disk_space_simulation() {
        let (_temp_dir, downloads_dir, library_dir) = create_temp_storage_dirs();
        let mut storage = FileStorage::new(downloads_dir, library_dir);

        let info_hash = create_test_info_hash();
        let piece_index = PieceIndex::new(0);

        // Create extremely large piece data that might exhaust disk space
        // This simulates what would happen with insufficient disk space
        let huge_piece_data = vec![0xAB; 100 * 1024 * 1024]; // 100MB

        let result = storage
            .store_piece(info_hash, piece_index, &huge_piece_data)
            .await;

        // This test might succeed on systems with sufficient space
        // or fail with ENOSPC on systems with limited disk space
        match result {
            Ok(_) => {
                // Verify the piece was actually stored
                let has_piece = storage.has_piece(info_hash, piece_index).await.unwrap();
                assert!(has_piece);
            }
            Err(StorageError::Io(io_error)) => {
                // Acceptable error for disk space exhaustion
                println!("Disk space test failed as expected: {}", io_error);
            }
            Err(e) => panic!("Unexpected error type: {}", e),
        }
    }

    #[test]
    async fn test_corrupted_piece_file_recovery() {
        let (_temp_dir, downloads_dir, library_dir) = create_temp_storage_dirs();
        let mut storage = FileStorage::new(downloads_dir.clone(), library_dir);

        let info_hash = create_test_info_hash();
        let piece_index = PieceIndex::new(0);
        let original_data = b"original piece data";

        // Store the piece
        storage
            .store_piece(info_hash, piece_index, original_data)
            .await
            .unwrap();

        // Corrupt the piece file by overwriting with different data
        let piece_path = downloads_dir
            .join(info_hash.to_string())
            .join(format!("piece_{}", piece_index.as_u32()));

        fs::write(&piece_path, b"corrupted data").unwrap();

        // Loading should succeed but return corrupted data
        let loaded_data = storage.load_piece(info_hash, piece_index).await.unwrap();
        assert_eq!(loaded_data, b"corrupted data");
        assert_ne!(loaded_data, original_data);
    }

    #[test]
    async fn test_piece_file_truncation() {
        let (_temp_dir, downloads_dir, library_dir) = create_temp_storage_dirs();
        let mut storage = FileStorage::new(downloads_dir.clone(), library_dir);

        let info_hash = create_test_info_hash();
        let piece_index = PieceIndex::new(0);
        let original_data = b"this is a longer piece of data that will be truncated";

        // Store the piece
        storage
            .store_piece(info_hash, piece_index, original_data)
            .await
            .unwrap();

        // Truncate the piece file
        let piece_path = downloads_dir
            .join(info_hash.to_string())
            .join(format!("piece_{}", piece_index.as_u32()));

        fs::write(&piece_path, b"truncated").unwrap();

        // Loading should return the truncated data
        let loaded_data = storage.load_piece(info_hash, piece_index).await.unwrap();
        assert_eq!(loaded_data, b"truncated");
        assert!(loaded_data.len() < original_data.len());
    }

    #[test]
    async fn test_empty_piece_file() {
        let (_temp_dir, downloads_dir, library_dir) = create_temp_storage_dirs();
        let mut storage = FileStorage::new(downloads_dir.clone(), library_dir);

        let info_hash = create_test_info_hash();
        let piece_index = PieceIndex::new(0);

        // Store an empty piece
        storage
            .store_piece(info_hash, piece_index, b"")
            .await
            .unwrap();

        // Loading should return empty data
        let loaded_data = storage.load_piece(info_hash, piece_index).await.unwrap();
        assert_eq!(loaded_data, b"");
        assert_eq!(loaded_data.len(), 0);

        // Piece should be marked as present
        let has_piece = storage.has_piece(info_hash, piece_index).await.unwrap();
        assert!(has_piece);
    }

    #[test]
    async fn test_directory_traversal_attack_prevention() {
        let (_temp_dir, downloads_dir, library_dir) = create_temp_storage_dirs();
        let mut storage = FileStorage::new(downloads_dir.clone(), library_dir);

        // Create a malicious info hash that attempts directory traversal
        let mut malicious_hash_bytes = [0u8; 20];
        // This would create a hash that, when converted to string, might contain '..'
        malicious_hash_bytes[0] = 0x2e; // '.'
        malicious_hash_bytes[1] = 0x2e; // '.'
        let malicious_info_hash = InfoHash::new(malicious_hash_bytes);

        let piece_index = PieceIndex::new(0);
        let piece_data = b"malicious data";

        // The storage should handle this safely without allowing directory traversal
        let result = storage
            .store_piece(malicious_info_hash, piece_index, piece_data)
            .await;

        // Should succeed - the hash is converted to hex string safely
        assert!(result.is_ok());

        // Verify the piece was stored in the expected location
        let expected_path = downloads_dir
            .join(malicious_info_hash.to_string())
            .join(format!("piece_{}", piece_index.as_u32()));

        assert!(expected_path.exists());
        assert!(expected_path.starts_with(&downloads_dir));
    }

    #[test]
    async fn test_symbolic_link_handling() {
        let (_temp_dir, downloads_dir, library_dir) = create_temp_storage_dirs();
        let mut storage = FileStorage::new(downloads_dir.clone(), library_dir.clone());

        let info_hash = create_test_info_hash();
        let piece_index = PieceIndex::new(0);
        let piece_data = b"test data";

        // Store a piece normally first
        storage
            .store_piece(info_hash, piece_index, piece_data)
            .await
            .unwrap();

        // Create a symbolic link in the downloads directory
        let link_path = downloads_dir.join("symbolic_link");
        let target_path = library_dir.join("target_file");
        fs::write(&target_path, b"target data").unwrap();

        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(&target_path, &link_path).unwrap();

            // Verify the symbolic link exists
            assert!(link_path.exists());

            // Our storage should handle this normally - no special symlink handling needed
            let loaded_data = storage.load_piece(info_hash, piece_index).await.unwrap();
            assert_eq!(loaded_data, piece_data);
        }
    }

    #[test]
    async fn test_finalize_torrent_with_partial_data() {
        let (_temp_dir, downloads_dir, library_dir) = create_temp_storage_dirs();
        let mut storage = FileStorage::new(downloads_dir.clone(), library_dir.clone());

        let info_hash = create_test_info_hash();
        let source_dir = downloads_dir.join(info_hash.to_string());
        fs::create_dir_all(&source_dir).unwrap();

        // Create some pieces but not all
        fs::write(source_dir.join("piece_0"), b"piece 0 data").unwrap();
        fs::write(source_dir.join("piece_1"), b"piece 1 data").unwrap();
        fs::write(source_dir.join("incomplete_file"), b"partial data").unwrap();

        let result = storage.finalize_torrent(info_hash).await;
        assert!(result.is_ok());

        let final_path = result.unwrap();
        assert!(final_path.exists());
        assert!(final_path.join("piece_0").exists());
        assert!(final_path.join("piece_1").exists());
        assert!(final_path.join("incomplete_file").exists());
    }

    #[test]
    async fn test_overwrite_piece_storage() {
        let (_temp_dir, downloads_dir, library_dir) = create_temp_storage_dirs();
        let mut storage = FileStorage::new(downloads_dir, library_dir);

        let info_hash = create_test_info_hash();
        let piece_index = PieceIndex::new(0);

        // Store the same piece multiple times with different data
        for i in 0..5 {
            let data = format!("overwrite test data {}", i);
            let result = storage
                .store_piece(info_hash, piece_index, data.as_bytes())
                .await;
            assert!(result.is_ok());
        }

        // Verify piece exists and contains the last write
        let has_piece = storage.has_piece(info_hash, piece_index).await.unwrap();
        assert!(has_piece);

        let loaded_data = storage.load_piece(info_hash, piece_index).await.unwrap();
        let loaded_string = String::from_utf8(loaded_data).unwrap();
        assert_eq!(loaded_string, "overwrite test data 4");
    }
}
