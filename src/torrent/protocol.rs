//! BitTorrent wire protocol abstractions and message types.
//!
//! BitTorrent peer-to-peer protocol implementation following BEP 3.
//! Defines message types, handshake procedures, and connection state management
//! for communicating with remote peers.

use super::{InfoHash, PieceIndex, TorrentError};
use async_trait::async_trait;
use bytes::{Buf, BufMut, Bytes};
use std::net::SocketAddr;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

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
    Have {
        piece_index: PieceIndex,
    },
    Bitfield {
        bitfield: Bytes,
    },
    Request {
        piece_index: PieceIndex,
        offset: u32,
        length: u32,
    },
    Piece {
        piece_index: PieceIndex,
        offset: u32,
        data: Bytes,
    },
    Cancel {
        piece_index: PieceIndex,
        offset: u32,
        length: u32,
    },
    Port {
        port: u16,
    },
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PeerState {
    #[default]
    Disconnected,
    Connecting,
    Handshaking,
    Connected,
    Choking,
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
    /// - `TorrentError::PeerConnectionError` - TCP connection failed
    /// - `TorrentError::ProtocolError` - Handshake validation failed
    async fn connect(
        &mut self,
        addr: SocketAddr,
        handshake: PeerHandshake,
    ) -> Result<(), TorrentError>;

    /// Sends wire protocol message to connected peer.
    ///
    /// # Errors
    /// - `TorrentError::PeerConnectionError` - Connection lost or write failed
    /// - `TorrentError::ProtocolError` - Message encoding failed
    async fn send_message(&mut self, message: PeerMessage) -> Result<(), TorrentError>;

    /// Receives next wire protocol message from peer.
    ///
    /// Blocks until complete message received or connection fails.
    ///
    /// # Errors
    /// - `TorrentError::PeerConnectionError` - Connection lost or read failed
    /// - `TorrentError::ProtocolError` - Message decoding failed
    async fn receive_message(&mut self) -> Result<PeerMessage, TorrentError>;

    /// Returns current connection state.
    fn peer_state(&self) -> PeerState;

    /// Returns peer socket address if connected.
    fn peer_address(&self) -> Option<SocketAddr>;

    /// Closes connection gracefully.
    ///
    /// # Errors
    /// - `TorrentError::PeerConnectionError` - Error during shutdown
    async fn disconnect(&mut self) -> Result<(), TorrentError>;
}

/// Reference implementation of BitTorrent wire protocol
#[derive(Default)]
pub struct BitTorrentPeerProtocol {
    state: PeerState,
    peer_address: Option<SocketAddr>,
    stream: Option<TcpStream>,
}

impl BitTorrentPeerProtocol {
    pub fn new() -> Self {
        Self {
            state: PeerState::Disconnected,
            peer_address: None,
            stream: None,
        }
    }

    /// Serializes handshake message following BEP 3
    fn serialize_handshake(handshake: &PeerHandshake) -> Vec<u8> {
        let mut buf = Vec::new();

        // Protocol name length
        buf.push(handshake.protocol.len() as u8);

        // Protocol name
        buf.extend_from_slice(handshake.protocol.as_bytes());

        // Reserved bytes
        buf.extend_from_slice(&handshake.reserved);

        // Info hash
        buf.extend_from_slice(handshake.info_hash.as_bytes());

        // Peer ID
        buf.extend_from_slice(handshake.peer_id.as_bytes());

        buf
    }

    /// Deserializes handshake message following BEP 3
    fn deserialize_handshake(data: &[u8]) -> Result<PeerHandshake, TorrentError> {
        if data.len() < 49 {
            return Err(TorrentError::ProtocolError {
                message: "Handshake too short".to_string(),
            });
        }

        let protocol_len = data[0] as usize;
        if data.len() < 1 + protocol_len + 8 + 20 + 20 {
            return Err(TorrentError::ProtocolError {
                message: "Invalid handshake length".to_string(),
            });
        }

        let protocol = String::from_utf8_lossy(&data[1..1 + protocol_len]).to_string();

        let mut reserved = [0u8; 8];
        reserved.copy_from_slice(&data[1 + protocol_len..1 + protocol_len + 8]);

        let mut info_hash_bytes = [0u8; 20];
        info_hash_bytes.copy_from_slice(&data[1 + protocol_len + 8..1 + protocol_len + 8 + 20]);
        let info_hash = InfoHash::new(info_hash_bytes);

        let mut peer_id_bytes = [0u8; 20];
        peer_id_bytes
            .copy_from_slice(&data[1 + protocol_len + 8 + 20..1 + protocol_len + 8 + 20 + 20]);
        let peer_id = PeerId::new(peer_id_bytes);

        Ok(PeerHandshake {
            protocol,
            reserved,
            info_hash,
            peer_id,
        })
    }

