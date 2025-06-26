//! Core types and structures for torrent parsing

use std::path::Path;

use async_trait::async_trait;

use super::super::{InfoHash, TorrentError};

/// Torrent file metadata
#[derive(Debug, Clone, PartialEq)]
/// Complete metadata extracted from a torrent file.
///
/// Contains all information needed to download a torrent including
/// piece hashes, file structure, and tracker URLs.
pub struct TorrentMetadata {
    pub info_hash: InfoHash,
    pub name: String,
    pub piece_length: u32,
    pub piece_hashes: Vec<[u8; 20]>,
    pub total_length: u64,
    pub files: Vec<TorrentFile>,
    pub announce_urls: Vec<String>,
}

/// Individual file within a torrent.
///
/// Represents a single file entry in multi-file torrents with its
/// relative path components and byte length.
#[derive(Debug, Clone, PartialEq)]
pub struct TorrentFile {
    pub path: Vec<String>,
    pub length: u64,
}

/// Magnet link components.
///
/// Parsed magnet URI containing minimal torrent metadata.
/// Contains info hash and optional display name and tracker URLs.
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
#[async_trait]
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