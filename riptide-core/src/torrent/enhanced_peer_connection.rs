//! Enhanced peer connection with bitfield exchange and choke/unchoke protocol

use std::net::SocketAddr;
use std::time::{Duration, Instant};

use bytes::Bytes;

use super::{
    BitTorrentPeerProtocol, InfoHash, PeerBitfield, PeerConnectionState, PeerHandshake, PeerId,
    PeerMessage, PeerProtocol, PieceIndex, TorrentError,
};

/// Maximum time to wait for connection activity before considering peer stale
const MAX_IDLE_DURATION: Duration = Duration::from_secs(300); // 5 minutes

/// Enhanced peer connection with protocol state management
#[derive(Debug)]
pub struct EnhancedPeerConnection {
    /// Low-level peer protocol implementation
    protocol: BitTorrentPeerProtocol,
    /// High-level peer state (bitfield, choke/unchoke, etc.)
    state: PeerConnectionState,
    /// Peer socket address
    address: SocketAddr,
    /// Connection establishment time
    connected_at: Option<Instant>,
}

impl EnhancedPeerConnection {
    /// Create new enhanced peer connection
    pub fn new(address: SocketAddr, total_pieces: u32) -> Self {
        Self {
            protocol: BitTorrentPeerProtocol::new(),
            state: PeerConnectionState::new(total_pieces),
            address,
            connected_at: None,
        }
    }

    /// Establish connection and perform BitTorrent handshake with protocol initialization
    ///
    /// # Errors
    /// - `TorrentError::PeerConnectionError` - TCP connection failed
    /// - `TorrentError::ProtocolError` - Handshake validation failed
    pub async fn connect_and_initialize(
        &mut self,
        info_hash: InfoHash,
        peer_id: PeerId,
    ) -> Result<(), TorrentError> {
        // Perform low-level connection and handshake
        let handshake = PeerHandshake::new(info_hash, peer_id);
        self.protocol.connect(self.address, handshake).await?;

        self.connected_at = Some(Instant::now());
        self.state.record_activity();

        // After handshake, exchange bitfields and initial protocol messages
        self.exchange_initial_messages().await?;

        Ok(())
    }

    /// Exchange initial protocol messages after handshake
    async fn exchange_initial_messages(&mut self) -> Result<(), TorrentError> {
        // Send our bitfield if we have pieces
        if let Some(our_bitfield) = self.state.our_bitfield_bytes() {
            let bitfield_message = PeerMessage::Bitfield {
                bitfield: Bytes::copy_from_slice(our_bitfield),
            };
            self.protocol.send_message(bitfield_message).await?;
        }

        // Process any immediate messages from peer (like their bitfield)
        self.handle_incoming_messages_nonblocking().await?;

        Ok(())
    }

    /// Handle incoming messages without blocking (process what's immediately available)
    async fn handle_incoming_messages_nonblocking(&mut self) -> Result<(), TorrentError> {
        // This is a simplified version - in a real implementation you'd use non-blocking reads
        // For now, we'll try to read one message with a very short timeout
        match tokio::time::timeout(Duration::from_millis(100), self.protocol.receive_message())
            .await
        {
            Ok(Ok(message)) => {
                self.handle_peer_message(message).await?;
            }
            Ok(Err(_)) | Err(_) => {
                // No message available or error - that's fine for non-blocking
            }
        }

        Ok(())
    }