    /// Serializes peer message following BEP 3
    fn serialize_message(message: &PeerMessage) -> Vec<u8> {
        let mut buf = Vec::new();

        match message {
            PeerMessage::KeepAlive => {
                buf.put_u32(0); // Length = 0
            }
            PeerMessage::Choke => {
                buf.put_u32(1); // Length = 1
                buf.put_u8(0); // Message ID
            }
            PeerMessage::Unchoke => {
                buf.put_u32(1);
                buf.put_u8(1);
            }
            PeerMessage::Interested => {
                buf.put_u32(1);
                buf.put_u8(2);
            }
            PeerMessage::NotInterested => {
                buf.put_u32(1);
                buf.put_u8(3);
            }
            PeerMessage::Have { piece_index } => {
                buf.put_u32(5); // Length = 1 + 4
                buf.put_u8(4); // Message ID
                buf.put_u32(piece_index.as_u32());
            }
            PeerMessage::Bitfield { bitfield } => {
                buf.put_u32(1 + bitfield.len() as u32);
                buf.put_u8(5);
                buf.extend_from_slice(bitfield);
            }
            PeerMessage::Request {
                piece_index,
                offset,
                length,
            } => {
                buf.put_u32(13); // Length = 1 + 4 + 4 + 4
                buf.put_u8(6);
                buf.put_u32(piece_index.as_u32());
                buf.put_u32(*offset);
                buf.put_u32(*length);
            }
            PeerMessage::Piece {
                piece_index,
                offset,
                data,
            } => {
                buf.put_u32(9 + data.len() as u32); // Length = 1 + 4 + 4 + data.len()
                buf.put_u8(7);
                buf.put_u32(piece_index.as_u32());
                buf.put_u32(*offset);
                buf.extend_from_slice(data);
            }
            PeerMessage::Cancel {
                piece_index,
                offset,
                length,
            } => {
                buf.put_u32(13);
                buf.put_u8(8);
                buf.put_u32(piece_index.as_u32());
                buf.put_u32(*offset);
                buf.put_u32(*length);
            }
            PeerMessage::Port { port } => {
                buf.put_u32(3); // Length = 1 + 2
                buf.put_u8(9);
                buf.put_u16(*port);
            }
        }

        buf
    }

