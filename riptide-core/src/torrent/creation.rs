//! Torrent creation from local files with piece splitting and hashing
//!
//! Converts local media files into proper torrent format with SHA-1 piece hashes
//! for use in simulation environments. Enables true content distribution simulation.

use std::io::SeekFrom;
use std::path::Path;

use sha1::{Digest, Sha1};
use tokio::fs::File;
use tokio::io::{AsyncReadExt, AsyncSeekExt};

use super::parsing::types::{TorrentFile, TorrentMetadata};
use super::{InfoHash, TorrentError};

/// Standard BitTorrent piece size (256KB)
pub const DEFAULT_PIECE_SIZE: u32 = 262_144; // 256 * 1024

/// Torrent creator for converting local files to torrent format
pub struct TorrentCreator {
    piece_size: u32,
}

impl Default for TorrentCreator {
    fn default() -> Self {
        Self::new()
    }
}

impl TorrentCreator {
    /// Creates torrent creator with default piece size (256KB)
    pub fn new() -> Self {
        Self {
            piece_size: DEFAULT_PIECE_SIZE,
        }
    }

    /// Creates torrent creator with custom piece size
    pub fn with_piece_size(piece_size: u32) -> Self {
        Self { piece_size }
    }

    /// Creates torrent metadata from a single file
    ///
    /// Splits file into pieces, calculates SHA-1 hashes, and generates
    /// complete torrent metadata ready for BitTorrent protocol use.
    ///
    /// # Errors
    /// - `TorrentError::Io` - File read error or access denied
    /// - `TorrentError::InvalidTorrentFile` - File too large or invalid
    pub async fn create_from_file(
        &self,
        file_path: &Path,
        announce_urls: Vec<String>,
    ) -> Result<TorrentMetadata, TorrentError> {
        if !file_path.exists() {
            return Err(TorrentError::InvalidTorrentFile {
                reason: format!("File does not exist: {}", file_path.display()),
            });
        }

        let mut file = File::open(file_path).await?;
        let file_size = file.metadata().await?.len();

        if file_size == 0 {
            return Err(TorrentError::InvalidTorrentFile {
                reason: "Cannot create torrent from empty file".to_string(),
            });
        }

        let file_name = file_path
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or_else(|| TorrentError::InvalidTorrentFile {
                reason: "Invalid filename".to_string(),
            })?
            .to_string();

        // Calculate piece hashes
        let piece_hashes = self.calculate_piece_hashes(&mut file, file_size).await?;

        // Create single-file torrent metadata
        let torrent_file = TorrentFile {
            path: vec![file_name.clone()],
            length: file_size,
        };

        // Generate info hash by hashing the info dictionary
        let info_hash = self.calculate_info_hash(&file_name, file_size, &piece_hashes)?;

        Ok(TorrentMetadata {
            info_hash,
            name: file_name,
            piece_length: self.piece_size,
            piece_hashes,
            total_length: file_size,
            files: vec![torrent_file],
            announce_urls,
        })
    }

    /// Creates torrent metadata from multiple files in a directory
    ///
    /// Processes all files in directory, maintaining file structure and
    /// creating continuous piece sequence across all files.
    ///
    /// # Errors
    /// - `TorrentError::Io` - Directory read error or file access issues
    /// - `TorrentError::InvalidTorrentFile` - Directory empty or invalid files
    pub async fn create_from_directory(
        &self,
        directory_path: &Path,
        announce_urls: Vec<String>,
    ) -> Result<TorrentMetadata, TorrentError> {
        if !directory_path.is_dir() {
            return Err(TorrentError::InvalidTorrentFile {
                reason: format!("Path is not a directory: {}", directory_path.display()),
            });
        }

        let files = self.collect_files_recursively(directory_path).await?;

        if files.is_empty() {
            return Err(TorrentError::InvalidTorrentFile {
                reason: "Directory contains no files".to_string(),
            });
        }

        let directory_name = directory_path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("torrent")
            .to_string();

        let total_length: u64 = files.iter().map(|f| f.length).sum();

        // Calculate piece hashes across all files
        let piece_hashes = self
            .calculate_multi_file_piece_hashes(directory_path, &files)
            .await?;

        // Generate info hash for multi-file torrent
        let info_hash =
            self.calculate_multi_file_info_hash(&directory_name, &files, &piece_hashes)?;

        Ok(TorrentMetadata {
            info_hash,
            name: directory_name,
            piece_length: self.piece_size,
            piece_hashes,
            total_length,
            files,
            announce_urls,
        })
    }

