//! Centralized range calculation utilities for streaming operations
//!
//! Provides efficient calculations for converting between byte ranges and piece indices,
//! validating HTTP ranges, and determining buffer requirements for streaming operations.
//! Consolidates range logic previously scattered across multiple streaming components.

use std::ops::Range;

use crate::storage::data_source::{DataError, DataResult};
use crate::torrent::InfoHash;

/// Information about a piece range required for a byte range
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PieceRange {
    pub piece_index: u32,
    pub start_offset: u32,
    pub end_offset: u32,
    pub priority: PiecePriority,
}

/// Priority levels for piece downloading
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum PiecePriority {
    /// Low priority - prefetch for future playback
    Low = 0,
    /// Normal priority - standard download
    Normal = 1,
    /// High priority - needed for current playback
    High = 2,
    /// Critical priority - blocking current operation
    Critical = 3,
}

/// Buffer requirements for efficient streaming
#[derive(Debug, Clone)]
pub struct BufferInfo {
    pub minimum_buffer_size: u64,
    pub recommended_buffer_size: u64,
    pub prefetch_pieces: Vec<u32>,
    pub critical_pieces: Vec<u32>,
}

/// Metadata about torrent structure needed for calculations
#[derive(Debug, Clone)]
pub struct TorrentLayout {
    pub piece_size: u32,
    pub total_pieces: u32,
    pub total_size: u64,
    pub last_piece_size: u32,
}

impl TorrentLayout {
    /// Create new torrent layout from basic parameters
    pub fn new(piece_size: u32, total_pieces: u32, total_size: u64) -> Self {
        let last_piece_size = if total_pieces > 0 {
            let last_piece_offset = (total_pieces - 1) as u64 * piece_size as u64;
            (total_size - last_piece_offset) as u32
        } else {
            0
        };

        Self {
            piece_size,
            total_pieces,
            total_size,
            last_piece_size,
        }
    }

    /// Calculate the actual size of a specific piece
    pub fn piece_size(&self, piece_index: u32) -> u32 {
        if piece_index >= self.total_pieces {
            0
        } else if piece_index == self.total_pieces - 1 {
            self.last_piece_size
        } else {
            self.piece_size
        }
    }

    /// Check if a piece index is valid for this torrent
    pub fn is_valid_piece(&self, piece_index: u32) -> bool {
        piece_index < self.total_pieces
    }
}

/// Centralized range calculation engine
#[derive(Debug, Clone)]
pub struct RangeCalculator {
    layout: TorrentLayout,
}

impl RangeCalculator {
    /// Create new range calculator for torrent with given layout
    pub fn new(layout: TorrentLayout) -> Self {
        Self { layout }
    }

    /// Create range calculator from basic torrent parameters
    pub fn from_params(piece_size: u32, total_pieces: u32, total_size: u64) -> Self {
        Self::new(TorrentLayout::new(piece_size, total_pieces, total_size))
    }

    /// Validate that a byte range is well-formed and within file bounds
    pub fn validate_range(&self, range: &Range<u64>) -> DataResult<()> {
        if range.start >= range.end {
            return Err(DataError::InvalidRange {
                start: range.start,
                end: range.end,
            });
        }

        if range.end > self.layout.total_size {
            return Err(DataError::RangeExceedsFile {
                start: range.start,
                end: range.end,
                file_size: self.layout.total_size,
            });
        }

        Ok(())
    }

    /// Calculate which pieces are needed for a byte range
    ///
    /// Returns a list of piece indices and the byte ranges within each piece
    /// that are needed to fulfill the request.
    pub fn pieces_for_range(&self, range: Range<u64>) -> DataResult<Vec<PieceRange>> {
        self.validate_range(&range)?;

        let start_piece = (range.start / self.layout.piece_size as u64) as u32;
        let end_byte = range.end - 1; // Convert to inclusive end
        let end_piece = (end_byte / self.layout.piece_size as u64) as u32;

        let mut pieces = Vec::new();

        for piece_index in start_piece..=end_piece.min(self.layout.total_pieces - 1) {
            let piece_start = piece_index as u64 * self.layout.piece_size as u64;
            let piece_end = piece_start + self.layout.piece_size(piece_index) as u64 - 1;

            let range_start_in_piece = if piece_index == start_piece {
                range.start - piece_start
            } else {
                0
            };

            let range_end_in_piece = if piece_index == end_piece {
                end_byte - piece_start
            } else {
                piece_end - piece_start
            };

            let priority = self.calculate_piece_priority(piece_index, &range);

            pieces.push(PieceRange {
                piece_index,
                start_offset: range_start_in_piece as u32,
                end_offset: range_end_in_piece as u32,
                priority,
            });
        }

        Ok(pieces)
    }