    /// Deserializes peer message following BEP 3
    fn deserialize_message(data: &[u8]) -> Result<PeerMessage, TorrentError> {
        if data.len() < 4 {
            return Err(TorrentError::ProtocolError {
                message: "Message too short".to_string(),
            });
        }

        let mut buf = data;
        let length = buf.get_u32();

        if length == 0 {
            return Ok(PeerMessage::KeepAlive);
        }

        if data.len() < 4 + length as usize {
            return Err(TorrentError::ProtocolError {
                message: "Incomplete message".to_string(),
            });
        }

        let message_id = buf.get_u8();

        match message_id {
            0 => Ok(PeerMessage::Choke),
            1 => Ok(PeerMessage::Unchoke),
            2 => Ok(PeerMessage::Interested),
            3 => Ok(PeerMessage::NotInterested),
            4 => {
                if length != 5 {
                    return Err(TorrentError::ProtocolError {
                        message: "Invalid Have message length".to_string(),
                    });
                }
                let piece_index = PieceIndex::new(buf.get_u32());
                Ok(PeerMessage::Have { piece_index })
            }
            5 => {
                let bitfield_len = length - 1;
                let bitfield = Bytes::copy_from_slice(&buf[..bitfield_len as usize]);
                Ok(PeerMessage::Bitfield { bitfield })
            }
            6 => {
                if length != 13 {
                    return Err(TorrentError::ProtocolError {
                        message: "Invalid Request message length".to_string(),
                    });
                }
                let piece_index = PieceIndex::new(buf.get_u32());
                let offset = buf.get_u32();
                let length = buf.get_u32();
                Ok(PeerMessage::Request {
                    piece_index,
                    offset,
                    length,
                })
            }
            7 => {
                if length < 9 {
                    return Err(TorrentError::ProtocolError {
                        message: "Invalid Piece message length".to_string(),
                    });
                }
                let piece_index = PieceIndex::new(buf.get_u32());
                let offset = buf.get_u32();
                let data_len = length - 9;
                let data = Bytes::copy_from_slice(&buf[..data_len as usize]);
                Ok(PeerMessage::Piece {
                    piece_index,
                    offset,
                    data,
                })
            }
            8 => {
                if length != 13 {
                    return Err(TorrentError::ProtocolError {
                        message: "Invalid Cancel message length".to_string(),
                    });
                }
                let piece_index = PieceIndex::new(buf.get_u32());
                let offset = buf.get_u32();
                let length = buf.get_u32();
                Ok(PeerMessage::Cancel {
                    piece_index,
                    offset,
                    length,
                })
            }
            9 => {
                if length != 3 {
                    return Err(TorrentError::ProtocolError {
                        message: "Invalid Port message length".to_string(),
                    });
                }
                let port = buf.get_u16();
                Ok(PeerMessage::Port { port })
            }
            _ => Err(TorrentError::ProtocolError {
                message: format!("Unknown message ID: {}", message_id),
            }),
        }
    }
}

#[async_trait]
impl PeerProtocol for BitTorrentPeerProtocol {
    async fn connect(
        &mut self,
        addr: SocketAddr,
        handshake: PeerHandshake,
    ) -> Result<(), TorrentError> {
        self.state = PeerState::Connecting;
        self.peer_address = Some(addr);

        // Establish TCP connection
        let stream = TcpStream::connect(addr).await.map_err(|e| {
            self.state = PeerState::Disconnected;
            self.peer_address = None;
            TorrentError::PeerConnectionError {
                reason: format!("Failed to connect to {}: {}", addr, e),
            }
        })?;

        self.stream = Some(stream);
        self.state = PeerState::Handshaking;

        // Send handshake
        let handshake_data = Self::serialize_handshake(&handshake);
        if let Some(ref mut stream) = self.stream {
            stream.write_all(&handshake_data).await.map_err(|e| {
                self.state = PeerState::Disconnected;
                TorrentError::PeerConnectionError {
                    reason: format!("Failed to send handshake: {}", e),
                }
            })?;
        }

        // Read peer handshake response
        let mut handshake_buffer = vec![0u8; 68]; // 1 + 19 + 8 + 20 + 20
        if let Some(ref mut stream) = self.stream {
            stream
                .read_exact(&mut handshake_buffer)
                .await
                .map_err(|e| {
                    self.state = PeerState::Disconnected;
                    TorrentError::PeerConnectionError {
                        reason: format!("Failed to read handshake response: {}", e),
                    }
                })?;
        }

        // Validate peer handshake
        let peer_handshake = Self::deserialize_handshake(&handshake_buffer)?;
        if peer_handshake.info_hash != handshake.info_hash {
            self.state = PeerState::Disconnected;
            return Err(TorrentError::ProtocolError {
                message: "Info hash mismatch in handshake".to_string(),
            });
        }

        self.state = PeerState::Connected;
        Ok(())
    }

    async fn send_message(&mut self, message: PeerMessage) -> Result<(), TorrentError> {
        if self.state == PeerState::Disconnected || self.stream.is_none() {
            return Err(TorrentError::PeerConnectionError {
                reason: "Not connected to peer".to_string(),
            });
        }

        let message_data = Self::serialize_message(&message);
        if let Some(ref mut stream) = self.stream {
            stream.write_all(&message_data).await.map_err(|e| {
                self.state = PeerState::Disconnected;
                TorrentError::PeerConnectionError {
                    reason: format!("Failed to send message: {}", e),
                }
            })?;
        }

        Ok(())
    }