    /// Handle received peer message and update state accordingly
    ///
    /// # Errors
    /// - `TorrentError::ProtocolError` - Invalid message or protocol violation
    pub async fn handle_peer_message(&mut self, message: PeerMessage) -> Result<(), TorrentError> {
        self.state.record_activity();

        match message {
            PeerMessage::KeepAlive => {
                // Activity already recorded
            }
            PeerMessage::Choke => {
                self.state.handle_choke();
            }
            PeerMessage::Unchoke => {
                self.state.handle_unchoke();
            }
            PeerMessage::Interested => {
                self.state.handle_peer_interested();
            }
            PeerMessage::NotInterested => {
                self.state.handle_peer_not_interested();
            }
            PeerMessage::Have { piece_index } => {
                self.state.peer_has_piece(piece_index);
                // If we're not interested but this peer now has a piece we need, become interested
                self.evaluate_interest().await?;
            }
            PeerMessage::Bitfield { bitfield } => {
                self.state.update_peer_bitfield(bitfield)?;
                // Evaluate our interest based on peer's pieces
                self.evaluate_interest().await?;
            }
            PeerMessage::Request {
                piece_index,
                offset,
                length,
            } => {
                // Handle piece request from peer
                self.handle_piece_request(piece_index, offset, length)
                    .await?;
            }
            PeerMessage::Piece {
                piece_index,
                offset,
                data: _,
            } => {
                // Mark request as completed
                self.state.complete_request(piece_index, offset);
            }
            PeerMessage::Cancel {
                piece_index,
                offset,
                length: _,
            } => {
                // Remove request from pending queue
                self.state.complete_request(piece_index, offset);
            }
            PeerMessage::Port { port: _ } => {
                // DHT port message - not implemented in this basic version
            }
        }

        Ok(())
    }

    /// Evaluate whether we should be interested in this peer based on their pieces
    async fn evaluate_interest(&mut self) -> Result<(), TorrentError> {
        // Simple interest logic: we're interested if peer has any pieces we don't have
        let should_be_interested = if let Some(peer_pieces) = self.state.peer_pieces() {
            // Check if peer has any pieces we need
            (0..peer_pieces.piece_count()).any(|i| {
                let piece_index = PieceIndex::new(i);
                self.state.peer_has_piece_we_need(piece_index)
            })
        } else {
            false
        };

        if should_be_interested != self.state.is_interested() {
            self.state.update_interest(should_be_interested);

            // Only send message if we're actually connected
            if self.is_connected() {
                let message = if should_be_interested {
                    PeerMessage::Interested
                } else {
                    PeerMessage::NotInterested
                };
                self.protocol.send_message(message).await?;
            }
        }

        Ok(())
    }

    /// Handle piece request from peer
    async fn handle_piece_request(
        &mut self,
        piece_index: PieceIndex,
        offset: u32,
        length: u32,
    ) -> Result<(), TorrentError> {
        // Check if we're choking the peer
        if self.state.is_choking_peer() {
            // Ignore request while choking
            return Ok(());
        }

        // Check if we have the requested piece
        if !self.state.we_have_piece_check(piece_index) {
            // We don't have this piece, ignore request
            return Ok(());
        }

        // TODO: In a real implementation, we would:
        // 1. Read the piece data from storage
        // 2. Send the Piece message with the data
        // For now, we'll just acknowledge that we received the request

        // This is where we would integrate with the piece store to read data
        // and send it back to the peer
        tracing::debug!(
            "Received piece request from {}: piece {} offset {} length {}",
            self.address,
            piece_index,
            offset,
            length
        );

        Ok(())
    }

    /// Request piece from peer
    ///
    /// # Errors
    /// - `TorrentError::ProtocolError` - Cannot request (choked, too many pending, etc.)
    /// - `TorrentError::PeerConnectionError` - Send failed
    pub async fn request_piece(
        &mut self,
        piece_index: PieceIndex,
        offset: u32,
        length: u32,
    ) -> Result<(), TorrentError> {
        if !self.state.can_request_pieces() {
            return Err(TorrentError::ProtocolError {
                message: "Cannot request pieces: peer is choking us or too many pending requests"
                    .to_string(),
            });
        }

        // Add to pending requests
        self.state
            .add_pending_request(piece_index, offset, length)?;

        // Send request message
        let request_message = PeerMessage::Request {
            piece_index,
            offset,
            length,
        };
        self.protocol.send_message(request_message).await?;

        Ok(())
    }

