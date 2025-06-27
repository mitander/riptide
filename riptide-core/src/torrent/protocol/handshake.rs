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
    /// - `TorrentError::ProtocolError` - Invalid handshake format or length
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