    /// Calculates SHA-1 hashes for all pieces in a single file
    async fn calculate_piece_hashes(
        &self,
        file: &mut File,
        file_size: u64,
    ) -> Result<Vec<[u8; 20]>, TorrentError> {
        let mut piece_hashes = Vec::new();
        let mut buffer = vec![0u8; self.piece_size as usize];
        let mut position = 0u64;

        file.seek(SeekFrom::Start(0)).await?;

        while position < file_size {
            let remaining = file_size - position;
            let read_size = (remaining as usize).min(self.piece_size as usize);

            // Resize buffer for final piece if needed
            if read_size < buffer.len() {
                buffer.resize(read_size, 0);
            }

            let bytes_read = file.read_exact(&mut buffer[..read_size]).await?;
            if bytes_read != read_size {
                return Err(TorrentError::InvalidTorrentFile {
                    reason: "Unexpected end of file during piece reading".to_string(),
                });
            }

            // Calculate SHA-1 hash for this piece
            let mut hasher = Sha1::new();
            hasher.update(&buffer[..read_size]);
            let hash = hasher.finalize();

            let mut hash_array = [0u8; 20];
            hash_array.copy_from_slice(&hash[..20]);
            piece_hashes.push(hash_array);

            position += read_size as u64;
        }

        Ok(piece_hashes)
    }

    /// Calculates piece hashes across multiple files
    async fn calculate_multi_file_piece_hashes(
        &self,
        base_path: &Path,
        files: &[TorrentFile],
    ) -> Result<Vec<[u8; 20]>, TorrentError> {
        let mut piece_hashes = Vec::new();
        let mut current_piece_buffer = Vec::new();
        let piece_size = self.piece_size as usize;

        for torrent_file in files {
            let file_path = base_path.join(torrent_file.path.join("/"));
            let mut file = File::open(&file_path).await?;
            let mut file_position = 0u64;

            while file_position < torrent_file.length {
                let remaining_in_file = torrent_file.length - file_position;
                let space_in_piece = piece_size - current_piece_buffer.len();
                let to_read = (remaining_in_file as usize).min(space_in_piece);

                let mut buffer = vec![0u8; to_read];
                file.read_exact(&mut buffer).await?;
                current_piece_buffer.extend_from_slice(&buffer);
                file_position += to_read as u64;

                // If piece is complete, hash it
                if current_piece_buffer.len() == piece_size
                    || (file_position == torrent_file.length
                        && torrent_file == files.last().unwrap())
                {
                    let mut hasher = Sha1::new();
                    hasher.update(&current_piece_buffer);
                    let hash = hasher.finalize();

                    let mut hash_array = [0u8; 20];
                    hash_array.copy_from_slice(&hash[..20]);
                    piece_hashes.push(hash_array);

                    current_piece_buffer.clear();
                }
            }
        }

        Ok(piece_hashes)
    }

    /// Collects all files in directory using iterative depth-first traversal
    async fn collect_files_recursively(
        &self,
        directory_path: &Path,
    ) -> Result<Vec<TorrentFile>, TorrentError> {
        let mut files = Vec::new();
        let mut dirs_to_process = vec![directory_path.to_path_buf()];

        while let Some(current_dir) = dirs_to_process.pop() {
            let mut entries = tokio::fs::read_dir(&current_dir).await?;

            while let Some(entry) = entries.next_entry().await? {
                let path = entry.path();
                let metadata = entry.metadata().await?;

                if metadata.is_file() {
                    // Skip hidden files and system files
                    if let Some(name) = path.file_name().and_then(|n| n.to_str())
                        && !name.starts_with('.')
                        && !name.starts_with('~')
                    {
                        let relative_path = path.strip_prefix(directory_path).map_err(|_| {
                            TorrentError::InvalidTorrentFile {
                                reason: "Failed to create relative path".to_string(),
                            }
                        })?;

                        let path_components: Vec<String> = relative_path
                            .components()
                            .map(|c| c.as_os_str().to_string_lossy().to_string())
                            .collect();

                        files.push(TorrentFile {
                            path: path_components,
                            length: metadata.len(),
                        });
                    }
                } else if metadata.is_dir() {
                    // Add subdirectory to processing queue
                    dirs_to_process.push(path);
                }
            }
        }

        // Sort files for deterministic ordering
        files.sort_by(|a, b| a.path.cmp(&b.path));
        Ok(files)
    }

