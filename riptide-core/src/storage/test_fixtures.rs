//! Test fixtures for storage testing.
//!
//! Provides standardized storage setup and teardown for consistent
//! testing across storage-related modules.

// Type alias for complex return type
type TempStorageDirs = (tempfile::TempDir, std::path::PathBuf, std::path::PathBuf);

/// Creates temporary directory structure for test storage operations.
///
/// # Panics
///
/// Panics if temporary directory creation fails or if subdirectories cannot be created.
/// This is acceptable in test fixtures where failures indicate environment issues.
pub fn create_temp_storage_dirs() -> TempStorageDirs {
    let temp_dir = tempfile::tempdir().unwrap();
    let downloads_dir = temp_dir.path().join("downloads");
    let library_dir = temp_dir.path().join("library");

    std::fs::create_dir_all(&downloads_dir).unwrap();
    std::fs::create_dir_all(&library_dir).unwrap();

    (temp_dir, downloads_dir, library_dir)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_temp_storage_dirs() {
        let (_temp_dir, downloads, library) = create_temp_storage_dirs();

        assert!(downloads.exists());
        assert!(library.exists());
        assert!(downloads.is_dir());
        assert!(library.is_dir());
    }
}