    /// Calculate optimal buffer requirements for streaming a byte range
    pub fn calculate_buffer_requirements(&self, range: Range<u64>) -> DataResult<BufferInfo> {
        self.validate_range(&range)?;

        let pieces = self.pieces_for_range(range.clone())?;
        let range_size = range.end - range.start;

        // Minimum buffer: just the requested range
        let minimum_buffer_size = range_size;

        // Recommended buffer: requested range + some prefetch
        let prefetch_factor = 1.5;
        let recommended_buffer_size = (range_size as f64 * prefetch_factor) as u64;

        // Critical pieces: needed for current playback
        let critical_pieces: Vec<u32> = pieces
            .iter()
            .filter(|p| matches!(p.priority, PiecePriority::Critical | PiecePriority::High))
            .map(|p| p.piece_index)
            .collect();

        // Prefetch pieces: additional pieces for smooth playback
        let prefetch_pieces = self.calculate_prefetch_pieces(&range, &pieces);

        Ok(BufferInfo {
            minimum_buffer_size,
            recommended_buffer_size,
            prefetch_pieces,
            critical_pieces,
        })
    }

    /// Calculate which byte offset corresponds to a specific piece and offset
    pub fn byte_offset_for_piece(&self, piece_index: u32, piece_offset: u32) -> Option<u64> {
        if !self.layout.is_valid_piece(piece_index) {
            return None;
        }

        let piece_size = self.layout.piece_size(piece_index);
        if piece_offset >= piece_size {
            return None;
        }

        let piece_start = piece_index as u64 * self.layout.piece_size as u64;
        Some(piece_start + piece_offset as u64)
    }

    /// Calculate which piece contains a specific byte offset
    pub fn piece_for_byte_offset(&self, byte_offset: u64) -> Option<(u32, u32)> {
        if byte_offset >= self.layout.total_size {
            return None;
        }

        let piece_index = (byte_offset / self.layout.piece_size as u64) as u32;
        let piece_offset = (byte_offset % self.layout.piece_size as u64) as u32;

        if self.layout.is_valid_piece(piece_index) {
            Some((piece_index, piece_offset))
        } else {
            None
        }
    }

    /// Calculate total bytes needed to fulfill all piece ranges
    pub fn total_bytes_for_pieces(&self, pieces: &[PieceRange]) -> u64 {
        pieces
            .iter()
            .map(|p| (p.end_offset - p.start_offset + 1) as u64)
            .sum()
    }

    /// Check if a byte range aligns with piece boundaries
    pub fn is_piece_aligned(&self, range: &Range<u64>) -> bool {
        let start_aligned = range.start % self.layout.piece_size as u64 == 0;
        let end_aligned = range.end % self.layout.piece_size as u64 == 0;
        start_aligned && end_aligned
    }

    /// Calculate piece priority based on position within requested range
    fn calculate_piece_priority(&self, piece_index: u32, range: &Range<u64>) -> PiecePriority {
        let piece_start = piece_index as u64 * self.layout.piece_size as u64;
        let piece_end = piece_start + self.layout.piece_size(piece_index) as u64;

        // Critical: piece is fully within the requested range
        if piece_start >= range.start && piece_end <= range.end {
            PiecePriority::Critical
        }
        // High: piece overlaps with the requested range
        else if piece_start < range.end && piece_end > range.start {
            PiecePriority::High
        }
        // Normal: piece is adjacent to the requested range
        else if (piece_end == range.start) || (piece_start == range.end) {
            PiecePriority::Normal
        }
        // Low: piece is nearby for prefetching
        else {
            PiecePriority::Low
        }
    }

    /// Calculate which pieces to prefetch for smooth streaming
    fn calculate_prefetch_pieces(
        &self,
        _range: &Range<u64>,
        current_pieces: &[PieceRange],
    ) -> Vec<u32> {
        let mut prefetch = Vec::new();

        if let Some(last_piece) = current_pieces.last() {
            let prefetch_count = 3; // Prefetch 3 pieces ahead
            let start_piece = last_piece.piece_index + 1;
            let end_piece = (start_piece + prefetch_count).min(self.layout.total_pieces);

            for piece_index in start_piece..end_piece {
                prefetch.push(piece_index);
            }
        }

        prefetch
    }
}

