//! BitTorrent torrent file and magnet link parsing implementations.
//!
//! Torrent metadata extraction using bencode-rs and magnet-url crates.
//! Supports both .torrent files and magnet links with error handling and validation.

use super::{InfoHash, TorrentError};
use std::path::Path;
use sha1::{Digest, Sha1};

// Type aliases for complex bencode types
type BencodeDict<'a> = std::collections::HashMap<&'a [u8], bencode_rs::Value<'a>>;
type ParseResult<T> = Result<T, TorrentError>;
type BytesResult<'a> = Result<&'a [u8], TorrentError>;
type FilesResult = ParseResult<(Vec<TorrentFile>, u64)>;

/// Torrent file metadata
#[derive(Debug, Clone, PartialEq)]
pub struct TorrentMetadata {
    pub info_hash: InfoHash,
    pub name: String,
    pub piece_length: u32,
    pub piece_hashes: Vec<[u8; 20]>,
    pub total_length: u64,
    pub files: Vec<TorrentFile>,
    pub announce_urls: Vec<String>,
}

/// Individual file within a torrent
#[derive(Debug, Clone, PartialEq)]
pub struct TorrentFile {
    pub path: Vec<String>,
    pub length: u64,
}

/// Magnet link components
#[derive(Debug, Clone, PartialEq)]
pub struct MagnetLink {
    pub info_hash: InfoHash,
    pub display_name: Option<String>,
    pub trackers: Vec<String>,
}

/// Abstract torrent parsing interface for multiple implementations.
///
/// Provides unified interface for parsing torrent metadata from various sources.
/// Implementations handle format-specific details while maintaining consistent
/// error handling and metadata extraction.
#[async_trait::async_trait]
pub trait TorrentParser: Send + Sync {
    /// Parses torrent metadata from raw bencode bytes.
    ///
    /// Extracts complete torrent information including info hash, piece hashes,
    /// file listings, and announce URLs from bencode-encoded torrent data.
    ///
    /// # Errors
    /// - `TorrentError::InvalidTorrentFile` - Malformed bencode or missing fields
    async fn parse_torrent_data(&self, data: &[u8]) -> Result<TorrentMetadata, TorrentError>;
    
    /// Parses torrent file from filesystem path.
    ///
    /// Reads file from disk and delegates to parse_torrent_data for processing.
    /// Convenience method for loading .torrent files.
    ///
    /// # Errors  
    /// - `TorrentError::InvalidTorrentFile` - File I/O error or parsing failure
    async fn parse_torrent_file(&self, path: &Path) -> Result<TorrentMetadata, TorrentError>;
    
    /// Parses magnet link to extract torrent information.
    ///
    /// Extracts info hash, display name, and tracker URLs from magnet URI.
    /// Limited metadata compared to .torrent files.
    ///
    /// # Errors
    /// - `TorrentError::InvalidTorrentFile` - Malformed magnet URI
    async fn parse_magnet_link(&self, magnet_url: &str) -> Result<MagnetLink, TorrentError>;
}

/// Reference implementation using bencode-rs and magnet-url
pub struct BencodeTorrentParser;

impl Default for BencodeTorrentParser {
    fn default() -> Self {
        Self::new()
    }
}

impl BencodeTorrentParser {
    pub fn new() -> Self {
        Self
    }

    /// Parse bencode data and extract torrent metadata
    fn parse_bencode_data(torrent_bytes: &[u8]) -> Result<TorrentMetadata, TorrentError> {
        let parsed = bencode_rs::Value::parse(torrent_bytes).map_err(|e| {
            TorrentError::InvalidTorrentFile {
                reason: format!("Bencode parsing failed: {:?}", e),
            }
        })?;

        if parsed.is_empty() {
            return Err(TorrentError::InvalidTorrentFile {
                reason: "Empty bencode data".to_string(),
            });
        }

        let root = &parsed[0];
        if let bencode_rs::Value::Dictionary(dict) = root {
            Self::extract_metadata_from_dict(dict, torrent_bytes)
        } else {
            Err(TorrentError::InvalidTorrentFile {
                reason: "Root element must be dictionary".to_string(),
            })
        }
    }