    /// Calculates info hash for single-file torrent
    fn calculate_info_hash(
        &self,
        name: &str,
        length: u64,
        piece_hashes: &[[u8; 20]],
    ) -> Result<InfoHash, TorrentError> {
        // Construct info dictionary for hashing
        // This is simplified - real implementation would use proper bencode
        let mut info_dict = Vec::new();

        // Add fields in alphabetical order as per bencode spec
        info_dict.extend_from_slice(b"6:lengthi");
        info_dict.extend_from_slice(length.to_string().as_bytes());
        info_dict.extend_from_slice(b"e4:name");
        info_dict.extend_from_slice(name.len().to_string().as_bytes());
        info_dict.push(b':');
        info_dict.extend_from_slice(name.as_bytes());
        info_dict.extend_from_slice(b"12:piece lengthi");
        info_dict.extend_from_slice(self.piece_size.to_string().as_bytes());
        info_dict.extend_from_slice(b"e6:pieces");
        info_dict.extend_from_slice((piece_hashes.len() * 20).to_string().as_bytes());
        info_dict.push(b':');

        for hash in piece_hashes {
            info_dict.extend_from_slice(hash);
        }

        let mut hasher = Sha1::new();
        hasher.update(&info_dict);
        let hash = hasher.finalize();

        let mut hash_array = [0u8; 20];
        hash_array.copy_from_slice(&hash[..20]);
        Ok(InfoHash::new(hash_array))
    }

    /// Calculates info hash for multi-file torrent
    fn calculate_multi_file_info_hash(
        &self,
        name: &str,
        files: &[TorrentFile],
        piece_hashes: &[[u8; 20]],
    ) -> Result<InfoHash, TorrentError> {
        // Simplified multi-file info hash calculation
        let mut info_dict = Vec::new();

        // Add files list
        info_dict.extend_from_slice(b"5:filesl");
        for file in files {
            info_dict.extend_from_slice(b"d6:lengthi");
            info_dict.extend_from_slice(file.length.to_string().as_bytes());
            info_dict.extend_from_slice(b"e4:pathl");
            for component in &file.path {
                info_dict.extend_from_slice(component.len().to_string().as_bytes());
                info_dict.push(b':');
                info_dict.extend_from_slice(component.as_bytes());
            }
            info_dict.extend_from_slice(b"ee");
        }
        info_dict.push(b'e'); // End files list

        // Add other fields
        info_dict.extend_from_slice(b"4:name");
        info_dict.extend_from_slice(name.len().to_string().as_bytes());
        info_dict.push(b':');
        info_dict.extend_from_slice(name.as_bytes());
        info_dict.extend_from_slice(b"12:piece lengthi");
        info_dict.extend_from_slice(self.piece_size.to_string().as_bytes());
        info_dict.extend_from_slice(b"e6:pieces");
        info_dict.extend_from_slice((piece_hashes.len() * 20).to_string().as_bytes());
        info_dict.push(b':');

        for hash in piece_hashes {
            info_dict.extend_from_slice(hash);
        }

        let mut hasher = Sha1::new();
        hasher.update(&info_dict);
        let hash = hasher.finalize();

        let mut hash_array = [0u8; 20];
        hash_array.copy_from_slice(&hash[..20]);
        Ok(InfoHash::new(hash_array))
    }
}