    async fn receive_message(&mut self) -> Result<PeerMessage, TorrentError> {
        if self.state == PeerState::Disconnected || self.stream.is_none() {
            return Err(TorrentError::PeerConnectionError {
                reason: "Not connected to peer".to_string(),
            });
        }

        // Read message length (first 4 bytes)
        let mut length_buf = [0u8; 4];
        if let Some(ref mut stream) = self.stream {
            stream.read_exact(&mut length_buf).await.map_err(|e| {
                self.state = PeerState::Disconnected;
                TorrentError::PeerConnectionError {
                    reason: format!("Failed to read message length: {}", e),
                }
            })?;
        }

        let length = u32::from_be_bytes(length_buf);

        // Handle keep-alive (length = 0)
        if length == 0 {
            return Ok(PeerMessage::KeepAlive);
        }

        // Read message payload
        let mut message_buf = vec![0u8; length as usize];
        if let Some(ref mut stream) = self.stream {
            stream.read_exact(&mut message_buf).await.map_err(|e| {
                self.state = PeerState::Disconnected;
                TorrentError::PeerConnectionError {
                    reason: format!("Failed to read message payload: {}", e),
                }
            })?;
        }

        // Reconstruct full message for parsing (length + payload)
        let mut full_message = Vec::with_capacity(4 + length as usize);
        full_message.extend_from_slice(&length_buf);
        full_message.extend_from_slice(&message_buf);

        Self::deserialize_message(&full_message)
    }

    fn peer_state(&self) -> PeerState {
        self.state
    }

    fn peer_address(&self) -> Option<SocketAddr> {
        self.peer_address
    }

    async fn disconnect(&mut self) -> Result<(), TorrentError> {
        if let Some(stream) = self.stream.take() {
            drop(stream); // Close the TCP connection
        }
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
            PeerMessage::Have {
                piece_index: PieceIndex::new(5),
            },
            PeerMessage::Request {
                piece_index: PieceIndex::new(10),
                offset: 0,
                length: 16384,
            },
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

    #[test]
    fn test_handshake_serialization() {
        let info_hash = InfoHash::new([1u8; 20]);
        let peer_id = PeerId::new([2u8; 20]);
        let handshake = PeerHandshake::new(info_hash, peer_id);

        let serialized = BitTorrentPeerProtocol::serialize_handshake(&handshake);
        let deserialized = BitTorrentPeerProtocol::deserialize_handshake(&serialized).unwrap();

        assert_eq!(handshake.protocol, deserialized.protocol);
        assert_eq!(handshake.info_hash, deserialized.info_hash);
        assert_eq!(handshake.peer_id, deserialized.peer_id);
    }

    #[test]
    fn test_message_serialization() {
        let test_cases = vec![
            PeerMessage::KeepAlive,
            PeerMessage::Choke,
            PeerMessage::Unchoke,
            PeerMessage::Interested,
            PeerMessage::NotInterested,
            PeerMessage::Have {
                piece_index: PieceIndex::new(42),
            },
            PeerMessage::Request {
                piece_index: PieceIndex::new(10),
                offset: 16384,
                length: 16384,
            },
        ];

        for original_message in test_cases {
            let serialized = BitTorrentPeerProtocol::serialize_message(&original_message);
            let deserialized = BitTorrentPeerProtocol::deserialize_message(&serialized).unwrap();
            assert_eq!(original_message, deserialized);
        }
    }

    #[test]
    fn test_piece_message_with_data() {
        let piece_data = Bytes::from(vec![1, 2, 3, 4, 5]);
        let message = PeerMessage::Piece {
            piece_index: PieceIndex::new(0),
            offset: 0,
            data: piece_data.clone(),
        };

        let serialized = BitTorrentPeerProtocol::serialize_message(&message);
        let deserialized = BitTorrentPeerProtocol::deserialize_message(&serialized).unwrap();

        if let PeerMessage::Piece {
            piece_index,
            offset,
            data,
        } = deserialized
        {
            assert_eq!(piece_index.as_u32(), 0);
            assert_eq!(offset, 0);
            assert_eq!(data, piece_data);
        } else {
            panic!("Expected Piece message");
        }
    }
}