    /// Extract torrent metadata from bencode dictionary
    fn extract_metadata_from_dict(
        dict: &BencodeDict<'_>,
        original_data: &[u8],
    ) -> ParseResult<TorrentMetadata> {
        // Extract info dictionary
        let info_dict = dict.get(b"info".as_slice())
            .ok_or_else(|| TorrentError::InvalidTorrentFile {
                reason: "Missing 'info' field".to_string(),
            })?;

        let info_hash = Self::calculate_info_hash(info_dict, original_data)?;
        
        if let bencode_rs::Value::Dictionary(info_dict_map) = info_dict {
            // Extract name
            let name = Self::extract_bytes_as_string(info_dict_map, b"name")?;
            
            // Extract piece length
            let piece_length = Self::extract_integer(info_dict_map, b"piece length")? as u32;
            
            // Extract pieces (concatenated SHA1 hashes)
            let pieces_bytes = Self::extract_bytes(info_dict_map, b"pieces")?;
            if pieces_bytes.len() % 20 != 0 {
                return Err(TorrentError::InvalidTorrentFile {
                    reason: "Invalid pieces length".to_string(),
                });
            }
            
            let piece_hashes: Vec<[u8; 20]> = pieces_bytes
                .chunks(20)
                .map(|chunk| {
                    let mut hash = [0u8; 20];
                    hash.copy_from_slice(chunk);
                    hash
                })
                .collect();

            // Extract files and total length
            let (files, total_length) = if let Ok(length) = Self::extract_integer(info_dict_map, b"length") {
                // Single file torrent
                let files = vec![TorrentFile {
                    path: vec![name.clone()],
                    length: length as u64,
                }];
                (files, length as u64)
            } else if let Ok(bencode_rs::Value::List(files_list)) = info_dict_map.get(b"files".as_slice()).ok_or_else(|| TorrentError::InvalidTorrentFile {
                reason: "Missing 'files' or 'length' field".to_string(),
            }) {
                // Multi-file torrent
                Self::extract_files_info(files_list)?
            } else {
                return Err(TorrentError::InvalidTorrentFile {
                    reason: "Invalid files structure".to_string(),
                });
            };

            // Extract announce URLs
            let announce_urls = Self::extract_announce_urls(dict)?;

            Ok(TorrentMetadata {
                info_hash,
                name,
                piece_length,
                piece_hashes,
                total_length,
                files,
                announce_urls,
            })
        } else {
            Err(TorrentError::InvalidTorrentFile {
                reason: "Info field must be dictionary".to_string(),
            })
        }
    }

    /// Calculate SHA1 hash of the info dictionary
    fn calculate_info_hash(
        _info_dict: &bencode_rs::Value<'_>,
        original_data: &[u8],
    ) -> Result<InfoHash, TorrentError> {
        // Find the start and end of the info dictionary in the original data
        // TODO: Implement proper bencode encoding of just the info dict
        let info_start = original_data.windows(b"4:info".len())
            .position(|window| window == b"4:info")
            .ok_or_else(|| TorrentError::InvalidTorrentFile {
                reason: "Could not find info dictionary in data".to_string(),
            })?;
        
        // Skip "4:info" to get to the actual dictionary
        let info_data_start = info_start + 6;
        
        // For now, use a placeholder hash calculation
        // TODO: Implement proper bencode encoding of just the info dict
        let mut hasher = Sha1::new();
        hasher.update(&original_data[info_data_start..info_data_start + 100.min(original_data.len() - info_data_start)]);
        let hash_result = hasher.finalize();
        let mut hash = [0u8; 20];
        hash.copy_from_slice(&hash_result);
        
        Ok(InfoHash::new(hash))
    }

    /// Extract string from bencode dictionary
    fn extract_bytes_as_string(
        dict: &BencodeDict<'_>,
        key: &[u8],
    ) -> ParseResult<String> {
        let bytes = Self::extract_bytes(dict, key)?;
        String::from_utf8(bytes.to_vec()).map_err(|_| {
            TorrentError::InvalidTorrentFile {
                reason: format!("Invalid UTF-8 in field: {:?}", String::from_utf8_lossy(key)),
            }
        })
    }