    /// Send choke message to peer
    ///
    /// # Errors
    /// - `TorrentError::PeerConnectionError` - Send failed
    pub async fn choke_peer(&mut self) -> Result<(), TorrentError> {
        if !self.state.is_choking_peer() {
            self.state.update_choking_peer(true);
            self.protocol.send_message(PeerMessage::Choke).await?;
        }
        Ok(())
    }

    /// Send unchoke message to peer
    ///
    /// # Errors
    /// - `TorrentError::PeerConnectionError` - Send failed
    pub async fn unchoke_peer(&mut self) -> Result<(), TorrentError> {
        if self.state.is_choking_peer() {
            self.state.update_choking_peer(false);
            self.protocol.send_message(PeerMessage::Unchoke).await?;
        }
        Ok(())
    }

    /// Mark that we have completed a piece (send Have message)
    ///
    /// # Errors
    /// - `TorrentError::PeerConnectionError` - Send failed
    pub async fn announce_piece(&mut self, piece_index: PieceIndex) -> Result<(), TorrentError> {
        self.state.we_have_piece(piece_index);
        let have_message = PeerMessage::Have { piece_index };
        self.protocol.send_message(have_message).await?;
        Ok(())
    }

    /// Process expired requests and return them for retry
    pub fn cleanup_expired_requests(&mut self) -> Vec<super::PendingPieceRequest> {
        self.state.remove_expired_requests()
    }

    /// Check if connection is healthy
    pub fn is_connection_healthy(&self) -> bool {
        // Connection is healthy if:
        // 1. We're connected
        // 2. Activity within reasonable time
        // 3. Not too many failed requests (TODO: implement failure tracking)
        self.is_connected() && !self.state.is_stale(MAX_IDLE_DURATION)
    }

    /// Check if peer is connected
    pub fn is_connected(&self) -> bool {
        self.protocol.peer_address().is_some()
    }

    /// Get connection statistics
    pub fn connection_stats(&self) -> PeerConnectionStats {
        PeerConnectionStats {
            address: self.address,
            connected_at: self.connected_at,
            is_choked_by_peer: self.state.is_choked_by_peer(),
            is_choking_peer: self.state.is_choking_peer(),
            is_interested: self.state.is_interested(),
            is_peer_interested: self.state.is_peer_interested(),
            pending_requests: self.state.pending_request_count(),
            peer_piece_count: self
                .state
                .peer_pieces()
                .map(|bf| bf.piece_count())
                .unwrap_or(0),
        }
    }

    /// Get peer's bitfield
    pub fn peer_bitfield(&self) -> Option<&PeerBitfield> {
        self.state.peer_pieces()
    }

    /// Check if peer has a specific piece
    pub fn peer_has_piece(&self, piece_index: PieceIndex) -> bool {
        self.state.peer_has_piece_we_need(piece_index)
    }

    /// Get peer address
    pub fn peer_address(&self) -> SocketAddr {
        self.address
    }

    /// Disconnect from peer
    ///
    /// # Errors
    /// - `TorrentError::PeerConnectionError` - Disconnect failed
    pub async fn disconnect(&mut self) -> Result<(), TorrentError> {
        self.protocol.disconnect().await?;
        self.connected_at = None;
        Ok(())
    }
}

/// Statistics about a peer connection
#[derive(Debug)]
pub struct PeerConnectionStats {
    /// Network address of the peer
    pub address: SocketAddr,
    /// When the connection was established
    pub connected_at: Option<Instant>,
    /// Whether this peer has choked our connection
    pub is_choked_by_peer: bool,
    /// Whether we are choking this peer
    pub is_choking_peer: bool,
    /// Whether we are interested in this peer's pieces
    pub is_interested: bool,
    /// Whether this peer is interested in our pieces
    pub is_peer_interested: bool,
    /// Number of piece requests currently pending
    pub pending_requests: usize,
    /// Number of pieces this peer has available
    pub peer_piece_count: u32,
}

#[cfg(test)]
mod tests {
    use std::net::{IpAddr, Ipv4Addr};

    use super::*;

