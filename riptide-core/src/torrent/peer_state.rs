//! Peer connection state management with bitfield and choke/unchoke protocol

use std::collections::VecDeque;
use std::time::{Duration, Instant};

use bytes::Bytes;

use crate::torrent::{PieceIndex, TorrentError};

/// Maximum number of pending requests per peer to prevent flooding
const MAX_PENDING_REQUESTS: usize = 10;

/// Request timeout duration - if no response in 30 seconds, consider request failed
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

/// Represents a piece request made to a peer
#[derive(Debug, Clone)]
pub struct PendingPieceRequest {
    pub piece_index: PieceIndex,
    pub offset: u32,
    pub length: u32,
    pub requested_at: Instant,
}

impl PendingPieceRequest {
    /// Create new pending request with current timestamp
    pub fn new(piece_index: PieceIndex, offset: u32, length: u32) -> Self {
        Self {
            piece_index,
            offset,
            length,
            requested_at: Instant::now(),
        }
    }

    /// Check if request has exceeded timeout duration
    pub fn is_expired(&self) -> bool {
        self.requested_at.elapsed() > REQUEST_TIMEOUT
    }
}

/// Manages peer connection state including bitfield and choke/unchoke protocol
#[derive(Debug)]
pub struct PeerConnectionState {
    /// Whether we are choking the peer (preventing uploads to them)
    peer_choked: bool,
    /// Whether the peer is choking us (preventing downloads from them)
    choked_by_peer: bool,
    /// Whether we are interested in pieces from this peer
    interested: bool,
    /// Whether the peer is interested in pieces from us
    peer_interested: bool,
    /// Bitfield representing pieces the peer has (bit per piece)
    peer_pieces: Option<PeerBitfield>,
    /// Our bitfield representing pieces we have
    our_pieces: Option<PeerBitfield>,
    /// Queue of piece requests we've sent to this peer
    pending_requests: VecDeque<PendingPieceRequest>,
    /// Total number of pieces for this torrent
    total_pieces: u32,
    /// Last activity timestamp for connection health monitoring
    last_activity: Instant,
}

/// Bitfield representing which pieces a peer has
#[derive(Debug, Clone)]
pub struct PeerBitfield {
    bits: Vec<u8>,
    piece_count: u32,
}

impl PeerBitfield {
    /// Create new bitfield for given number of pieces
    pub fn new(piece_count: u32) -> Self {
        let byte_count = piece_count.div_ceil(8); // Round up to nearest byte
        Self {
            bits: vec![0u8; byte_count as usize],
            piece_count,
        }
    }

    /// Create bitfield from raw bytes (received from peer)
    ///
    /// # Errors
    /// - `TorrentError::ProtocolError` - Invalid bitfield size for piece count
    pub fn from_bytes(piece_data: Bytes, piece_count: u32) -> Result<Self, TorrentError> {
        let expected_bytes = piece_count.div_ceil(8);
        if piece_data.len() != expected_bytes as usize {
            return Err(TorrentError::ProtocolError {
                message: format!(
                    "Invalid bitfield size: expected {} bytes for {} pieces, got {}",
                    expected_bytes,
                    piece_count,
                    piece_data.len()
                ),
            });
        }

        Ok(Self {
            bits: piece_data.to_vec(),
            piece_count,
        })
    }

    /// Check if peer has specific piece
    pub fn has_piece(&self, piece_index: PieceIndex) -> bool {
        let index = piece_index.as_u32();
        if index >= self.piece_count {
            return false;
        }

        let byte_index = (index / 8) as usize;
        let bit_index = 7 - (index % 8); // MSB first

        byte_index < self.bits.len() && (self.bits[byte_index] & (1 << bit_index)) != 0
    }

    /// Mark piece as available (set bit)
    pub fn set_piece(&mut self, piece_index: PieceIndex) {
        let index = piece_index.as_u32();
        if index >= self.piece_count {
            return;
        }

        let byte_index = (index / 8) as usize;
        let bit_index = 7 - (index % 8); // MSB first

        if byte_index < self.bits.len() {
            self.bits[byte_index] |= 1 << bit_index;
        }
    }