/// Create range calculator for HTTP streaming operations
pub fn create_range_calculator_for_info_hash(
    _info_hash: InfoHash,
    piece_size: u32,
    total_pieces: u32,
    total_size: u64,
) -> RangeCalculator {
    RangeCalculator::from_params(piece_size, total_pieces, total_size)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_layout() -> TorrentLayout {
        TorrentLayout::new(1024, 10, 9216) // 9 full pieces + 1 partial (128 bytes)
    }

    #[test]
    fn test_torrent_layout_creation() {
        let layout = create_test_layout();
        assert_eq!(layout.piece_size, 1024);
        assert_eq!(layout.total_pieces, 10);
        assert_eq!(layout.total_size, 9216);
        assert_eq!(layout.last_piece_size, 128);
    }

    #[test]
    fn test_piece_size_calculation() {
        let layout = create_test_layout();

        // Regular pieces
        assert_eq!(layout.piece_size(0), 1024);
        assert_eq!(layout.piece_size(8), 1024);

        // Last piece
        assert_eq!(layout.piece_size(9), 128);

        // Invalid piece
        assert_eq!(layout.piece_size(10), 0);
    }

    #[test]
    fn test_range_validation() {
        let calculator = RangeCalculator::new(create_test_layout());

        // Valid range
        assert!(calculator.validate_range(&(0..1024)).is_ok());
        assert!(calculator.validate_range(&(1000..2000)).is_ok());

        // Invalid range (start >= end)
        assert!(calculator.validate_range(&(1000..1000)).is_err());
        let start = 2000;
        let end = 1000;
        let invalid_range = start..end;
        assert!(calculator.validate_range(&invalid_range).is_err());

        // Range exceeds file size
        assert!(calculator.validate_range(&(1000..10000)).is_err());
    }

    #[test]
    fn test_pieces_for_range() {
        let calculator = RangeCalculator::new(create_test_layout());

        // Single piece range
        let pieces = calculator.pieces_for_range(100..200).unwrap();
        assert_eq!(pieces.len(), 1);
        assert_eq!(pieces[0].piece_index, 0);
        assert_eq!(pieces[0].start_offset, 100);
        assert_eq!(pieces[0].end_offset, 199);

        // Multi-piece range
        let pieces = calculator.pieces_for_range(1000..2000).unwrap();
        assert_eq!(pieces.len(), 2);
        assert_eq!(pieces[0].piece_index, 0);
        assert_eq!(pieces[0].start_offset, 1000);
        assert_eq!(pieces[0].end_offset, 1023);
        assert_eq!(pieces[1].piece_index, 1);
        assert_eq!(pieces[1].start_offset, 0);
        assert_eq!(pieces[1].end_offset, 975);
    }

    #[test]
    fn test_byte_offset_for_piece() {
        let calculator = RangeCalculator::new(create_test_layout());

        // First piece
        assert_eq!(calculator.byte_offset_for_piece(0, 0), Some(0));
        assert_eq!(calculator.byte_offset_for_piece(0, 100), Some(100));

        // Second piece
        assert_eq!(calculator.byte_offset_for_piece(1, 0), Some(1024));
        assert_eq!(calculator.byte_offset_for_piece(1, 100), Some(1124));

        // Invalid piece
        assert_eq!(calculator.byte_offset_for_piece(10, 0), None);

        // Invalid offset
        assert_eq!(calculator.byte_offset_for_piece(0, 2000), None);
    }

    #[test]
    fn test_piece_for_byte_offset() {
        let calculator = RangeCalculator::new(create_test_layout());

        // First piece
        assert_eq!(calculator.piece_for_byte_offset(0), Some((0, 0)));
        assert_eq!(calculator.piece_for_byte_offset(100), Some((0, 100)));
        assert_eq!(calculator.piece_for_byte_offset(1023), Some((0, 1023)));

        // Second piece
        assert_eq!(calculator.piece_for_byte_offset(1024), Some((1, 0)));
        assert_eq!(calculator.piece_for_byte_offset(1124), Some((1, 100)));

        // Last piece
        assert_eq!(calculator.piece_for_byte_offset(9215), Some((9, 127)));

        // Beyond file size
        assert_eq!(calculator.piece_for_byte_offset(10000), None);
    }

    #[test]
    fn test_piece_alignment() {
        let calculator = RangeCalculator::new(create_test_layout());

        // Aligned ranges
        assert!(calculator.is_piece_aligned(&(0..1024)));
        assert!(calculator.is_piece_aligned(&(1024..2048)));

        // Unaligned ranges
        assert!(!calculator.is_piece_aligned(&(100..1024)));
        assert!(!calculator.is_piece_aligned(&(0..1100)));
        assert!(!calculator.is_piece_aligned(&(100..1100)));
    }

    #[test]
    fn test_buffer_requirements() {
        let calculator = RangeCalculator::new(create_test_layout());

        let buffer_info = calculator.calculate_buffer_requirements(0..1024).unwrap();
        assert_eq!(buffer_info.minimum_buffer_size, 1024);
        assert_eq!(buffer_info.recommended_buffer_size, 1536); // 1024 * 1.5
        assert!(!buffer_info.critical_pieces.is_empty());
    }

    #[test]
    fn test_piece_priority_calculation() {
        let calculator = RangeCalculator::new(create_test_layout());
        let range = 1000..2000;

        // Piece 0 overlaps with range start
        let priority = calculator.calculate_piece_priority(0, &range);
        assert_eq!(priority, PiecePriority::High);

        // Piece 1 is fully within range
        let priority = calculator.calculate_piece_priority(1, &range);
        assert_eq!(priority, PiecePriority::Critical);

        // Piece 5 is far from range
        let priority = calculator.calculate_piece_priority(5, &range);
        assert_eq!(priority, PiecePriority::Low);
    }
}
