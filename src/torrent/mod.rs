//! BitTorrent protocol implementation optimized for streaming

pub mod downloader;
pub mod engine;
pub mod parsing;
pub mod peer_connection;
pub mod piece_picker;
pub mod protocol;
#[cfg(test)]
pub mod test_data;
pub mod tracker;

pub use downloader::{PieceDownloader, PieceProgress, PieceRequest, PieceStatus};
pub use engine::TorrentEngine;
pub use parsing::{BencodeTorrentParser, MagnetLink, TorrentMetadata, TorrentParser};
pub use piece_picker::{PiecePicker, StreamingPiecePicker};
pub use protocol::{BitTorrentPeerProtocol, PeerId, PeerMessage, PeerProtocol};
pub use tracker::{AnnounceRequest, AnnounceResponse, HttpTrackerClient, TrackerClient};

use std::fmt;

/// Context identifier for torrent operations
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct InfoHash([u8; 20]);

impl InfoHash {
    pub fn new(hash: [u8; 20]) -> Self {
        Self(hash)
    }

    pub fn as_bytes(&self) -> &[u8; 20] {
        &self.0
    }
}

impl fmt::Display for InfoHash {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for byte in &self.0 {
            write!(f, "{:02x}", byte)?;
        }
        Ok(())
    }
}

/// Index of a piece within a torrent
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PieceIndex(pub u32);

impl PieceIndex {
    pub fn new(index: u32) -> Self {
        Self(index)
    }

    pub fn as_u32(self) -> u32 {
        self.0
    }
}

impl fmt::Display for PieceIndex {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Torrent-related errors
#[derive(Debug, thiserror::Error)]
pub enum TorrentError {
    #[error("Failed to parse torrent file: {reason}")]
    InvalidTorrentFile { reason: String },

    #[error("Tracker connection failed: {url}")]
    TrackerConnectionFailed { url: String },

    #[error("Piece {index} hash mismatch")]
    PieceHashMismatch { index: PieceIndex },

    #[error("Peer connection error: {reason}")]
    PeerConnectionError { reason: String },

    #[error("Protocol error: {message}")]
    ProtocolError { message: String },

    #[error("Storage error")]
    Storage(#[from] crate::storage::StorageError),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_info_hash_display() {
        let hash = [
            0x01, 0x23, 0x45, 0x67, 0x89, 0xab, 0xcd, 0xef, 0x01, 0x23, 0x45, 0x67, 0x89, 0xab,
            0xcd, 0xef, 0x01, 0x23, 0x45, 0x67,
        ];
        let info_hash = InfoHash::new(hash);
        assert_eq!(
            info_hash.to_string(),
            "0123456789abcdef0123456789abcdef01234567"
        );
    }

    #[test]
    fn test_piece_index_ordering() {
        let piece1 = PieceIndex::new(5);
        let piece2 = PieceIndex::new(10);
        assert!(piece1 < piece2);
        assert_eq!(piece1.as_u32(), 5);
    }

    #[test]
    fn test_piece_index_display() {
        let piece = PieceIndex::new(42);
        assert_eq!(piece.to_string(), "42");
    }
}