    /// Mark piece as unavailable (clear bit)
    pub fn clear_piece(&mut self, piece_index: PieceIndex) {
        let index = piece_index.as_u32();
        if index >= self.piece_count {
            return;
        }

        let byte_index = (index / 8) as usize;
        let bit_index = 7 - (index % 8); // MSB first

        if byte_index < self.bits.len() {
            self.bits[byte_index] &= !(1 << bit_index);
        }
    }

    /// Get raw bytes for transmission
    pub fn as_bytes(&self) -> &[u8] {
        &self.bits
    }

    /// Count total number of pieces this peer has
    pub fn piece_count(&self) -> u32 {
        let mut count = 0;
        for &byte in &self.bits {
            count += byte.count_ones();
        }
        count
    }
}

impl PeerConnectionState {
    /// Create new peer connection state for given torrent
    pub fn new(total_pieces: u32) -> Self {
        Self {
            peer_choked: true,    // Start with peer choked
            choked_by_peer: true, // Assume we're choked until unchoke message
            interested: false,
            peer_interested: false,
            peer_pieces: None,
            our_pieces: Some(PeerBitfield::new(total_pieces)), // We know our own pieces
            pending_requests: VecDeque::new(),
            total_pieces,
            last_activity: Instant::now(),
        }
    }

    /// Update activity timestamp
    pub fn record_activity(&mut self) {
        self.last_activity = Instant::now();
    }

    /// Check if connection has been inactive for too long
    pub fn is_stale(&self, max_idle: Duration) -> bool {
        self.last_activity.elapsed() > max_idle
    }

    /// Set peer's bitfield from received message
    ///
    /// # Errors
    /// - `TorrentError::ProtocolError` - Invalid bitfield data
    pub fn update_peer_bitfield(&mut self, bitfield_data: Bytes) -> Result<(), TorrentError> {
        self.record_activity();
        self.peer_pieces = Some(PeerBitfield::from_bytes(bitfield_data, self.total_pieces)?);
        Ok(())
    }

    /// Mark that peer has a specific piece (from Have message)
    pub fn peer_has_piece(&mut self, piece_index: PieceIndex) {
        self.record_activity();

        // Initialize empty bitfield if we haven't received one yet
        if self.peer_pieces.is_none() {
            self.peer_pieces = Some(PeerBitfield::new(self.total_pieces));
        }

        if let Some(ref mut pieces) = self.peer_pieces {
            pieces.set_piece(piece_index);
        }
    }

    /// Mark that we have a specific piece
    pub fn we_have_piece(&mut self, piece_index: PieceIndex) {
        if let Some(ref mut pieces) = self.our_pieces {
            pieces.set_piece(piece_index);
        }
    }

    /// Check if peer has a piece we're interested in
    pub fn peer_has_piece_we_need(&self, piece_index: PieceIndex) -> bool {
        if let Some(ref peer_pieces) = self.peer_pieces {
            peer_pieces.has_piece(piece_index) && !self.we_have_piece_check(piece_index)
        } else {
            false
        }
    }

    /// Check if we have a specific piece
    pub fn we_have_piece_check(&self, piece_index: PieceIndex) -> bool {
        if let Some(ref our_pieces) = self.our_pieces {
            our_pieces.has_piece(piece_index)
        } else {
            false
        }
    }

    /// Handle choke message from peer
    pub fn handle_choke(&mut self) {
        self.record_activity();
        self.choked_by_peer = true;
        // Cancel all pending requests when choked
        self.pending_requests.clear();
    }

    /// Handle unchoke message from peer
    pub fn handle_unchoke(&mut self) {
        self.record_activity();
        self.choked_by_peer = false;
    }

    /// Handle interested message from peer
    pub fn handle_peer_interested(&mut self) {
        self.record_activity();
        self.peer_interested = true;
    }

    /// Handle not interested message from peer
    pub fn handle_peer_not_interested(&mut self) {
        self.record_activity();
        self.peer_interested = false;
    }

    /// Set our interest in this peer
    pub fn set_interested(&mut self, interested: bool) {
        self.interested = interested;
    }