/// Stores actual piece data for simulation use
#[derive(Debug, Clone)]
pub struct TorrentPiece {
    pub index: u32,
    pub hash: [u8; 20],
    pub data: Vec<u8>,
}

/// Enhanced torrent creator that stores pieces for simulation
pub struct SimulationTorrentCreator {
    creator: TorrentCreator,
    pieces: Vec<TorrentPiece>,
}

impl SimulationTorrentCreator {
    /// Creates simulation torrent creator with adaptive piece size
    pub fn new() -> Self {
        Self {
            creator: TorrentCreator::new(),
            pieces: Vec::new(),
        }
    }

    /// Creates simulation torrent creator with custom piece size
    pub fn with_piece_size(piece_size: u32) -> Self {
        Self {
            creator: TorrentCreator::with_piece_size(piece_size),
            pieces: Vec::new(),
        }
    }

    /// Creates torrent with stored pieces for simulation use
    ///
    /// Returns both torrent metadata and actual piece data for serving
    /// in simulated peer swarms.
    ///
    /// # Errors
    /// - `TorrentError::Io` - File read error
    /// - `TorrentError::InvalidTorrentFile` - Invalid file or data
    pub async fn create_with_pieces(
        &mut self,
        file_path: &Path,
        announce_urls: Vec<String>,
    ) -> Result<(TorrentMetadata, Vec<TorrentPiece>), TorrentError> {
        if !file_path.exists() {
            return Err(TorrentError::InvalidTorrentFile {
                reason: format!("File does not exist: {}", file_path.display()),
            });
        }

        let mut file = File::open(file_path).await?;
        let file_size = file.metadata().await?.len();

        // Use adaptive piece size for small files to avoid protocol issues
        if file_size <= 1024 {
            // For very small files, use the file size as piece size
            self.creator = TorrentCreator::with_piece_size(file_size as u32);
        } else if file_size <= 16384 {
            // For small files, use 16KB pieces
            self.creator = TorrentCreator::with_piece_size(16384);
        }

        // Calculate pieces with data storage
        self.pieces.clear();
        let piece_hashes = self
            .calculate_and_store_pieces(&mut file, file_size)
            .await?;

        // Create metadata using base creator
        let file_name = file_path
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or_else(|| TorrentError::InvalidTorrentFile {
                reason: "Invalid filename".to_string(),
            })?
            .to_string();

        let torrent_file = TorrentFile {
            path: vec![file_name.clone()],
            length: file_size,
        };

        let info_hash = self
            .creator
            .calculate_info_hash(&file_name, file_size, &piece_hashes)?;

        let metadata = TorrentMetadata {
            info_hash,
            name: file_name,
            piece_length: self.creator.piece_size,
            piece_hashes: piece_hashes.clone(),
            total_length: file_size,
            files: vec![torrent_file],
            announce_urls,
        };

        Ok((metadata, self.pieces.clone()))
    }

    /// Calculates piece hashes while storing piece data
    async fn calculate_and_store_pieces(
        &mut self,
        file: &mut File,
        file_size: u64,
    ) -> Result<Vec<[u8; 20]>, TorrentError> {
        let mut piece_hashes = Vec::new();
        let mut buffer = vec![0u8; self.creator.piece_size as usize];
        let mut position = 0u64;
        let mut piece_index = 0u32;

        file.seek(SeekFrom::Start(0)).await?;

        while position < file_size {
            let remaining = file_size - position;
            let read_size = (remaining as usize).min(self.creator.piece_size as usize);

            // Resize buffer for final piece if needed
            if read_size < buffer.len() {
                buffer.resize(read_size, 0);
            }

            let bytes_read = file.read_exact(&mut buffer[..read_size]).await?;
            if bytes_read != read_size {
                return Err(TorrentError::InvalidTorrentFile {
                    reason: "Unexpected end of file during piece reading".to_string(),
                });
            }

            // Calculate SHA-1 hash
            let mut hasher = Sha1::new();
            hasher.update(&buffer[..read_size]);
            let hash = hasher.finalize();

            let mut hash_array = [0u8; 20];
            hash_array.copy_from_slice(&hash[..20]);
            piece_hashes.push(hash_array);

            // Store piece data
            self.pieces.push(TorrentPiece {
                index: piece_index,
                hash: hash_array,
                data: buffer[..read_size].to_vec(),
            });

            position += read_size as u64;
            piece_index += 1;
        }

        Ok(piece_hashes)
    }

