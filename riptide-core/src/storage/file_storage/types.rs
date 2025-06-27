//! Core types and structures for file-based storage

use std::path::PathBuf;

/// File system-based storage implementation.
///
/// Stores torrent pieces as individual files in a directory structure
/// organized by torrent info hash. Handles piece verification and
/// torrent completion detection.
pub struct FileStorage {
    pub(super) download_dir: PathBuf,
    pub(super) library_dir: PathBuf,
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

#[cfg(test)]
pub(crate) fn create_test_info_hash() -> crate::torrent::InfoHash {
    crate::torrent::InfoHash::new([
        0x01, 0x23, 0x45, 0x67, 0x89, 0xab, 0xcd, 0xef, 0x01, 0x23, 0x45, 0x67, 0x89, 0xab, 0xcd,
        0xef, 0x01, 0x23, 0x45, 0x67,
    ])
}