    fn create_test_connection() -> EnhancedPeerConnection {
        let address = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 6881);
        EnhancedPeerConnection::new(address, 100)
    }

    #[test]
    fn test_enhanced_connection_creation() {
        let connection = create_test_connection();
        assert!(!connection.is_connected());
        assert!(!connection.is_connection_healthy()); // Not healthy when not connected
        assert_eq!(connection.peer_address().port(), 6881);
    }

    #[test]
    fn test_connection_stats() {
        let connection = create_test_connection();
        let stats = connection.connection_stats();

        assert_eq!(stats.address.port(), 6881);
        assert!(stats.connected_at.is_none());
        assert!(stats.is_choked_by_peer);
        assert!(stats.is_choking_peer);
        assert!(!stats.is_interested);
        assert!(!stats.is_peer_interested);
        assert_eq!(stats.pending_requests, 0);
        assert_eq!(stats.peer_piece_count, 0);
    }

    #[tokio::test]
    async fn test_choke_unchoke_operations() {
        let mut connection = create_test_connection();

        // Initially choking
        assert!(connection.state.is_choking_peer());

        // Unchoke should work (even if not connected, state should update)
        // Note: This will fail to send message since we're not connected, but state should update
        let _ = connection.unchoke_peer().await; // Ignore send error
        assert!(!connection.state.is_choking_peer());

        let _ = connection.choke_peer().await; // Ignore send error
        assert!(connection.state.is_choking_peer());
    }

    #[tokio::test]
    async fn test_piece_announcement() {
        let mut connection = create_test_connection();
        let piece_index = PieceIndex::new(42);

        // Initially don't have piece
        assert!(!connection.state.we_have_piece_check(piece_index));

        // Announce piece (will fail to send since not connected, but state should update)
        let _ = connection.announce_piece(piece_index).await; // Ignore send error
        assert!(connection.state.we_have_piece_check(piece_index));
    }

    #[tokio::test]
    async fn test_message_handling() {
        let mut connection = create_test_connection();

        // Handle choke message
        connection
            .handle_peer_message(PeerMessage::Choke)
            .await
            .unwrap();
        assert!(connection.state.is_choked_by_peer());
        assert!(!connection.state.can_request_pieces());

        // Handle unchoke message
        connection
            .handle_peer_message(PeerMessage::Unchoke)
            .await
            .unwrap();
        assert!(!connection.state.is_choked_by_peer());
        assert!(connection.state.can_request_pieces());

        // Handle interested message
        connection
            .handle_peer_message(PeerMessage::Interested)
            .await
            .unwrap();
        assert!(connection.state.is_peer_interested());
    }

    #[tokio::test]
    async fn test_have_message_handling() {
        let mut connection = create_test_connection();
        let piece_index = PieceIndex::new(5);

        // Initially peer doesn't have piece
        assert!(!connection.peer_has_piece(piece_index));

        // Handle Have message
        connection
            .handle_peer_message(PeerMessage::Have { piece_index })
            .await
            .unwrap();

        // Now peer should have the piece
        assert!(connection.peer_has_piece(piece_index));
    }

    #[tokio::test]
    async fn test_bitfield_message_handling() {
        let mut connection = create_test_connection();

        // Create test bitfield (first 3 pieces)
        let mut bitfield_bytes = vec![0u8; 13]; // 100 pieces = 13 bytes
        bitfield_bytes[0] = 0b11100000; // Pieces 0, 1, 2
        let bitfield = Bytes::from(bitfield_bytes);

        // Handle bitfield message (this should work without connection since it's only state)
        connection
            .handle_peer_message(PeerMessage::Bitfield { bitfield })
            .await
            .unwrap();

        // Check peer has pieces
        assert!(connection.peer_has_piece(PieceIndex::new(0)));
        assert!(connection.peer_has_piece(PieceIndex::new(1)));
        assert!(connection.peer_has_piece(PieceIndex::new(2)));
        assert!(!connection.peer_has_piece(PieceIndex::new(3)));

        let stats = connection.connection_stats();
        assert_eq!(stats.peer_piece_count, 3);
    }
}
