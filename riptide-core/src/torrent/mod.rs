//! BitTorrent protocol implementation optimized for streaming

pub mod creation;
pub mod downloader;
pub mod enhanced_peer_connection;
pub mod enhanced_peers;
pub mod error_recovery;

pub mod parsing;
pub mod peer_connection;
pub mod peer_state;
pub mod peers;
pub mod piece_picker;
pub mod piece_store;
// pub mod production_peers;
pub mod protocol;

#[cfg(any(test, feature = "test-utils"))]
pub mod test_data;
pub mod tracker;
pub mod upload_rate_limiter;

use std::fmt;
use std::str::FromStr;

pub use creation::{DEFAULT_PIECE_SIZE, SimulationTorrentCreator, TorrentCreator, TorrentPiece};
pub use downloader::{PieceDownloader, PieceProgress, PieceRequest, PieceStatus};
pub use enhanced_peer_connection::{EnhancedPeerConnection, PeerConnectionStats};
pub use enhanced_peers::{
    EnhancedPeers, EnhancedPeersStats, PieceRequestParams, PieceResult, Priority,
};
pub use parsing::{BencodeTorrentParser, MagnetLink, TorrentFile, TorrentMetadata, TorrentParser};
pub use peer_state::{PeerBitfield, PeerConnectionState, PendingPieceRequest};
pub use peers::{ConnectionStatus, PeerInfo, PeerManager, PeerMessageEvent, TcpPeers};
pub use piece_picker::{
    AdaptivePiecePicker, AdaptiveStreamingPiecePicker, BufferStatus, PiecePicker, PiecePriority,
    StreamingPiecePicker,
};
pub use piece_store::PieceStore;
// pub use production_peers::ProductionPeers;
pub use protocol::{
    BitTorrentPeerProtocol, PeerHandshake, PeerId, PeerMessage, PeerProtocol, PeerState,
};
pub use tracker::{
    AnnounceRequest, AnnounceResponse, HttpTrackerClient, ScrapeRequest, ScrapeResponse,
    ScrapeStats, Tracker, TrackerClient, TrackerManager,
};
pub use upload_rate_limiter::{UploadRateLimitConfig, UploadRateLimitError, UploadRateLimiter};

pub use crate::engine::{EngineStats, TorrentEngineHandle, TorrentSession, spawn_torrent_engine};
#[cfg(any(test, feature = "test-utils"))]
pub use crate::engine::{MockPeers, MockTracker};
use crate::storage::StorageError;

/// SHA-1 hash identifying a unique torrent.
///
/// 20-byte SHA-1 hash of the info dictionary from a torrent file.
/// Used to uniquely identify torrents across the BitTorrent network.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Ord, PartialOrd)]
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
    ///
    /// - `TorrentError::InvalidTorrentFile` - If the hex string is invalid
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
    /// Torrent file format is invalid or corrupted.
    #[error("Failed to parse torrent file: {reason}")]
    InvalidTorrentFile {
        /// Specific reason for the parsing failure.
        reason: String,
    },

    /// Failed to establish connection with tracker.
    #[error("Tracker connection failed: {url}")]
    TrackerConnectionFailed {
        /// URL of the tracker that failed to connect.
        url: String,
    },

    /// Tracker does not have information about the requested torrent.
    #[error("Torrent not found on tracker: {url}")]
    TorrentNotFoundOnTracker {
        /// URL of the tracker that doesn't have the torrent.
        url: String,
    },

    /// Tracker request exceeded the timeout limit.
    #[error("Tracker request timed out: {url}")]
    TrackerTimeout {
        /// URL of the tracker that timed out.
        url: String,
    },

    /// Tracker responded with an HTTP error status.
    #[error("Tracker server error: {url}, status: {status}")]
    TrackerServerError {
        /// URL of the tracker that returned an error.
        url: String,
        /// HTTP status code returned by the tracker.
        status: u16,
    },

    /// Downloaded piece data does not match expected SHA-1 hash.
    #[error("Piece {index} hash mismatch")]
    PieceHashMismatch {
        /// Index of the piece with incorrect hash.
        index: PieceIndex,
    },

    /// Error occurred while establishing or maintaining peer connection.
    #[error("Peer connection error: {reason}")]
    PeerConnectionError {
        /// Specific reason for the connection failure.
        reason: String,
    },

    /// BitTorrent protocol violation or unexpected message format.
    #[error("Protocol error: {message}")]
    ProtocolError {
        /// Description of the protocol violation.
        message: String,
    },

    /// Underlying storage system error.
    #[error("Storage error")]
    Storage(#[from] StorageError),

    /// Maximum number of peer connections has been reached.
    #[error("Connection limit exceeded")]
    ConnectionLimitExceeded,

    /// No peers are available for downloading the torrent.
    #[error("No peers available for torrent")]
    NoPeersAvailable,

    /// Requested torrent session does not exist.
    #[error("Torrent {info_hash} not found")]
    TorrentNotFound {
        /// Info hash of the torrent that was not found.
        info_hash: InfoHash,
    },

    /// Upload or download bandwidth limit has been exceeded.
    #[error("Bandwidth limit exceeded")]
    BandwidthLimitExceeded,

    /// Peer has been blacklisted due to misbehavior.
    #[error("Peer {address} is blacklisted")]
    PeerBlacklisted {
        /// Socket address of the blacklisted peer.
        address: std::net::SocketAddr,
    },

    /// Underlying I/O operation failed.
    #[error("I/O error")]
    Io(#[from] std::io::Error),

    /// URL string could not be parsed.
    #[error("URL parsing error")]
    UrlParsing(#[from] url::ParseError),

    /// Byte sequence is not valid UTF-8.
    #[error("UTF-8 conversion error")]
    Utf8(#[from] std::string::FromUtf8Error),

    /// HTTP request or response processing failed.
    #[error("HTTP error")]
    Http(#[from] reqwest::Error),

    /// Torrent engine has been shut down and cannot process requests.
    #[error("Engine has been shut down")]
    EngineShutdown,

    /// Piece index is out of bounds for the torrent.
    #[error("Invalid piece index {index}, max is {max_index}")]
    InvalidPieceIndex {
        /// The invalid piece index that was requested.
        index: u32,
        /// Maximum valid piece index for this torrent.
        max_index: u32,
    },

    /// Attempted to add a torrent that already exists.
    #[error("Duplicate torrent {info_hash}")]
    DuplicateTorrent {
        /// Info hash of the duplicate torrent.
        info_hash: InfoHash,
    },

    /// Magnet link format is invalid or unsupported.
    #[error("Invalid magnet link: {reason}")]
    InvalidMagnetLink {
        /// Specific reason for the magnet link parsing failure.
        reason: String,
    },

    /// Torrent metadata is invalid or corrupted.
    #[error("Invalid metadata: {reason}")]
    InvalidMetadata {
        /// Specific reason for the metadata validation failure.
        reason: String,
    },
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
