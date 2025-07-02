//! Local movie file simulation for development testing

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::torrent::InfoHash;

/// Local movie file for simulation
#[derive(Debug, Clone)]
pub struct LocalMovie {
    /// Path to the movie file
    pub file_path: PathBuf,
    /// File size in bytes
    pub size: u64,
    /// Movie title extracted from filename
    pub title: String,
    /// Simulated info hash for this movie
    pub info_hash: InfoHash,
}

/// Manager for local movie files used in demo mode
#[derive(Debug, Default)]
pub struct LocalMovieManager {
    /// Map from info hash to movie file
    movies: HashMap<InfoHash, LocalMovie>,
}

impl LocalMovieManager {
    /// Create new movie manager
    pub fn new() -> Self {
        Self {
            movies: HashMap::new(),
        }
    }

    /// Scan directory for movie files and create simulated torrents
    ///
    /// # Errors
    /// - `std::io::Error` - Failed to read directory or file metadata
    pub async fn scan_directory(&mut self, dir: &Path) -> Result<usize, std::io::Error> {
        self.scan_directory_recursive(dir).await
    }

    /// Recursively scan directory for movie files
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
                            let movie = self.create_movie_from_file(path, metadata.len()).await;
                            self.movies.insert(movie.info_hash, movie);
                            count += 1;
                        }
                    }
                }
            }

            Ok(count)
        })
    }

    /// Get movie by info hash
    pub fn get_movie(&self, info_hash: InfoHash) -> Option<&LocalMovie> {
        self.movies.get(&info_hash)
    }

    /// Get all movies
    pub fn all_movies(&self) -> Vec<&LocalMovie> {
        self.movies.values().collect()
    }

    /// Create movie entry from file path
    async fn create_movie_from_file(&self, path: PathBuf, size: u64) -> LocalMovie {
        // Extract title from filename
        let title = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("Unknown Movie")
            .replace(['.', '_'], " ");

        // Generate deterministic info hash from file path
        let info_hash = self.generate_info_hash(&path);

        LocalMovie {
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
        if let Some(movie) = self.movies.get(&info_hash) {
            use tokio::io::{AsyncReadExt, AsyncSeekExt};

            let mut file = tokio::fs::File::open(&movie.file_path).await?;
            file.seek(std::io::SeekFrom::Start(start)).await?;

            let mut buffer = vec![0u8; length as usize];
            let bytes_read = file.read(&mut buffer).await?;
            buffer.truncate(bytes_read);

            Ok(buffer)
        } else {
            Err(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "Movie not found",
            ))
        }
    }

    /// Update movie's info hash (used when canonical content-based hash differs from path-based)
    pub fn update_movie_info_hash(&mut self, old_hash: InfoHash, new_hash: InfoHash) {
        if let Some(mut movie) = self.movies.remove(&old_hash) {
            movie.info_hash = new_hash;
            self.movies.insert(new_hash, movie);
        }
    }
}
