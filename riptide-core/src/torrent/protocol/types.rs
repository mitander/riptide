//! Core types and enumerations for BitTorrent wire protocol

use std::net::SocketAddr;

use async_trait::async_trait;
use bytes::Bytes;

use crate::torrent::{InfoHash, PieceIndex, TorrentError};

/// BitTorrent peer identifier.
///
/// 20-byte identifier for peers in the BitTorrent network.
/// Used in handshakes and tracker communication to identify clients.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PeerId([u8; 20]);

impl PeerId {
    /// Creates peer ID from 20-byte array.
    pub fn new(id: [u8; 20]) -> Self {
        Self(id)
    }

    /// Returns peer ID as byte array reference.
    pub fn as_bytes(&self) -> &[u8; 20] {
        &self.0
    }

    /// Generate random peer ID for this client.
    pub fn generate() -> Self {
        let mut id = [0u8; 20];
        // Use Riptide client identifier prefix
        id[..8].copy_from_slice(b"-RT0001-");
        // Fill remaining with random bytes
        for byte in &mut id[8..] {
            *byte = rand::random();
        }
        Self(id)
    }
}

/// BitTorrent wire protocol messages.
///
/// Complete set of message types defined in BEP 3 for peer communication.
/// Handles keep-alive, choke/unchoke, piece requests, and data transfer.
#[derive(Debug, Clone, PartialEq)]
pub enum PeerMessage {
    /// Keep-alive message to maintain connection
    KeepAlive,
    /// Inform peer that we are choking them
    Choke,
    /// Inform peer that we are no longer choking them
    Unchoke,
    /// Inform peer that we are interested in their pieces
    Interested,
    /// Inform peer that we are not interested in their pieces
    NotInterested,
    /// Inform peer that we have acquired a specific piece
    Have {
        /// Index of the piece we now have
        piece_index: PieceIndex,
    },
    /// Send our complete piece availability bitmap
    Bitfield {
        /// Bitmap indicating which pieces we have
        bitfield: Bytes,
    },
    /// Request a block of data from a piece
    Request {
        /// Index of the piece to request from
        piece_index: PieceIndex,
        /// Byte offset within the piece
        offset: u32,
        /// Number of bytes to request
        length: u32,
    },
    /// Send a block of piece data
    Piece {
        /// Index of the piece this data belongs to
        piece_index: PieceIndex,
        /// Byte offset within the piece
        offset: u32,
        /// The actual piece data
        data: Bytes,
    },
    /// Cancel a previously sent request
    Cancel {
        /// Index of the piece to cancel
        piece_index: PieceIndex,
        /// Byte offset within the piece
        offset: u32,
        /// Number of bytes that were requested
        length: u32,
    },
    /// Inform peer of our DHT port
    Port {
        /// UDP port for DHT communication
        port: u16,
    },
}

/// Peer handshake information.
///
/// Initial exchange between peers to establish protocol compatibility
/// and verify info hash matching for torrent verification.
#[derive(Debug, Clone, PartialEq)]
pub struct PeerHandshake {
    /// Protocol identifier string ("BitTorrent protocol")
    pub protocol: String,
    /// Reserved bytes for protocol extensions
    pub reserved: [u8; 8],
    /// Info hash of the torrent being shared
    pub info_hash: InfoHash,
    /// Unique identifier for the peer
    pub peer_id: PeerId,
}

impl PeerHandshake {
    /// Create handshake for BitTorrent protocol.
    pub fn new(info_hash: InfoHash, peer_id: PeerId) -> Self {
        Self {
            protocol: "BitTorrent protocol".to_string(),
            reserved: [0u8; 8],
            info_hash,
            peer_id,
        }
    }
}

/// Peer connection state.
///
/// Tracks connection lifecycle from initial disconnect through handshake
/// to active downloading. Used for connection management and protocol flow.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PeerState {
    /// No connection established
    #[default]
    Disconnected,
    /// TCP connection in progress
    Connecting,
    /// Performing BitTorrent handshake
    Handshaking,
    /// Connected but not transferring data
    Connected,
    /// Connected but being choked by peer
    Choking,
    /// Actively downloading pieces
    Downloading,
}

/// Abstract peer protocol interface for BitTorrent communication.
///
/// Defines wire protocol operations for connecting to peers, exchanging messages,
/// and managing connection state. Implementations handle TCP socket management
/// and protocol-specific encoding/decoding.
#[async_trait]
pub trait PeerProtocol: Send + Sync {
    /// Establishes TCP connection and performs BitTorrent handshake.
    ///
    /// Connects to peer address and exchanges handshake messages to verify
    /// protocol compatibility and info hash matching.
    ///
    /// # Errors
    ///
    /// - `TorrentError::PeerConnectionError` - If TCP connection failed
    /// - `TorrentError::ProtocolError` - If handshake validation failed
    async fn connect(
        &mut self,
        address: SocketAddr,
        handshake: PeerHandshake,
    ) -> Result<(), TorrentError>;

    /// Sends wire protocol message to connected peer.
    ///
    /// # Errors
    ///
    /// - `TorrentError::PeerConnectionError` - If connection lost or write failed
    /// - `TorrentError::ProtocolError` - If message encoding failed
    async fn send_message(&mut self, message: PeerMessage) -> Result<(), TorrentError>;

    /// Receives next wire protocol message from peer.
    ///
    /// Blocks until complete message received or connection fails.
    ///
    /// # Errors
    ///
    /// - `TorrentError::PeerConnectionError` - If connection lost or read failed
    /// - `TorrentError::ProtocolError` - If message decoding failed
    async fn receive_message(&mut self) -> Result<PeerMessage, TorrentError>;

    /// Returns current connection state.
    fn peer_state(&self) -> PeerState;

    /// Returns peer socket address if connected.
    fn peer_address(&self) -> Option<SocketAddr>;

    /// Closes connection gracefully.
    ///
    /// # Errors
    ///
    /// - `TorrentError::PeerConnectionError` - If error during shutdown
    async fn disconnect(&mut self) -> Result<(), TorrentError>;
}
