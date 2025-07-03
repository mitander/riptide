//! File library management for local media content

use std::collections::HashMap;
use std::path::{Path, PathBuf};

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

/// Manager for local media file library
#[derive(Debug, Default)]
pub struct FileLibraryManager {
    /// Map from info hash to media file
    files: HashMap<InfoHash, LibraryFile>,
}

impl FileLibraryManager {
    /// Create new file library manager
    pub fn new() -> Self {
        Self {
            files: HashMap::new(),
        }
    }

    /// Scan directory for media files and create library entries
    ///
    /// # Errors
    /// - `std::io::Error` - Failed to read directory or file metadata
    pub async fn scan_directory(&mut self, dir: &Path) -> Result<usize, std::io::Error> {
        self.scan_directory_recursive(dir).await
    }

    /// Recursively scan directory for media files
    fn scan_directory_recursive<'a>(
        &'a mut self,
        dir: &'a Path,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<usize, std::io::Error>> + 'a>>
    {
        Box::pin(async move {
            let mut count = 0;
            let mut entries = tokio::fs::read_dir(dir).await?;

            while let Some(entry) = entries.next_entry().await? {
                let path = entry.path();

                if path.is_dir() {
                    // Skip certain system directories
                    if let Some(dir_name) = path.file_name().and_then(|n| n.to_str())
                        && matches!(
                            dir_name,
                            ".DS_Store"
                                | "Thumbs.db"
                                | ".Trash"
                                | ".localized"
                                | ".tvlibrary"
                                | ".tvdb"
                        )
                    {
                        continue;
                    }

                    // Recursively scan subdirectory
                    match self.scan_directory_recursive(&path).await {
                        Ok(subcount) => count += subcount,
                        Err(e) => eprintln!("Warning: Failed to scan {}: {}", path.display(), e),
                    }
                } else if path.is_file() {
                    // Check if it's a video file
                    if let Some(extension) = path.extension() {
                        let ext = extension.to_string_lossy().to_lowercase();
                        if matches!(
                            ext.as_str(),
                            "mp4" | "mkv" | "avi" | "mov" | "m4v" | "webm" | "flv"
                        ) && let Ok(metadata) = entry.metadata().await
                        {
                            let file = self.create_file_from_path(path, metadata.len()).await;
                            self.files.insert(file.info_hash, file);
                            count += 1;
                        }
                    }
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
    /// - `std::io::Error` - Failed to read file
    pub async fn read_file_segment(
        &self,
        info_hash: InfoHash,
        start: u64,
        length: u64,
    ) -> Result<Vec<u8>, std::io::Error> {
        if let Some(file) = self.files.get(&info_hash) {
            use tokio::io::{AsyncReadExt, AsyncSeekExt};

            let mut file_handle = tokio::fs::File::open(&file.file_path).await?;
            file_handle.seek(std::io::SeekFrom::Start(start)).await?;

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
