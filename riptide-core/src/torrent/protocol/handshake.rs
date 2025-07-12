//! BitTorrent handshake serialization and deserialization

use super::types::{PeerHandshake, PeerId};
use crate::torrent::{InfoHash, TorrentError};

/// Handshake serialization utilities for BitTorrent wire protocol.
pub struct HandshakeCodec;

impl HandshakeCodec {
    /// Serializes handshake message following BEP 3
    pub fn serialize_handshake(handshake: &PeerHandshake) -> Vec<u8> {
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
    ///
    /// # Errors
    ///
    /// - `TorrentError::ProtocolError` - If invalid handshake format or length
    pub fn deserialize_handshake(data: &[u8]) -> Result<PeerHandshake, TorrentError> {
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
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_handshake() -> PeerHandshake {
        let info_hash = InfoHash::new([0x12; 20]);
        let peer_id = PeerId::new([0x34; 20]);
        PeerHandshake::new(info_hash, peer_id)
    }

    #[test]
    fn test_handshake_serialization_roundtrip() {
        let original = create_test_handshake();

        let serialized = HandshakeCodec::serialize_handshake(&original);
        let deserialized = HandshakeCodec::deserialize_handshake(&serialized).unwrap();

        assert_eq!(original.protocol, deserialized.protocol);
        assert_eq!(original.reserved, deserialized.reserved);
        assert_eq!(original.info_hash, deserialized.info_hash);
        assert_eq!(original.peer_id, deserialized.peer_id);
    }

    #[test]
    fn test_handshake_serialization_format() {
        let handshake = create_test_handshake();
        let serialized = HandshakeCodec::serialize_handshake(&handshake);

        // Check protocol length (19 for "BitTorrent protocol")
        assert_eq!(serialized[0], 19);

        // Check protocol string
        assert_eq!(&serialized[1..20], b"BitTorrent protocol");

        // Check reserved bytes (should be zeros)
        assert_eq!(&serialized[20..28], &[0u8; 8]);

        // Check info hash
        assert_eq!(&serialized[28..48], &[0x12; 20]);

        // Check peer ID
        assert_eq!(&serialized[48..68], &[0x34; 20]);

        // Total length should be 68 bytes
        assert_eq!(serialized.len(), 68);
    }

    #[test]
    fn test_handshake_deserialization_too_short() {
        let short_data = vec![0u8; 48]; // One byte short
        let result = HandshakeCodec::deserialize_handshake(&short_data);

        assert!(result.is_err());
        if let Err(TorrentError::ProtocolError { message }) = result {
            assert!(message.contains("too short"));
        } else {
            panic!("Expected ProtocolError with 'too short' message");
        }
    }

    #[test]
    fn test_handshake_deserialization_invalid_length() {
        // Create data with protocol length that exceeds actual data
        let mut handshake_data = vec![0u8; 100];
        handshake_data[0] = 200; // Invalid protocol length

        let result = HandshakeCodec::deserialize_handshake(&handshake_data);

        assert!(result.is_err());
        if let Err(TorrentError::ProtocolError { message }) = result {
            assert!(message.contains("Invalid handshake length"));
        } else {
            panic!("Expected ProtocolError with 'Invalid handshake length' message");
        }
    }

    #[test]
    fn test_handshake_with_custom_protocol() {
        let info_hash = InfoHash::new([0xAB; 20]);
        let peer_id = PeerId::new([0xCD; 20]);
        let handshake = PeerHandshake {
            protocol: "CustomProtocol".to_string(),
            reserved: [0xFF; 8],
            info_hash,
            peer_id,
        };

        let serialized = HandshakeCodec::serialize_handshake(&handshake);
        let deserialized = HandshakeCodec::deserialize_handshake(&serialized).unwrap();

        assert_eq!(handshake.protocol, deserialized.protocol);
        assert_eq!(handshake.reserved, deserialized.reserved);
        assert_eq!(handshake.info_hash, deserialized.info_hash);
        assert_eq!(handshake.peer_id, deserialized.peer_id);
    }

    #[test]
    fn test_handshake_with_reserved_bytes() {
        let info_hash = InfoHash::new([0x01; 20]);
        let peer_id = PeerId::new([0x02; 20]);
        let reserved_bytes = [0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88];

        let handshake = PeerHandshake {
            protocol: "BitTorrent protocol".to_string(),
            reserved: reserved_bytes,
            info_hash,
            peer_id,
        };

        let serialized = HandshakeCodec::serialize_handshake(&handshake);
        let deserialized = HandshakeCodec::deserialize_handshake(&serialized).unwrap();

        assert_eq!(deserialized.reserved, reserved_bytes);
    }

    #[test]
    fn test_handshake_minimal_valid_size() {
        // Create minimal valid handshake (1-char protocol)
        let mut handshake_data = vec![0u8; 50]; // 1 + 1 + 8 + 20 + 20 = 50
        handshake_data[0] = 1; // Protocol length of 1
        handshake_data[1] = b'X'; // Single character protocol

        let result = HandshakeCodec::deserialize_handshake(&handshake_data);
        assert!(result.is_ok());

        let handshake = result.unwrap();
        assert_eq!(handshake.protocol, "X");
    }

    #[test]
    fn test_handshake_empty_protocol() {
        // Test with empty protocol string
        let mut handshake_data = vec![0u8; 49]; // 1 + 0 + 8 + 20 + 20 = 49
        handshake_data[0] = 0; // Protocol length of 0

        let result = HandshakeCodec::deserialize_handshake(&handshake_data);
        assert!(result.is_ok());

        let handshake = result.unwrap();
        assert_eq!(handshake.protocol, "");
    }
}
