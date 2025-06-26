//! Main torrent parser implementation

use std::path::Path;

use async_trait::async_trait;

use super::super::TorrentError;
use super::bencode::BencodeParser;
use super::magnet::MagnetParser;
use super::types::{MagnetLink, TorrentMetadata, TorrentParser};

/// Reference implementation using bencode-rs and magnet-url.
///
/// Production-ready parser for .torrent files and magnet links using
/// established Rust crates for bencode and magnet URI parsing.
#[derive(Default)]
pub struct BencodeTorrentParser;

impl BencodeTorrentParser {
    /// Creates new bencode parser instance.
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl TorrentParser for BencodeTorrentParser {
    async fn parse_torrent_data(
        &self,
        torrent_bytes: &[u8],
    ) -> Result<TorrentMetadata, TorrentError> {
        BencodeParser::parse_bencode_data(torrent_bytes)
    }

    async fn parse_torrent_file(&self, path: &Path) -> Result<TorrentMetadata, TorrentError> {
        let file_contents = tokio::fs::read(path).await?;

        self.parse_torrent_data(&file_contents).await
    }

    async fn parse_magnet_link(&self, magnet_url: &str) -> Result<MagnetLink, TorrentError> {
        MagnetParser::parse_magnet_link(magnet_url)
    }
}