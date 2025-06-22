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
}

#[async_trait::async_trait]
impl TorrentParser for BencodeTorrentParser {
    async fn parse_torrent_data(&self, _data: &[u8]) -> Result<TorrentMetadata, TorrentError> {
        // TODO: Implement using bencode-rs
        Err(TorrentError::ProtocolError {
            message: "Torrent parsing implementation pending".to_string(),
        })
    }
    
    async fn parse_torrent_file(&self, _path: &Path) -> Result<TorrentMetadata, TorrentError> {
        // TODO: Read file and delegate to parse_torrent_data
        Err(TorrentError::ProtocolError {
            message: "File parsing implementation pending".to_string(),
        })
    }
    
    async fn parse_magnet_link(&self, _magnet_url: &str) -> Result<MagnetLink, TorrentError> {
        // TODO: Implement using magnet-url crate
        Err(TorrentError::ProtocolError {
            message: "Magnet parsing implementation pending".to_string(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_torrent_parser_interface() {
        let parser = BencodeTorrentParser::new();
        
        // Interface should be callable even with stub implementation
        let result = parser.parse_magnet_link("magnet:?xt=urn:btih:test").await;
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