    /// Returns stored pieces for simulation use
    pub fn pieces(&self) -> &[TorrentPiece] {
        &self.pieces
    }
}

impl Default for SimulationTorrentCreator {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use tempfile::NamedTempFile;

    use super::*;

    #[tokio::test]
    async fn test_create_torrent_from_small_file() {
        // Create temporary test file
        let mut temp_file = NamedTempFile::new().unwrap();
        let test_data = b"Hello, BitTorrent! This is test data for torrent creation.";
        temp_file.write_all(test_data).unwrap();

        let creator = TorrentCreator::with_piece_size(32); // Small piece size for testing
        let announce_urls = vec!["http://tracker.example.com/announce".to_string()];

        let result = creator
            .create_from_file(temp_file.path(), announce_urls.clone())
            .await;

        assert!(result.is_ok());
        let metadata = result.unwrap();

        assert_eq!(metadata.total_length, test_data.len() as u64);
        assert_eq!(metadata.piece_length, 32);
        assert_eq!(metadata.announce_urls, announce_urls);
        assert_eq!(metadata.files.len(), 1);
        assert!(!metadata.piece_hashes.is_empty());
    }

    #[tokio::test]
    async fn test_create_torrent_multiple_pieces() {
        // Create larger test file spanning multiple pieces
        let mut temp_file = NamedTempFile::new().unwrap();
        let test_data = vec![42u8; 1000]; // 1KB of data
        temp_file.write_all(&test_data).unwrap();

        let creator = TorrentCreator::with_piece_size(256); // Multiple pieces
        let announce_urls = vec!["http://tracker.example.com/announce".to_string()];

        let result = creator
            .create_from_file(temp_file.path(), announce_urls)
            .await;

        assert!(result.is_ok());
        let metadata = result.unwrap();

        // Should have 4 pieces (256 + 256 + 256 + 232)
        assert_eq!(metadata.piece_hashes.len(), 4);
        assert_eq!(metadata.total_length, 1000);
    }

    #[tokio::test]
    async fn test_simulation_creator_stores_pieces() {
        let mut temp_file = NamedTempFile::new().unwrap();
        let test_data = b"Test data for simulation torrent creation";
        temp_file.write_all(test_data).unwrap();

        let mut creator = SimulationTorrentCreator::new();
        let announce_urls = vec!["http://tracker.example.com/announce".to_string()];

        let result = creator
            .create_with_pieces(temp_file.path(), announce_urls)
            .await;

        assert!(result.is_ok());
        let (metadata, pieces) = result.unwrap();

        assert_eq!(pieces.len(), metadata.piece_hashes.len());
        assert!(!pieces.is_empty());

        // Verify piece data matches hashes
        let piece = &pieces[0];
        let mut hasher = Sha1::new();
        hasher.update(&piece.data);
        let hash = hasher.finalize();
        assert_eq!(&piece.hash[..], &hash[..20]);
    }

    #[tokio::test]
    async fn test_nonexistent_file_error() {
        let creator = TorrentCreator::new();
        let result = creator
            .create_from_file(Path::new("/nonexistent/file.txt"), vec![])
            .await;

        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("File does not exist")
        );
    }

    #[tokio::test]
    async fn test_info_hash_consistency() {
        let mut temp_file = NamedTempFile::new().unwrap();
        let test_data = b"Consistent data for hash testing";
        temp_file.write_all(test_data).unwrap();

        let creator = TorrentCreator::new();
        let announce_urls = vec!["http://tracker.example.com/announce".to_string()];

        // Create torrent twice
        let result1 = creator
            .create_from_file(temp_file.path(), announce_urls.clone())
            .await
            .unwrap();
        let result2 = creator
            .create_from_file(temp_file.path(), announce_urls)
            .await
            .unwrap();

        // Info hashes should be identical
        assert_eq!(result1.info_hash, result2.info_hash);
    }
}
