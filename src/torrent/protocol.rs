//! BitTorrent wire protocol abstractions and message types

use super::{InfoHash, PieceIndex, TorrentError};
use bytes::Bytes;
use std::net::SocketAddr;

/// BitTorrent peer identifier
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PeerId([u8; 20]);

impl PeerId {
    pub fn new(id: [u8; 20]) -> Self {
        Self(id)
    }
    
    pub fn as_bytes(&self) -> &[u8; 20] {
        &self.0
    }
    
    /// Generate random peer ID for this client
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

/// BitTorrent wire protocol messages
#[derive(Debug, Clone, PartialEq)]
pub enum PeerMessage {
    KeepAlive,
    Choke,
    Unchoke,
    Interested,
    NotInterested,
    Have { piece_index: PieceIndex },
    Bitfield { bitfield: Bytes },
    Request { piece_index: PieceIndex, offset: u32, length: u32 },
    Piece { piece_index: PieceIndex, offset: u32, data: Bytes },
    Cancel { piece_index: PieceIndex, offset: u32, length: u32 },
    Port { port: u16 },
}

/// Peer handshake information
#[derive(Debug, Clone, PartialEq)]
pub struct PeerHandshake {
    pub protocol: String,
    pub reserved: [u8; 8],
    pub info_hash: InfoHash,
    pub peer_id: PeerId,
}

impl PeerHandshake {
    /// Create handshake for BitTorrent protocol
    pub fn new(info_hash: InfoHash, peer_id: PeerId) -> Self {
        Self {
            protocol: "BitTorrent protocol".to_string(),
            reserved: [0u8; 8],
            info_hash,
            peer_id,
        }
    }
}

/// Peer connection state
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PeerState {
    Disconnected,
    Connecting,
    Handshaking,
    Connected,
    Choking,
    Downloading,
}

/// Abstract peer protocol interface for streaming optimization
#[async_trait::async_trait]
pub trait PeerProtocol: Send + Sync {
    /// Establish connection and perform handshake
    async fn connect(&mut self, addr: SocketAddr, handshake: PeerHandshake) -> Result<(), TorrentError>;
    
    /// Send message to peer
    async fn send_message(&mut self, message: PeerMessage) -> Result<(), TorrentError>;
    
    /// Receive next message from peer
    async fn receive_message(&mut self) -> Result<PeerMessage, TorrentError>;
    
    /// Get current peer state
    fn peer_state(&self) -> PeerState;
    
    /// Get peer address
    fn peer_address(&self) -> Option<SocketAddr>;
    
    /// Close connection
    async fn disconnect(&mut self) -> Result<(), TorrentError>;
}

/// Reference implementation of BitTorrent wire protocol
pub struct BitTorrentPeerProtocol {
    state: PeerState,
    peer_address: Option<SocketAddr>,
}

impl BitTorrentPeerProtocol {
    pub fn new() -> Self {
        Self {
            state: PeerState::Disconnected,
            peer_address: None,
        }
    }
}

#[async_trait::async_trait]
impl PeerProtocol for BitTorrentPeerProtocol {
    async fn connect(&mut self, addr: SocketAddr, _handshake: PeerHandshake) -> Result<(), TorrentError> {
        // TODO: Implement TCP connection and BitTorrent handshake
        self.state = PeerState::Connecting;
        self.peer_address = Some(addr);
        
        Err(TorrentError::PeerConnectionError {
            reason: "Wire protocol implementation pending".to_string(),
        })
    }
    
    async fn send_message(&mut self, _message: PeerMessage) -> Result<(), TorrentError> {
        // TODO: Serialize and send message over TCP connection
        Err(TorrentError::ProtocolError {
            message: "Message sending implementation pending".to_string(),
        })
    }
    
    async fn receive_message(&mut self) -> Result<PeerMessage, TorrentError> {
        // TODO: Read and deserialize message from TCP connection
        Err(TorrentError::ProtocolError {
            message: "Message receiving implementation pending".to_string(),
        })
    }
    
    fn peer_state(&self) -> PeerState {
        self.state
    }
    
    fn peer_address(&self) -> Option<SocketAddr> {
        self.peer_address
    }
    
    async fn disconnect(&mut self) -> Result<(), TorrentError> {
        // TODO: Close TCP connection cleanly
        self.state = PeerState::Disconnected;
        self.peer_address = None;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_peer_id_generation() {
        let peer_id = PeerId::generate();
        let bytes = peer_id.as_bytes();
        
        // Should start with Riptide client identifier
        assert_eq!(&bytes[..8], b"-RT0001-");
        
        // Should have random remaining bytes
        let peer_id2 = PeerId::generate();
        assert_ne!(peer_id.as_bytes(), peer_id2.as_bytes());
    }
    
    #[test]
    fn test_peer_handshake_creation() {
        let info_hash = InfoHash::new([1u8; 20]);
        let peer_id = PeerId::new([2u8; 20]);
        
        let handshake = PeerHandshake::new(info_hash, peer_id);
        
        assert_eq!(handshake.protocol, "BitTorrent protocol");
        assert_eq!(handshake.info_hash, info_hash);
        assert_eq!(handshake.peer_id, peer_id);
    }
    
    #[test]
    fn test_peer_message_types() {
        let messages = vec![
            PeerMessage::KeepAlive,
            PeerMessage::Choke,
            PeerMessage::Unchoke,
            PeerMessage::Interested,
            PeerMessage::NotInterested,
            PeerMessage::Have { piece_index: PieceIndex::new(5) },
            PeerMessage::Request { piece_index: PieceIndex::new(10), offset: 0, length: 16384 },
        ];
        
        assert_eq!(messages.len(), 7);
        
        // Test message equality
        assert_eq!(messages[0], PeerMessage::KeepAlive);
        assert_ne!(messages[1], messages[2]);
    }
    
    #[tokio::test]
    async fn test_peer_protocol_interface() {
        let mut protocol = BitTorrentPeerProtocol::new();
        
        assert_eq!(protocol.peer_state(), PeerState::Disconnected);
        assert_eq!(protocol.peer_address(), None);
        
        // Should handle disconnect gracefully even when not connected
        let result = protocol.disconnect().await;
        assert!(result.is_ok());
    }
}