    /// Set whether we're choking this peer
    pub fn set_choking_peer(&mut self, choking: bool) {
        self.peer_choked = choking;
    }

    /// Check if we can request pieces from this peer
    pub fn can_request_pieces(&self) -> bool {
        !self.choked_by_peer && self.pending_requests.len() < MAX_PENDING_REQUESTS
    }

    /// Add piece request to pending queue
    ///
    /// # Errors
    /// - `TorrentError::ProtocolError` - Too many pending requests
    pub fn add_pending_request(
        &mut self,
        piece_index: PieceIndex,
        offset: u32,
        length: u32,
    ) -> Result<(), TorrentError> {
        if self.pending_requests.len() >= MAX_PENDING_REQUESTS {
            return Err(TorrentError::ProtocolError {
                message: "Too many pending requests".to_string(),
            });
        }

        self.pending_requests
            .push_back(PendingPieceRequest::new(piece_index, offset, length));
        Ok(())
    }

    /// Remove request from pending queue (when piece received)
    pub fn complete_request(&mut self, piece_index: PieceIndex, offset: u32) -> bool {
        if let Some(pos) = self
            .pending_requests
            .iter()
            .position(|req| req.piece_index == piece_index && req.offset == offset)
        {
            self.pending_requests.remove(pos);
            true
        } else {
            false
        }
    }

    /// Remove expired requests and return them for retry
    pub fn remove_expired_requests(&mut self) -> Vec<PendingPieceRequest> {
        let mut expired = Vec::new();

        while let Some(request) = self.pending_requests.front() {
            if request.is_expired() {
                expired.push(self.pending_requests.pop_front().unwrap());
            } else {
                break; // Requests are ordered by time, so we can stop at first non-expired
            }
        }

        expired
    }

    /// Get read-only access to peer's bitfield
    pub fn peer_pieces(&self) -> Option<&PeerBitfield> {
        self.peer_pieces.as_ref()
    }

    /// Get our bitfield for sending to peer
    pub fn our_bitfield_bytes(&self) -> Option<&[u8]> {
        self.our_pieces.as_ref().map(|bf| bf.as_bytes())
    }

    /// Check current choke/unchoke state
    pub fn is_choked_by_peer(&self) -> bool {
        self.choked_by_peer
    }

    /// Check if we're choking the peer
    pub fn is_choking_peer(&self) -> bool {
        self.peer_choked
    }

    /// Check if we're interested in the peer
    pub fn is_interested(&self) -> bool {
        self.interested
    }

    /// Check if peer is interested in us
    pub fn is_peer_interested(&self) -> bool {
        self.peer_interested
    }

