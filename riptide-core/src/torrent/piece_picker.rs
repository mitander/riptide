//! Piece selection algorithms for torrent downloads.
//!
//! Provides strategies for choosing which pieces to download next.
//! Sequential picker prioritizes in-order download for streaming applications.

use super::PieceIndex;

/// Trait for piece selection strategies.
///
/// Defines interface for algorithms that choose which pieces to download
/// next based on availability, priority, and download strategy.
pub trait PiecePicker: Send + Sync {
    /// Select the next piece to download.
    fn next_piece(&mut self) -> Option<PieceIndex>;

    /// Mark a piece as completed.
    fn mark_completed(&mut self, index: PieceIndex);
}

/// Sequential piece picker optimized for streaming.
///
/// Downloads pieces in order from first to last, which is ideal for
/// streaming media content where sequential access is required.
#[derive(Default)]
pub struct StreamingPiecePicker {
    next_index: u32,
    total_pieces: u32,
}

impl StreamingPiecePicker {
    /// Creates new streaming piece picker.
    pub fn new() -> Self {
        Self {
            next_index: 0,
            total_pieces: 0,
        }
    }

    /// Sets total number of pieces in torrent.
    pub fn with_total_pieces(mut self, total: u32) -> Self {
        self.total_pieces = total;
        self
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
        // Completion tracking implementation pending
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_streaming_picker_sequential_order() {
        let mut picker = StreamingPiecePicker::new().with_total_pieces(5);

        assert_eq!(picker.next_piece(), Some(PieceIndex::new(0)));
        assert_eq!(picker.next_piece(), Some(PieceIndex::new(1)));
        assert_eq!(picker.next_piece(), Some(PieceIndex::new(2)));
        assert_eq!(picker.next_piece(), Some(PieceIndex::new(3)));
        assert_eq!(picker.next_piece(), Some(PieceIndex::new(4)));
        assert_eq!(picker.next_piece(), None);
    }

    #[test]
    fn test_streaming_picker_empty_torrent() {
        let mut picker = StreamingPiecePicker::new().with_total_pieces(0);
        assert_eq!(picker.next_piece(), None);
    }

    #[test]
    fn test_mark_completed_no_panic() {
        let mut picker = StreamingPiecePicker::new();
        picker.mark_completed(PieceIndex::new(42)); // Should not panic
    }
}
