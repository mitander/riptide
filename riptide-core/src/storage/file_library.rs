//! File library management for local media content

use std::collections::HashMap;
use std::future::Future;
use std::io::SeekFrom;
use std::path::{Path, PathBuf};
use std::pin::Pin;

use tokio::fs::DirEntry;

/// Type alias for complex async scan result future
type ScanFuture<'a> = Pin<Box<dyn Future<Output = Result<usize, std::io::Error>> + 'a>>;

use crate::torrent::InfoHash;

/// Library file entry for local media content
#[derive(Debug, Clone)]
pub struct LibraryFile {
    /// Path to the media file
    pub file_path: PathBuf,
    /// File size in bytes
    pub size: u64,
    /// Media title extracted from filename
    pub title: String,
    /// Deterministic content identifier for this file
    pub info_hash: InfoHash,
}

/// Local media file library
#[derive(Debug, Default)]
pub struct FileLibrary {
    /// Map from info hash to media file
    files: HashMap<InfoHash, LibraryFile>,
}

impl FileLibrary {
    /// Create new file library
    pub fn new() -> Self {
        Self {
            files: HashMap::new(),
        }
    }

    /// Scan directory for media files and create library entries
    ///
    /// # Errors
    ///
    /// - `std::io::Error` - If failed to read directory or file metadata
    pub async fn scan_directory(&mut self, dir: &Path) -> Result<usize, std::io::Error> {
        self.scan_directory_recursive(dir).await
    }

    /// Recursively scan directory for media files
    fn scan_directory_recursive<'a>(&'a mut self, dir: &'a Path) -> ScanFuture<'a> {
        Box::pin(async move {
            let mut count = 0;
            let mut entries = tokio::fs::read_dir(dir).await?;

            while let Some(entry) = entries.next_entry().await? {
                let path = entry.path();

                if path.is_dir() {
                    count += self.process_directory(&path).await?;
                } else if path.is_file() {
                    count += self.process_video_file(&path, &entry).await;
                }
            }

            Ok(count)
        })
    }

    /// Find file by info hash
    pub fn file_by_hash(&self, info_hash: InfoHash) -> Option<&LibraryFile> {
        self.files.get(&info_hash)
    }

    /// Get all files
    pub fn all_files(&self) -> Vec<&LibraryFile> {
        self.files.values().collect()
    }

    /// Check if directory should be skipped during scan
    fn should_skip_directory(path: &Path) -> bool {
        if let Some(dir_name) = path.file_name().and_then(|n| n.to_str()) {
            matches!(
                dir_name,
                ".DS_Store" | "Thumbs.db" | ".Trash" | ".localized" | ".tvlibrary" | ".tvdb"
            )
        } else {
            false
        }
    }

    /// Process a directory, skipping if necessary or recursively scanning
    async fn process_directory(&mut self, path: &Path) -> Result<usize, std::io::Error> {
        if Self::should_skip_directory(path) {
            return Ok(0);
        }

        // Recursively scan subdirectory
        match self.scan_directory_recursive(path).await {
            Ok(subcount) => Ok(subcount),
            Err(e) => {
                eprintln!("Warning: Failed to scan {}: {}", path.display(), e);
                Ok(0)
            }
        }
    }

    /// Process a video file and return 1 if successful, 0 otherwise
    async fn process_video_file(&mut self, path: &Path, entry: &DirEntry) -> usize {
        let Some(extension) = path.extension() else {
            return 0;
        };

        let ext = extension.to_string_lossy().to_lowercase();
        let is_video = matches!(
            ext.as_str(),
            "mp4" | "mkv" | "avi" | "mov" | "m4v" | "webm" | "flv"
        );

        if is_video && let Ok(metadata) = entry.metadata().await {
            let video_file = self
                .create_file_from_path(path.to_path_buf(), metadata.len())
                .await;
            self.files.insert(video_file.info_hash, video_file);
            return 1;
        }

        0
    }

    /// Create library entry from file path
    async fn create_file_from_path(&self, path: PathBuf, size: u64) -> LibraryFile {
        // Extract title from filename
        let title = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("Unknown File")
            .replace(['.', '_'], " ");

        // Generate deterministic info hash from file path
        let info_hash = self.generate_info_hash(&path);

        LibraryFile {
            file_path: path,
            size,
            title,
            info_hash,
        }
    }

    /// Generate deterministic info hash from file path
    fn generate_info_hash(&self, path: &Path) -> InfoHash {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        let mut hasher = DefaultHasher::new();
        path.hash(&mut hasher);
        let hash = hasher.finish();

        // Convert u64 hash to 20-byte array
        let mut hash_bytes = [0u8; 20];
        hash_bytes[0..8].copy_from_slice(&hash.to_be_bytes());
        // Fill remaining bytes with a pattern based on the hash
        for (offset, byte) in hash_bytes.iter_mut().enumerate().skip(8).take(12) {
            *byte = ((hash >> (offset % 8)) & 0xFF) as u8;
        }

        InfoHash::new(hash_bytes)
    }

    /// Read file segment for streaming
    ///
    /// # Errors
    ///
    /// - `std::io::Error` - If failed to read file
    pub async fn read_file_segment(
        &self,
        info_hash: InfoHash,
        start: u64,
        length: u64,
    ) -> Result<Vec<u8>, std::io::Error> {
        if let Some(file) = self.files.get(&info_hash) {
            use tokio::io::{AsyncReadExt, AsyncSeekExt};

            let mut file_handle = tokio::fs::File::open(&file.file_path).await?;
            file_handle.seek(SeekFrom::Start(start)).await?;

            let mut buffer = vec![0u8; length as usize];
            let bytes_read = file_handle.read(&mut buffer).await?;
            buffer.truncate(bytes_read);

            Ok(buffer)
        } else {
            Err(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "File not found",
            ))
        }
    }

    /// Update file's info hash (used when canonical content-based hash differs from path-based)
    pub fn update_file_info_hash(&mut self, old_hash: InfoHash, new_hash: InfoHash) {
        if let Some(mut file) = self.files.remove(&old_hash) {
            file.info_hash = new_hash;
            self.files.insert(new_hash, file);
        }
    }
}
