//! Piece selection algorithms optimized for streaming

use super::PieceIndex;

/// Trait for piece selection strategies
pub trait PiecePicker: Send + Sync {
    /// Select the next piece to download
    fn next_piece(&mut self) -> Option<PieceIndex>;
    
    /// Mark a piece as completed
    fn mark_completed(&mut self, index: PieceIndex);
}

/// Sequential piece picker optimized for streaming
pub struct StreamingPiecePicker {
    next_index: u32,
    total_pieces: u32,
}

impl StreamingPiecePicker {
    pub fn new() -> Self {
        Self {
            next_index: 0,
            total_pieces: 0,
        }
    }
}

impl PiecePicker for StreamingPiecePicker {
    fn next_piece(&mut self) -> Option<PieceIndex> {
        if self.next_index < self.total_pieces {
            let index = PieceIndex::new(self.next_index);
            self.next_index += 1;
            Some(index)
        } else {
            None
        }
    }
    
    fn mark_completed(&mut self, _index: PieceIndex) {
        // TODO: Track completed pieces
    }
}