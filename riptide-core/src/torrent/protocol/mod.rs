//! BitTorrent wire protocol abstractions and message types.
//!
//! BitTorrent peer-to-peer protocol implementation following BEP 3.
//! Defines message types, handshake procedures, and connection state management
//! for communicating with remote peers.

pub mod connection;
pub mod handshake;
pub mod messages;
pub mod types;

// Re-export public API
pub use connection::BitTorrentPeerProtocol;
pub use types::{PeerHandshake, PeerId, PeerMessage, PeerProtocol, PeerState};

#[cfg(test)]
mod tests {
    use bytes::Bytes;

    use super::connection::BitTorrentPeerProtocol;
    use super::handshake::HandshakeCodec;
    use super::messages::MessageCodec;
    use super::types::{PeerHandshake, PeerId, PeerMessage, PeerProtocol, PeerState};
    use crate::torrent::{InfoHash, PieceIndex};

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

        let serialized = HandshakeCodec::serialize_handshake(&handshake);
        let deserialized = HandshakeCodec::deserialize_handshake(&serialized).unwrap();

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
            let serialized = MessageCodec::serialize_message(&original_message);
            let deserialized = MessageCodec::deserialize_message(&serialized).unwrap();
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

        let serialized = MessageCodec::serialize_message(&message);
        let deserialized = MessageCodec::deserialize_message(&serialized).unwrap();

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