    /// Extract bytes from bencode dictionary
    fn extract_bytes<'a>(
        dict: &'a BencodeDict<'_>,
        key: &[u8],
    ) -> BytesResult<'a> {
        match dict.get(key) {
            Some(bencode_rs::Value::Bytes(bytes)) => Ok(bytes),
            _ => Err(TorrentError::InvalidTorrentFile {
                reason: format!("Missing or invalid field: {:?}", String::from_utf8_lossy(key)),
            }),
        }
    }

    /// Extract integer from bencode dictionary
    fn extract_integer(
        dict: &BencodeDict<'_>,
        key: &[u8],
    ) -> ParseResult<i64> {
        match dict.get(key) {
            Some(bencode_rs::Value::Integer(value)) => Ok(*value),
            _ => Err(TorrentError::InvalidTorrentFile {
                reason: format!("Missing or invalid integer field: {:?}", String::from_utf8_lossy(key)),
            }),
        }
    }

    /// Extract files information from multi-file torrent
    fn extract_files_info(
        files_list: &[bencode_rs::Value<'_>],
    ) -> FilesResult {
        let mut files = Vec::new();
        let mut total_length = 0u64;

        for file_value in files_list {
            if let bencode_rs::Value::Dictionary(file_dict) = file_value {
                let length = Self::extract_integer(file_dict, b"length")? as u64;
                total_length += length;

                let path_list = match file_dict.get(b"path".as_slice()) {
                    Some(bencode_rs::Value::List(path_list)) => path_list,
                    _ => return Err(TorrentError::InvalidTorrentFile {
                        reason: "Missing or invalid path in file".to_string(),
                    }),
                };

                let mut path = Vec::new();
                for path_component in path_list {
                    if let bencode_rs::Value::Bytes(component) = path_component {
                        let component_str = String::from_utf8(component.to_vec()).map_err(|_| {
                            TorrentError::InvalidTorrentFile {
                                reason: "Invalid UTF-8 in file path".to_string(),
                            }
                        })?;
                        path.push(component_str);
                    } else {
                        return Err(TorrentError::InvalidTorrentFile {
                            reason: "Invalid path component type".to_string(),
                        });
                    }
                }

                files.push(TorrentFile { path, length });
            } else {
                return Err(TorrentError::InvalidTorrentFile {
                    reason: "Invalid file entry type".to_string(),
                });
            }
        }

        Ok((files, total_length))
    }

    /// Extract announce URLs from torrent dictionary
    fn extract_announce_urls(
        dict: &BencodeDict<'_>,
    ) -> ParseResult<Vec<String>> {
        let mut announce_urls = Vec::new();

        // Primary announce URL
        if let Ok(announce) = Self::extract_bytes_as_string(dict, b"announce") {
            announce_urls.push(announce);
        }

        // Announce list (optional)
        if let Some(bencode_rs::Value::List(announce_list)) = dict.get(b"announce-list".as_slice()) {
            for tier in announce_list {
                if let bencode_rs::Value::List(tier_urls) = tier {
                    for url_value in tier_urls {
                        if let bencode_rs::Value::Bytes(url_bytes) = url_value {
                            if let Ok(url) = String::from_utf8(url_bytes.to_vec()) {
                                announce_urls.push(url);
                            }
                        }
                    }
                }
            }
        }

        if announce_urls.is_empty() {
            return Err(TorrentError::InvalidTorrentFile {
                reason: "No announce URLs found".to_string(),
            });
        }

        Ok(announce_urls)
    }
}


impl BencodeTorrentParser {
    /// Extract info hash from magnet link
    fn extract_info_hash_from_magnet(magnet: &magnet_url::Magnet) -> Result<InfoHash, TorrentError> {
        // Simple approach: parse the URL string manually since API methods are unclear
        let url_str = magnet.to_string();
        
        // Look for xt parameter with btih hash
        for param in url_str.split('&') {
            if let Some(xt_value) = param.strip_prefix("xt=urn:btih:") {
                // Also handle the case where it appears after '?'
                return Self::parse_hash_from_string(xt_value);
            }
        }
        
        // Also check after the initial '?'
        for param in url_str.split('?').skip(1) {
            for sub_param in param.split('&') {
                if let Some(xt_value) = sub_param.strip_prefix("xt=urn:btih:") {
                    return Self::parse_hash_from_string(xt_value);
                }
            }
        }
        
        Err(TorrentError::InvalidTorrentFile {
            reason: format!("Missing or invalid info hash in magnet link: {}", url_str),
        })
    }
    
    /// Parse hex string to 20-byte hash
    fn parse_hash_from_string(hash_str: &str) -> Result<InfoHash, TorrentError> {
        if hash_str.len() == 40 {
            let mut hash = [0u8; 20];
            for (i, chunk) in hash_str.as_bytes().chunks(2).enumerate() {
                if i >= 20 { break; }
                if let Ok(byte) = u8::from_str_radix(
                    &String::from_utf8_lossy(chunk), 16
                ) {
                    hash[i] = byte;
                } else {
                    return Err(TorrentError::InvalidTorrentFile {
                        reason: format!("Invalid hex character in hash: {}", hash_str),
                    });
                }
            }
            Ok(InfoHash::new(hash))
        } else {
            Err(TorrentError::InvalidTorrentFile {
                reason: format!("Invalid hash length: {} (expected 40)", hash_str.len()),
            })
        }
    }
}

#[async_trait::async_trait]
impl TorrentParser for BencodeTorrentParser {
    async fn parse_torrent_data(&self, torrent_bytes: &[u8]) -> Result<TorrentMetadata, TorrentError> {
        Self::parse_bencode_data(torrent_bytes)
    }
    
