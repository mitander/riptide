//! BitTorrent protocol implementation optimized for streaming

pub mod engine;
pub mod piece_picker;
pub mod peer_connection;

pub use engine::TorrentEngine;
pub use piece_picker::{PiecePicker, StreamingPiecePicker};

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
}