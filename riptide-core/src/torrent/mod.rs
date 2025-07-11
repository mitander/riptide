//! BitTorrent protocol implementation optimized for streaming

pub mod creation;
pub mod downloader;
pub mod enhanced_peer_connection;
pub mod enhanced_peer_manager;
pub mod error_recovery;

pub mod parsing;
pub mod peer_connection;
pub mod peer_manager;
pub mod peer_state;
pub mod piece_picker;
pub mod piece_store;
// pub mod production_peer_manager;
pub mod protocol;
pub mod streaming_upload_manager;
#[cfg(test)]
pub mod test_data;
pub mod tracker;

use std::fmt;
use std::str::FromStr;

pub use creation::{DEFAULT_PIECE_SIZE, SimulationTorrentCreator, TorrentCreator, TorrentPiece};
pub use downloader::{PieceDownloader, PieceProgress, PieceRequest, PieceStatus};
pub use enhanced_peer_connection::{EnhancedPeerConnection, PeerConnectionStats};
pub use enhanced_peer_manager::{
    EnhancedPeerManager, EnhancedPeerManagerStats, PieceRequestParams, PieceResult, Priority,
};
pub use parsing::{BencodeTorrentParser, MagnetLink, TorrentFile, TorrentMetadata, TorrentParser};
pub use peer_manager::{ConnectionStatus, PeerInfo, PeerManager, PeerMessageEvent, TcpPeerManager};
pub use peer_state::{PeerBitfield, PeerConnectionState, PendingPieceRequest};
pub use piece_picker::{
    AdaptivePiecePicker, AdaptiveStreamingPiecePicker, BufferStatus, PiecePicker, PiecePriority,
    StreamingPiecePicker,
};
pub use piece_store::PieceStore;
// pub use production_peer_manager::ProductionPeerManager;
pub use protocol::{
    BitTorrentPeerProtocol, PeerHandshake, PeerId, PeerMessage, PeerProtocol, PeerState,
};
pub use streaming_upload_manager::{StreamingUploadConfig, StreamingUploadManager, UploadRequest};
pub use tracker::{
    AnnounceRequest, AnnounceResponse, HttpTrackerClient, ScrapeRequest, ScrapeResponse,
    ScrapeStats, TrackerClient, TrackerManagement, TrackerManager,
};

pub use crate::engine::{EngineStats, TorrentEngineHandle, TorrentSession, spawn_torrent_engine};
#[cfg(any(test, feature = "test-utils"))]
pub use crate::engine::{MockPeerManager, MockTrackerManager};
use crate::storage::StorageError;

/// SHA-1 hash identifying a unique torrent.
///
/// 20-byte SHA-1 hash of the info dictionary from a torrent file.
/// Used to uniquely identify torrents across the BitTorrent network.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct InfoHash([u8; 20]);

impl InfoHash {
    /// Creates InfoHash from 20-byte SHA-1 hash.
    pub fn new(hash: [u8; 20]) -> Self {
        Self(hash)
    }

    /// Returns reference to underlying 20-byte hash.
    pub fn as_bytes(&self) -> &[u8; 20] {
        &self.0
    }

    /// Creates InfoHash from a 40-character hex string.
    ///
    /// # Errors
    /// - `TorrentError::InvalidTorrentFile` if the hex string is invalid.
    pub fn from_hex(hex_str: &str) -> Result<Self, TorrentError> {
        if hex_str.len() != 40 {
            return Err(TorrentError::InvalidTorrentFile {
                reason: format!(
                    "Invalid info hash length: expected 40, got {}",
                    hex_str.len()
                ),
            });
        }

        let mut bytes = [0u8; 20];
        for (i, chunk) in hex_str.as_bytes().chunks(2).enumerate() {
            let hex_byte_str =
                std::str::from_utf8(chunk).map_err(|_| TorrentError::InvalidTorrentFile {
                    reason: "Invalid UTF-8 in info hash hex string".to_string(),
                })?;

            let byte = u8::from_str_radix(hex_byte_str, 16).map_err(|_| {
                TorrentError::InvalidTorrentFile {
                    reason: format!("Invalid hex character in info hash: '{hex_byte_str}'"),
                }
            })?;

            bytes[i] = byte;
        }

        Ok(InfoHash::new(bytes))
    }
}

impl FromStr for InfoHash {
    type Err = TorrentError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::from_hex(s)
    }
}

impl fmt::Display for InfoHash {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for byte in &self.0 {
            write!(f, "{byte:02x}")?;
        }
        Ok(())
    }
}

/// Zero-based index of a piece within a torrent.
///
/// Torrent files are divided into pieces for downloading and verification.
/// Each piece has a sequential index starting from 0.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PieceIndex(pub u32);

impl PieceIndex {
    /// Creates PieceIndex from zero-based index.
    pub fn new(index: u32) -> Self {
        Self(index)
    }

    /// Returns the underlying piece index as u32.
    pub fn as_u32(self) -> u32 {
        self.0
    }
}

impl fmt::Display for PieceIndex {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Errors that can occur during torrent operations.
///
/// Covers all failure modes in BitTorrent protocol operations including
/// file parsing, network communication, and data verification.
#[derive(Debug, thiserror::Error)]
pub enum TorrentError {
    #[error("Failed to parse torrent file: {reason}")]
    InvalidTorrentFile { reason: String },

    #[error("Tracker connection failed: {url}")]
    TrackerConnectionFailed { url: String },

    #[error("Torrent not found on tracker: {url}")]
    TorrentNotFoundOnTracker { url: String },

    #[error("Tracker request timed out: {url}")]
    TrackerTimeout { url: String },

    #[error("Tracker server error: {url}, status: {status}")]
    TrackerServerError { url: String, status: u16 },

    #[error("Piece {index} hash mismatch")]
    PieceHashMismatch { index: PieceIndex },

    #[error("Peer connection error: {reason}")]
    PeerConnectionError { reason: String },

    #[error("Protocol error: {message}")]
    ProtocolError { message: String },

    #[error("Storage error")]
    Storage(#[from] StorageError),

    #[error("Connection limit exceeded")]
    ConnectionLimitExceeded,

    #[error("No peers available for torrent")]
    NoPeersAvailable,

    #[error("Torrent {info_hash} not found")]
    TorrentNotFound { info_hash: InfoHash },

    #[error("Bandwidth limit exceeded")]
    BandwidthLimitExceeded,

    #[error("Peer {address} is blacklisted")]
    PeerBlacklisted { address: std::net::SocketAddr },

    #[error("I/O error")]
    Io(#[from] std::io::Error),

    #[error("URL parsing error")]
    UrlParsing(#[from] url::ParseError),

    #[error("UTF-8 conversion error")]
    Utf8(#[from] std::string::FromUtf8Error),

    #[error("HTTP error")]
    Http(#[from] reqwest::Error),

    #[error("Engine has been shut down")]
    EngineShutdown,

    #[error("Invalid piece index {index}, max is {max_index}")]
    InvalidPieceIndex { index: u32, max_index: u32 },

    #[error("Duplicate torrent {info_hash}")]
    DuplicateTorrent { info_hash: InfoHash },

    #[error("Invalid magnet link: {reason}")]
    InvalidMagnetLink { reason: String },

    #[error("Invalid metadata: {reason}")]
    InvalidMetadata { reason: String },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_info_hash_formatting() {
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
    fn display_piece_index_formatting() {
        let piece = PieceIndex::new(42);
        assert_eq!(piece.to_string(), "42");
    }
}
