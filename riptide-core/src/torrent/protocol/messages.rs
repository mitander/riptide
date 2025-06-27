//! BitTorrent wire protocol message serialization and deserialization

use bytes::{Buf, BufMut, Bytes};

use super::types::PeerMessage;
use crate::torrent::{PieceIndex, TorrentError};

/// Message serialization utilities for BitTorrent wire protocol.
pub struct MessageCodec;

impl MessageCodec {
    /// Serializes peer message following BEP 3
    pub fn serialize_message(message: &PeerMessage) -> Vec<u8> {
        let mut buffer = Vec::new();

        match message {
            PeerMessage::KeepAlive => {
                buffer.put_u32(0); // Length = 0
            }
            PeerMessage::Choke => {
                buffer.put_u32(1); // Length = 1
                buffer.put_u8(0); // Message ID
            }
            PeerMessage::Unchoke => {
                buffer.put_u32(1);
                buffer.put_u8(1);
            }
            PeerMessage::Interested => {
                buffer.put_u32(1);
                buffer.put_u8(2);
            }
            PeerMessage::NotInterested => {
                buffer.put_u32(1);
                buffer.put_u8(3);
            }
            PeerMessage::Have { piece_index } => {
                buffer.put_u32(5); // Length = 1 + 4
                buffer.put_u8(4); // Message ID
                buffer.put_u32(piece_index.as_u32());
            }
            PeerMessage::Bitfield { bitfield } => {
                buffer.put_u32(1 + bitfield.len() as u32);
                buffer.put_u8(5);
                buffer.extend_from_slice(bitfield);
            }
            PeerMessage::Request {
                piece_index,
                offset,
                length,
            } => {
                buffer.put_u32(13); // Length = 1 + 4 + 4 + 4
                buffer.put_u8(6);
                buffer.put_u32(piece_index.as_u32());
                buffer.put_u32(*offset);
                buffer.put_u32(*length);
            }
            PeerMessage::Piece {
                piece_index,
                offset,
                data,
            } => {
                buffer.put_u32(9 + data.len() as u32); // Length = 1 + 4 + 4 + data.len()
                buffer.put_u8(7);
                buffer.put_u32(piece_index.as_u32());
                buffer.put_u32(*offset);
                buffer.extend_from_slice(data);
            }
            PeerMessage::Cancel {
                piece_index,
                offset,
                length,
            } => {
                buffer.put_u32(13);
                buffer.put_u8(8);
                buffer.put_u32(piece_index.as_u32());
                buffer.put_u32(*offset);
                buffer.put_u32(*length);
            }
            PeerMessage::Port { port } => {
                buffer.put_u32(3); // Length = 1 + 2
                buffer.put_u8(9);
                buffer.put_u16(*port);
            }
        }

        buffer
    }

    /// Deserializes peer message following BEP 3
    ///
    /// # Errors
    /// - `TorrentError::ProtocolError` - Invalid message format or unsupported message type
    pub fn deserialize_message(data: &[u8]) -> Result<PeerMessage, TorrentError> {
        if data.len() < 4 {
            return Err(TorrentError::ProtocolError {
                message: "Message too short".to_string(),
            });
        }

        let mut buffer = data;
        let length = buffer.get_u32();

        if length == 0 {
            return Ok(PeerMessage::KeepAlive);
        }

        if data.len() < 4 + length as usize {
            return Err(TorrentError::ProtocolError {
                message: "Incomplete message".to_string(),
            });
        }

        let message_id = buffer.get_u8();

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
                let piece_index = PieceIndex::new(buffer.get_u32());
                Ok(PeerMessage::Have { piece_index })
            }
            5 => {
                let bitfield_len = length - 1;
                let bitfield = Bytes::copy_from_slice(&buffer[..bitfield_len as usize]);
                Ok(PeerMessage::Bitfield { bitfield })
            }
            6 => {
                if length != 13 {
                    return Err(TorrentError::ProtocolError {
                        message: "Invalid Request message length".to_string(),
                    });
                }
                let piece_index = PieceIndex::new(buffer.get_u32());
                let offset = buffer.get_u32();
                let length = buffer.get_u32();
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
                let piece_index = PieceIndex::new(buffer.get_u32());
                let offset = buffer.get_u32();
                let data_len = length - 9;
                let data = Bytes::copy_from_slice(&buffer[..data_len as usize]);
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
                let piece_index = PieceIndex::new(buffer.get_u32());
                let offset = buffer.get_u32();
                let length = buffer.get_u32();
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
                let port = buffer.get_u16();
                Ok(PeerMessage::Port { port })
            }
            _ => Err(TorrentError::ProtocolError {
                message: format!("Unknown message ID: {message_id}"),
            }),
        }
    }
}