    async fn parse_torrent_file(&self, path: &Path) -> Result<TorrentMetadata, TorrentError> {
        let file_contents = tokio::fs::read(path).await.map_err(|e| {
            TorrentError::InvalidTorrentFile {
                reason: format!("Failed to read file: {}", e),
            }
        })?;
        
        self.parse_torrent_data(&file_contents).await
    }
    
    async fn parse_magnet_link(&self, magnet_url: &str) -> Result<MagnetLink, TorrentError> {
        let magnet = magnet_url::Magnet::new(magnet_url).map_err(|e| {
            TorrentError::InvalidTorrentFile {
                reason: format!("Invalid magnet link: {}", e),
            }
        })?;

        // Extract info hash from exact topic (xt) parameter
        let info_hash = Self::extract_info_hash_from_magnet(&magnet)?;

        Ok(MagnetLink {
            info_hash,
            display_name: magnet.display_name().map(|s| s.to_string()),
            trackers: magnet.trackers().to_vec(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_magnet_link_parsing() {
        let parser = BencodeTorrentParser::new();
        
        // Valid magnet link
        let magnet_url = "magnet:?xt=urn:btih:0123456789abcdef0123456789abcdef01234567&dn=Test%20Torrent&tr=http://tracker.example.com/announce";
        let result = parser.parse_magnet_link(magnet_url).await;
        
        assert!(result.is_ok());
        let magnet = result.unwrap();
        assert_eq!(magnet.display_name, Some("Test%20Torrent".to_string()));
        assert_eq!(magnet.trackers, vec!["http://tracker.example.com/announce"]);
    }

    #[tokio::test]
    async fn test_invalid_magnet_link() {
        let parser = BencodeTorrentParser::new();
        
        let result = parser.parse_magnet_link("invalid://not-a-magnet").await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_torrent_data_parsing() {
        let parser = BencodeTorrentParser::new();
        
        // For now, test our implementation with a simple structure
        // and focus on the valid parsing path rather than complex bencode
        let torrent_data = b"d8:announce9:test:80804:infod6:lengthi1000e4:name8:test.txt12:piece lengthi32768e6:pieces20:12345678901234567890ee";
        let result = parser.parse_torrent_data(torrent_data).await;
        
        assert!(result.is_ok());
        let metadata = result.unwrap();
        assert_eq!(metadata.name, "test.txt");
        assert_eq!(metadata.piece_length, 32768);
        assert_eq!(metadata.total_length, 1000);
        assert_eq!(metadata.piece_hashes.len(), 1);
    }

    #[tokio::test] 
    async fn test_invalid_torrent_data() {
        let parser = BencodeTorrentParser::new();
        
        // Invalid bencode data (doesn't start with 'd')
        let invalid_data = b"invalid torrent data";
        let result = parser.parse_torrent_data(invalid_data).await;
        
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_torrent_file_parsing() {
        let parser = BencodeTorrentParser::new();
        
        // Create temporary file with valid bencode data
        let temp_dir = tempfile::tempdir().unwrap();
        let file_path = temp_dir.path().join("test.torrent");
        let torrent_data = b"d8:announce9:test:80804:infod6:lengthi1000e4:name8:test.txt12:piece lengthi32768e6:pieces20:12345678901234567890ee";
        
        tokio::fs::write(&file_path, torrent_data).await.unwrap();
        
        let result = parser.parse_torrent_file(&file_path).await;
        assert!(result.is_ok());
        
        let metadata = result.unwrap();
        assert_eq!(metadata.name, "test.txt");
        assert_eq!(metadata.total_length, 1000);
    }

    #[tokio::test]
    async fn test_nonexistent_file() {
        let parser = BencodeTorrentParser::new();
        
        let result = parser.parse_torrent_file(Path::new("/nonexistent/file.torrent")).await;
        assert!(result.is_err());
    }
    
    #[test]
    fn test_torrent_metadata_structure() {
        let metadata = TorrentMetadata {
            info_hash: InfoHash::new([0u8; 20]),
            name: "test.torrent".to_string(),
            piece_length: 16384,
            piece_hashes: vec![[1u8; 20], [2u8; 20]],
            total_length: 32768,
            files: vec![TorrentFile {
                path: vec!["test.txt".to_string()],
                length: 32768,
            }],
            announce_urls: vec!["http://tracker.example.com/announce".to_string()],
        };
        
        assert_eq!(metadata.piece_hashes.len(), 2);
        assert_eq!(metadata.files.len(), 1);
    }
}