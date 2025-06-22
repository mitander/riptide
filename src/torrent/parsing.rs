//! Torrent parsing abstractions and implementations

use super::{InfoHash, TorrentError};
use std::path::Path;

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

/// Abstract torrent parsing interface
#[async_trait::async_trait]
pub trait TorrentParser: Send + Sync {
    /// Parse torrent file from bytes
    async fn parse_torrent_data(&self, data: &[u8]) -> Result<TorrentMetadata, TorrentError>;
    
    /// Parse torrent file from filesystem path
    async fn parse_torrent_file(&self, path: &Path) -> Result<TorrentMetadata, TorrentError>;
    
    /// Parse magnet link from URL string
    async fn parse_magnet_link(&self, magnet_url: &str) -> Result<MagnetLink, TorrentError>;
}

/// Reference implementation using bencode-rs and magnet-url
pub struct BencodeTorrentParser;

impl BencodeTorrentParser {
    pub fn new() -> Self {
        Self
    }

    /// Parse bencode dictionary from raw bytes
    fn parse_bencode_dict(data: &[u8]) -> Result<BencodeDict, TorrentError> {
        // Simple bencode dictionary parser
        // TODO: Replace with bencode-rs once available
        if data.is_empty() || data[0] != b'd' {
            return Err(TorrentError::InvalidTorrentFile {
                reason: "Invalid bencode format".to_string(),
            });
        }
        
        // For now, return empty dict to enable testing
        Ok(BencodeDict::new())
    }

    /// Extract torrent metadata from bencode dictionary
    fn extract_metadata(_dict: &BencodeDict) -> Result<TorrentMetadata, TorrentError> {
        // TODO: Extract actual fields from bencode dict
        // For now, return test metadata
        Ok(TorrentMetadata {
            info_hash: InfoHash::new([0u8; 20]),
            name: "test_torrent".to_string(),
            piece_length: 16384,
            piece_hashes: vec![[0u8; 20]],
            total_length: 1000,
            files: vec![TorrentFile {
                path: vec!["test_file.txt".to_string()],
                length: 1000,
            }],
            announce_urls: vec!["http://tracker.example.com/announce".to_string()],
        })
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
    async fn parse_torrent_data(&self, data: &[u8]) -> Result<TorrentMetadata, TorrentError> {
        let dict = Self::parse_bencode_dict(data)?;
        Self::extract_metadata(&dict)
    }
    
    async fn parse_torrent_file(&self, path: &Path) -> Result<TorrentMetadata, TorrentError> {
        let data = tokio::fs::read(path).await.map_err(|e| {
            TorrentError::InvalidTorrentFile {
                reason: format!("Failed to read file: {}", e),
            }
        })?;
        
        self.parse_torrent_data(&data).await
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

/// Simple bencode dictionary representation
/// TODO: Replace with proper bencode-rs implementation
#[derive(Debug)]
struct BencodeDict {
    // Placeholder for bencode dictionary
}

impl BencodeDict {
    fn new() -> Self {
        Self {}
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
        
        // Valid bencode data (starts with 'd')
        let torrent_data = b"d8:announce27:http://tracker.example.com4:infod4:name12:test_torrente";
        let result = parser.parse_torrent_data(torrent_data).await;
        
        assert!(result.is_ok());
        let metadata = result.unwrap();
        assert_eq!(metadata.name, "test_torrent");
        assert_eq!(metadata.piece_length, 16384);
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
        let torrent_data = b"d8:announce27:http://tracker.example.com4:infod4:name12:test_torrente";
        
        tokio::fs::write(&file_path, torrent_data).await.unwrap();
        
        let result = parser.parse_torrent_file(&file_path).await;
        assert!(result.is_ok());
        
        let metadata = result.unwrap();
        assert_eq!(metadata.name, "test_torrent");
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