    /// Get number of pending requests
    pub fn pending_request_count(&self) -> usize {
        self.pending_requests.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_peer_bitfield_creation() {
        let bitfield = PeerBitfield::new(100);
        assert_eq!(bitfield.piece_count, 100);
        assert_eq!(bitfield.bits.len(), 13); // (100 + 7) / 8 = 13 bytes
        assert!(!bitfield.has_piece(PieceIndex::new(0)));
        assert!(!bitfield.has_piece(PieceIndex::new(99)));
    }

    #[test]
    fn test_bitfield_piece_operations() {
        let mut bitfield = PeerBitfield::new(16);

        // Set and check pieces
        bitfield.set_piece(PieceIndex::new(0));
        bitfield.set_piece(PieceIndex::new(7));
        bitfield.set_piece(PieceIndex::new(15));

        assert!(bitfield.has_piece(PieceIndex::new(0)));
        assert!(bitfield.has_piece(PieceIndex::new(7)));
        assert!(bitfield.has_piece(PieceIndex::new(15)));
        assert!(!bitfield.has_piece(PieceIndex::new(1)));
        assert!(!bitfield.has_piece(PieceIndex::new(8)));

        // Clear piece
        bitfield.clear_piece(PieceIndex::new(7));
        assert!(!bitfield.has_piece(PieceIndex::new(7)));

        // Count pieces
        assert_eq!(bitfield.piece_count(), 2);
    }

    #[test]
    fn test_bitfield_from_bytes() {
        let piece_data = Bytes::from(vec![0b10000000, 0b00000001]); // Pieces 0 and 15
        let bitfield = PeerBitfield::from_bytes(piece_data, 16).unwrap();

        assert!(bitfield.has_piece(PieceIndex::new(0)));
        assert!(bitfield.has_piece(PieceIndex::new(15)));
        assert!(!bitfield.has_piece(PieceIndex::new(1)));
        assert_eq!(bitfield.piece_count(), 2);
    }

    #[test]
    fn test_bitfield_invalid_size() {
        let piece_data = Bytes::from(vec![0xFF]); // 1 byte for 16 pieces (should be 2)
        let result = PeerBitfield::from_bytes(piece_data, 16);
        assert!(result.is_err());
    }

    #[test]
    fn test_peer_connection_state_creation() {
        let state = PeerConnectionState::new(100);
        assert!(state.is_choked_by_peer());
        assert!(state.is_choking_peer());
        assert!(!state.is_interested());
        assert!(!state.is_peer_interested());
        assert_eq!(state.pending_request_count(), 0);
    }

    #[test]
    fn test_choke_unchoke_handling() {
        let mut state = PeerConnectionState::new(100);

        // Start choked
        assert!(state.is_choked_by_peer());
        assert!(!state.can_request_pieces());

        // Unchoke allows requests
        state.handle_unchoke();
        assert!(!state.is_choked_by_peer());
        assert!(state.can_request_pieces());

        // Choke prevents requests and clears pending
        state
            .add_pending_request(PieceIndex::new(0), 0, 16384)
            .unwrap();
        assert_eq!(state.pending_request_count(), 1);

        state.handle_choke();
        assert!(state.is_choked_by_peer());
        assert!(!state.can_request_pieces());
        assert_eq!(state.pending_request_count(), 0);
    }

    #[test]
    fn test_interest_handling() {
        let mut state = PeerConnectionState::new(100);

        assert!(!state.is_interested());
        assert!(!state.is_peer_interested());

        state.set_interested(true);
        state.handle_peer_interested();

        assert!(state.is_interested());
        assert!(state.is_peer_interested());

        state.handle_peer_not_interested();
        assert!(!state.is_peer_interested());
    }

    #[test]
    fn test_pending_request_management() {
        let mut state = PeerConnectionState::new(100);
        state.handle_unchoke(); // Allow requests

        // Add requests
        state
            .add_pending_request(PieceIndex::new(0), 0, 16384)
            .unwrap();
        state
            .add_pending_request(PieceIndex::new(1), 0, 16384)
            .unwrap();
        assert_eq!(state.pending_request_count(), 2);

        // Complete request
        assert!(state.complete_request(PieceIndex::new(0), 0));
        assert_eq!(state.pending_request_count(), 1);

        // Complete non-existent request
        assert!(!state.complete_request(PieceIndex::new(5), 0));
        assert_eq!(state.pending_request_count(), 1);
    }

    #[test]
    fn test_max_pending_requests() {
        let mut state = PeerConnectionState::new(100);
        state.handle_unchoke();

        // Fill up to limit
        for i in 0..MAX_PENDING_REQUESTS {
            state
                .add_pending_request(PieceIndex::new(i as u32), 0, 16384)
                .unwrap();
        }

        // Adding one more should fail
        let result =
            state.add_pending_request(PieceIndex::new(MAX_PENDING_REQUESTS as u32), 0, 16384);
        assert!(result.is_err());
    }

    #[test]
    fn test_peer_piece_tracking() {
        let mut state = PeerConnectionState::new(100);

        // No pieces initially
        assert!(!state.peer_has_piece_we_need(PieceIndex::new(0)));

        // Peer has piece
        state.peer_has_piece(PieceIndex::new(0));
        assert!(state.peer_has_piece_we_need(PieceIndex::new(0)));

        // We have the piece too
        state.we_have_piece(PieceIndex::new(0));
        assert!(!state.peer_has_piece_we_need(PieceIndex::new(0))); // No longer needed
    }
}
