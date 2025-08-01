//! BitTorrent peer protocol implementation with TCP connection management

use std::net::SocketAddr;

use async_trait::async_trait;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

use super::handshake::HandshakeCodec;
use super::messages::MessageCodec;
use super::types::{PeerHandshake, PeerMessage, PeerProtocol, PeerState};
use crate::torrent::TorrentError;

/// Reference implementation of BitTorrent wire protocol.
///
/// Production TCP-based implementation of BEP 3 wire protocol.
/// Handles connection management, message serialization, and protocol state.
#[derive(Debug, Default)]
pub struct BitTorrentPeerProtocol {
    state: PeerState,
    peer_address: Option<SocketAddr>,
    stream: Option<TcpStream>,
}

impl BitTorrentPeerProtocol {
    /// Creates new protocol instance in disconnected state.
    pub fn new() -> Self {
        Self {
            state: PeerState::Disconnected,
            peer_address: None,
            stream: None,
        }
    }
}

#[async_trait]
impl PeerProtocol for BitTorrentPeerProtocol {
    async fn connect(
        &mut self,
        address: SocketAddr,
        handshake: PeerHandshake,
    ) -> Result<(), TorrentError> {
        self.state = PeerState::Connecting;
        self.peer_address = Some(address);

        // Establish TCP connection with timeout
        let stream = match tokio::time::timeout(
            std::time::Duration::from_secs(3),
            TcpStream::connect(address),
        )
        .await
        {
            Ok(Ok(stream)) => stream,
            Ok(Err(_)) | Err(_) => {
                self.state = PeerState::Disconnected;
                self.peer_address = None;
                return Err(TorrentError::PeerConnectionError {
                    reason: format!("Failed to connect to {address}"),
                });
            }
        };

        self.stream = Some(stream);
        self.state = PeerState::Handshaking;

        // Send handshake
        let handshake_data = HandshakeCodec::serialize_handshake(&handshake);
        if let Some(ref mut stream) = self.stream
            && stream.write_all(&handshake_data).await.is_err()
        {
            self.state = PeerState::Disconnected;
            return Err(TorrentError::PeerConnectionError {
                reason: "Failed to send handshake".to_string(),
            });
        }

        // Read peer handshake response
        let mut handshake_buffer = vec![0u8; 68]; // 1 + 19 + 8 + 20 + 20
        if let Some(ref mut stream) = self.stream
            && stream.read_exact(&mut handshake_buffer).await.is_err()
        {
            self.state = PeerState::Disconnected;
            return Err(TorrentError::PeerConnectionError {
                reason: "Failed to read handshake response".to_string(),
            });
        }

        // Validate peer handshake
        let peer_handshake = HandshakeCodec::deserialize_handshake(&handshake_buffer)?;
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

        let message_data = MessageCodec::serialize_message(&message);
        if let Some(ref mut stream) = self.stream
            && stream.write_all(&message_data).await.is_err()
        {
            self.state = PeerState::Disconnected;
            return Err(TorrentError::PeerConnectionError {
                reason: "Failed to send message".to_string(),
            });
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
        if let Some(ref mut stream) = self.stream
            && stream.read_exact(&mut length_buf).await.is_err()
        {
            self.state = PeerState::Disconnected;
            return Err(TorrentError::PeerConnectionError {
                reason: "Failed to read message length".to_string(),
            });
        }

        let length = u32::from_be_bytes(length_buf);

        // Handle keep-alive (length = 0)
        if length == 0 {
            return Ok(PeerMessage::KeepAlive);
        }

        // Read message payload
        let mut message_buf = vec![0u8; length as usize];
        if let Some(ref mut stream) = self.stream
            && stream.read_exact(&mut message_buf).await.is_err()
        {
            self.state = PeerState::Disconnected;
            return Err(TorrentError::PeerConnectionError {
                reason: "Failed to read message payload".to_string(),
            });
        }

        // Reconstruct full message for parsing (length + payload)
        let mut full_message = Vec::with_capacity(4 + length as usize);
        full_message.extend_from_slice(&length_buf);
        full_message.extend_from_slice(&message_buf);

        MessageCodec::deserialize_message(&full_message)
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
mod protocol_connection_tests {
    use std::net::SocketAddr;
    use std::str::FromStr;

    use super::*;
    use crate::torrent::{InfoHash, PeerId};

    fn create_test_handshake() -> PeerHandshake {
        let info_hash = InfoHash::new([0x12; 20]);
        let peer_id = PeerId::new([0x34; 20]);
        PeerHandshake::new(info_hash, peer_id)
    }

    #[test]
    fn test_protocol_creation() {
        let protocol = BitTorrentPeerProtocol::new();
        assert_eq!(protocol.peer_state(), PeerState::Disconnected);
        assert_eq!(protocol.peer_address(), None);
    }

    #[test]
    fn test_protocol_default() {
        let protocol = BitTorrentPeerProtocol::default();
        assert_eq!(protocol.peer_state(), PeerState::Disconnected);
        assert_eq!(protocol.peer_address(), None);
    }

    #[test]
    fn test_handshake_creation() {
        let handshake = create_test_handshake();
        assert_eq!(handshake.info_hash, InfoHash::new([0x12; 20]));
        assert_eq!(handshake.peer_id, PeerId::new([0x34; 20]));
        assert_eq!(handshake.protocol, "BitTorrent protocol");
    }

    #[tokio::test]
    async fn test_connect_to_nonexistent_peer() {
        let mut protocol = BitTorrentPeerProtocol::new();
        let peer_addr = SocketAddr::from_str("127.0.0.1:0").unwrap(); // Port 0 should fail
        let handshake = create_test_handshake();

        let result = protocol.connect(peer_addr, handshake).await;
        assert!(result.is_err());
        assert_eq!(protocol.peer_state(), PeerState::Disconnected);
    }

    #[tokio::test]
    async fn test_send_message_when_not_connected() {
        let mut protocol = BitTorrentPeerProtocol::new();
        let message = PeerMessage::Choke;

        let result = protocol.send_message(message).await;
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            TorrentError::PeerConnectionError { reason } if reason.contains("Not connected")
        ));
    }

    #[tokio::test]
    async fn test_receive_message_when_not_connected() {
        let mut protocol = BitTorrentPeerProtocol::new();

        let result = protocol.receive_message().await;
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            TorrentError::PeerConnectionError { reason } if reason.contains("Not connected")
        ));
    }

    #[tokio::test]
    async fn test_disconnect() {
        let mut protocol = BitTorrentPeerProtocol::new();

        // Should succeed even when not connected
        let result = protocol.disconnect().await;
        assert!(result.is_ok());
        assert_eq!(protocol.peer_state(), PeerState::Disconnected);
        assert_eq!(protocol.peer_address(), None);
    }
}
