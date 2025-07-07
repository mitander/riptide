//! Piece selection algorithms for torrent downloads.
//!
//! Provides strategies for choosing which pieces to download next.
//! Sequential picker prioritizes in-order download for streaming applications.
//! Adaptive picker responds to seek positions for better streaming experience.

use std::collections::HashMap;

use super::PieceIndex;

/// Priority levels for piece selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum PiecePriority {
    /// Low priority - background prefetch
    Low = 1,
    /// Normal priority - sequential download
    Normal = 2,
    /// High priority - near current position
    High = 3,
    /// Critical priority - immediately needed for playback
    Critical = 4,
    /// Urgent priority - explicit seek requests (overrides position-based priority)
    Urgent = 5,
}

/// Buffer status around current playback position.
#[derive(Debug, Clone)]
pub struct BufferStatus {
    /// Current playback position in bytes
    pub current_position: u64,
    /// Bytes available ahead of current position (buffered)
    pub bytes_ahead: u64,
    /// Bytes available behind current position
    pub bytes_behind: u64,
    /// Percentage of buffer filled (0.0 to 1.0)
    pub buffer_health: f64,
    /// Number of pieces in buffer
    pub pieces_in_buffer: u32,
    /// Estimated seconds of content buffered
    pub buffer_duration: f64,
}

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

/// Enhanced trait for streaming-aware piece selection.
///
/// Provides additional methods for position-based prioritization
/// and seek-ahead capabilities.
pub trait AdaptivePiecePicker: PiecePicker {
    /// Update the current playback position for prioritization.
    fn update_current_position(&mut self, byte_position: u64);

    /// Prioritize pieces in a specific range.
    fn prioritize_range(&mut self, start_piece: u32, end_piece: u32, priority: PiecePriority);

    /// Request immediate download of pieces for seek position.
    fn request_seek_position(&mut self, byte_position: u64, buffer_size: u64);

    /// Clear all priority overrides.
    fn clear_priorities(&mut self);

    /// Update buffer strategy based on playback progress.
    fn update_buffer_strategy(&mut self, playback_speed: f64, available_bandwidth: u64);

    /// Get current buffer status around playback position.
    fn buffer_status(&self) -> BufferStatus;
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

/// Adaptive piece picker that responds to playback position.
///
/// Prioritizes pieces based on current playback position and seek requests.
/// Maintains buffers around current position and handles seek-ahead scenarios.
pub struct AdaptiveStreamingPiecePicker {
    /// Total number of pieces in the torrent
    total_pieces: u32,
    /// Size of each piece in bytes
    piece_size: u32,
    /// Current playback position in bytes
    current_position: u64,
    /// Pieces that have been completed
    completed_pieces: Vec<bool>,
    /// Priority overrides for specific pieces
    piece_priorities: HashMap<u32, PiecePriority>,
    /// Forward buffer size (pieces ahead of current position)
    forward_buffer_size: u32,
    /// Backward buffer size (pieces behind current position)
    backward_buffer_size: u32,
    /// Sequential download starting index
    sequential_index: u32,
    /// Adaptive buffer sizing based on playback speed
    adaptive_buffer_enabled: bool,
    /// Base playback speed (1.0 = normal speed)
    playback_speed: f64,
    /// Available bandwidth estimate (bytes per second)
    estimated_bandwidth: u64,
    /// Minimum buffer size to maintain
    min_buffer_size: u32,
    /// Maximum buffer size to prevent over-buffering
    max_buffer_size: u32,
}

impl AdaptiveStreamingPiecePicker {
    /// Creates new adaptive streaming piece picker.
    pub fn new(total_pieces: u32, piece_size: u32) -> Self {
        Self {
            total_pieces,
            piece_size,
            current_position: 0,
            completed_pieces: vec![false; total_pieces as usize],
            piece_priorities: HashMap::new(),
            forward_buffer_size: 8,  // Default 8-piece forward buffer
            backward_buffer_size: 2, // Default 2-piece backward buffer
            sequential_index: 0,
            adaptive_buffer_enabled: true,
            playback_speed: 1.0,
            estimated_bandwidth: 1_000_000, // Default 1 MB/s
            min_buffer_size: 3,
            max_buffer_size: 20,
        }
    }

    /// Sets the buffer size around current position.
    pub fn with_buffer_size(mut self, forward_size: u32, backward_size: u32) -> Self {
        self.forward_buffer_size = forward_size;
        self.backward_buffer_size = backward_size;
        self
    }

    /// Enables or disables adaptive buffer sizing.
    pub fn with_adaptive_buffer(mut self, enabled: bool) -> Self {
        self.adaptive_buffer_enabled = enabled;
        self
    }

    /// Calculate which piece contains the given byte position.
    fn byte_to_piece(&self, byte_position: u64) -> u32 {
        (byte_position / self.piece_size as u64) as u32
    }

    /// Get the priority for a specific piece based on current position.
    ///
    /// This heuristic prioritizes pieces to ensure smooth playback. The strategy is:
    /// 1. Critical: The current piece and the ones immediately following it.
    /// 2. High: The first half of the forward buffer to stay ahead of playback.
    /// 3. Normal: The rest of the forward buffer and the immediate history for seeking backwards.
    /// 4. Low: All other pieces, which can be downloaded opportunistically.
    fn calculate_piece_priority(&self, piece_index: u32) -> PiecePriority {
        if let Some(&priority) = self.piece_priorities.get(&piece_index) {
            return priority;
        }

        let current_piece = self.byte_to_piece(self.current_position);

        let forward_buffer = if self.adaptive_buffer_enabled {
            self.calculate_adaptive_buffer_size()
        } else {
            self.forward_buffer_size
        };

        if piece_index == current_piece {
            PiecePriority::Critical
        } else if piece_index > current_piece {
            let distance = piece_index - current_piece;
            let near_buffer_threshold = forward_buffer / 2;

            if distance <= 2 {
                PiecePriority::Critical // Immediate next pieces
            } else if distance <= near_buffer_threshold {
                PiecePriority::High // Near buffer
            } else if distance <= forward_buffer {
                PiecePriority::Normal // Within buffer
            } else {
                PiecePriority::Low // Beyond buffer
            }
        } else {
            // Backward pieces (behind current position)
            let distance = current_piece - piece_index;
            if distance <= self.backward_buffer_size {
                PiecePriority::Normal // Recent pieces
            } else {
                PiecePriority::Low // Older pieces
            }
        }
    }

    /// Calculate adaptive buffer size based on playback speed and bandwidth.
    fn calculate_adaptive_buffer_size(&self) -> u32 {
        if !self.adaptive_buffer_enabled {
            return self.forward_buffer_size;
        }

        // Calculate buffer based on playback speed and available bandwidth
        // Higher playback speed = larger buffer needed
        // Lower bandwidth = smaller buffer to avoid stalling
        let speed_multiplier = (self.playback_speed * 1.5).max(1.0);
        let bandwidth_factor = (self.estimated_bandwidth as f64 / 1_000_000.0).clamp(0.5, 2.0);

        let adaptive_size =
            (self.forward_buffer_size as f64 * speed_multiplier * bandwidth_factor) as u32;
        adaptive_size.clamp(self.min_buffer_size, self.max_buffer_size)
    }

    /// Find the next piece to download based on priorities.
    fn find_next_priority_piece(&mut self) -> Option<PieceIndex> {
        let mut candidates: Vec<(u32, PiecePriority)> = Vec::new();

        // Collect all incomplete pieces with their priorities
        for piece_index in 0..self.total_pieces {
            if !self.completed_pieces[piece_index as usize] {
                let priority = self.calculate_piece_priority(piece_index);
                candidates.push((piece_index, priority));
            }
        }

        // Sort by priority (highest first), then by distance from current position
        candidates.sort_by(|a, b| {
            let priority_cmp = b.1.cmp(&a.1);
            if priority_cmp == std::cmp::Ordering::Equal {
                let current_piece = self.byte_to_piece(self.current_position);
                let distance_a = a.0.abs_diff(current_piece);
                let distance_b = b.0.abs_diff(current_piece);
                distance_a.cmp(&distance_b)
            } else {
                priority_cmp
            }
        });

        // Return the highest priority piece
        candidates.first().map(|(index, _)| PieceIndex::new(*index))
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
        // Implementation not needed for current streaming use case
    }
}

impl PiecePicker for AdaptiveStreamingPiecePicker {
    fn next_piece(&mut self) -> Option<PieceIndex> {
        if let Some(piece) = self.find_next_priority_piece() {
            return Some(piece);
        }
        while self.sequential_index < self.total_pieces {
            if !self.completed_pieces[self.sequential_index as usize] {
                let index = PieceIndex::new(self.sequential_index);
                self.sequential_index += 1;
                return Some(index);
            }
            self.sequential_index += 1;
        }

        None
    }

    fn mark_completed(&mut self, index: PieceIndex) {
        let piece_index = index.as_u32();
        if (piece_index as usize) < self.completed_pieces.len() {
            self.completed_pieces[piece_index as usize] = true;
        }
    }
}

impl AdaptivePiecePicker for AdaptiveStreamingPiecePicker {
    fn update_current_position(&mut self, byte_position: u64) {
        self.current_position = byte_position;
    }

    fn prioritize_range(&mut self, start_piece: u32, end_piece: u32, priority: PiecePriority) {
        for piece_index in start_piece..=end_piece.min(self.total_pieces - 1) {
            self.piece_priorities.insert(piece_index, priority);
        }
    }

    fn request_seek_position(&mut self, byte_position: u64, buffer_size: u64) {
        let seek_piece = self.byte_to_piece(byte_position);
        let buffer_pieces = (buffer_size / self.piece_size as u64) as u32;

        // Set urgent priority for immediate area around seek position
        let start_piece = seek_piece.saturating_sub(1);
        let end_piece = (seek_piece + buffer_pieces).min(self.total_pieces - 1);

        self.prioritize_range(start_piece, end_piece, PiecePriority::Urgent);

        // Update current position to seek position
        self.current_position = byte_position;
    }

    fn clear_priorities(&mut self) {
        self.piece_priorities.clear();
    }

    fn update_buffer_strategy(&mut self, playback_speed: f64, available_bandwidth: u64) {
        self.playback_speed = playback_speed;
        self.estimated_bandwidth = available_bandwidth;

        // Recalculate buffer sizes if adaptive buffering is enabled
        if self.adaptive_buffer_enabled {
            let new_buffer_size = self.calculate_adaptive_buffer_size();
            tracing::debug!(
                "Updated adaptive buffer: speed={:.1}x, bandwidth={}MB/s, buffer_size={}",
                playback_speed,
                available_bandwidth / 1_000_000,
                new_buffer_size
            );
        }
    }

    fn buffer_status(&self) -> BufferStatus {
        let current_piece = self.byte_to_piece(self.current_position);
        let effective_buffer_size = if self.adaptive_buffer_enabled {
            self.calculate_adaptive_buffer_size()
        } else {
            self.forward_buffer_size
        };

        // Count completed pieces in forward buffer
        let mut pieces_ahead = 0u64;
        let mut pieces_in_buffer = 0u32;

        for i in 1..=effective_buffer_size {
            let piece_index = current_piece + i;
            if piece_index < self.total_pieces {
                pieces_in_buffer += 1;
                if self.completed_pieces[piece_index as usize] {
                    pieces_ahead += self.piece_size as u64;
                }
            }
        }

        // Count completed pieces behind current position
        let mut pieces_behind = 0u64;
        for i in 1..=self.backward_buffer_size {
            if current_piece >= i {
                let piece_index = current_piece - i;
                if self.completed_pieces[piece_index as usize] {
                    pieces_behind += self.piece_size as u64;
                }
            }
        }

        // Calculate buffer health (0.0 to 1.0)
        let max_buffer_bytes = effective_buffer_size as u64 * self.piece_size as u64;
        let buffer_health = if max_buffer_bytes > 0 {
            pieces_ahead as f64 / max_buffer_bytes as f64
        } else {
            0.0
        };

        // Estimate buffer duration (assuming typical video bitrate)
        let estimated_bitrate = 2_000_000u64; // 2 Mbps estimate
        let buffer_duration = (pieces_ahead * 8) as f64 / estimated_bitrate as f64;

        BufferStatus {
            current_position: self.current_position,
            bytes_ahead: pieces_ahead,
            bytes_behind: pieces_behind,
            buffer_health,
            pieces_in_buffer,
            buffer_duration,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_adaptive_buffering_strategy() {
        let total_pieces = 100;
        let piece_size = 32768; // 32KB pieces
        let mut picker = AdaptiveStreamingPiecePicker::new(total_pieces, piece_size);

        // Disable adaptive buffering for predictable test results
        picker.adaptive_buffer_enabled = false;
        picker.forward_buffer_size = 8;

        // Set playback position to piece 10 (byte position ~327KB)
        let playback_position = 10 * piece_size as u64;
        picker.update_current_position(playback_position);

        // Test priority calculation around current position
        assert_eq!(picker.calculate_piece_priority(10), PiecePriority::Critical); // Current piece
        assert_eq!(picker.calculate_piece_priority(11), PiecePriority::Critical); // Next piece
        assert_eq!(picker.calculate_piece_priority(12), PiecePriority::Critical); // Next piece
        assert_eq!(picker.calculate_piece_priority(13), PiecePriority::High); // Near buffer (distance 3 <= 4)
        assert_eq!(picker.calculate_piece_priority(15), PiecePriority::Normal); // Within buffer (distance 5 > 4 but <= 8)
        assert_eq!(picker.calculate_piece_priority(25), PiecePriority::Low); // Beyond buffer (distance > 8)

        // Test backward pieces
        assert_eq!(picker.calculate_piece_priority(9), PiecePriority::Normal); // Recent (distance 1 <= 2)
        assert_eq!(picker.calculate_piece_priority(8), PiecePriority::Normal); // Recent (distance 2 <= 2)  
        assert_eq!(picker.calculate_piece_priority(7), PiecePriority::Low); // Older (distance 3 > 2)
    }

    #[test]
    fn test_adaptive_buffer_sizing() {
        let mut picker = AdaptiveStreamingPiecePicker::new(100, 32768);
        picker.adaptive_buffer_enabled = true;
        picker.max_buffer_size = 50; // Increase max to allow growth

        // Test normal playback speed with lower bandwidth to avoid clamping
        picker.update_buffer_strategy(1.0, 1_000_000); // 1 MB/s
        let normal_buffer = picker.calculate_adaptive_buffer_size();

        // Test higher playback speed should increase buffer
        picker.update_buffer_strategy(1.5, 1_000_000); // 1.5x speed, 1 MB/s
        let high_speed_buffer = picker.calculate_adaptive_buffer_size();
        assert!(high_speed_buffer > normal_buffer);

        // Test lower bandwidth should decrease buffer
        picker.update_buffer_strategy(1.0, 500_000); // 1x speed, 0.5 MB/s  
        let low_bandwidth_buffer = picker.calculate_adaptive_buffer_size();
        assert!(low_bandwidth_buffer < normal_buffer);

        // Test buffer stays within bounds
        picker.update_buffer_strategy(10.0, 10_000_000); // Very high values
        let clamped_buffer = picker.calculate_adaptive_buffer_size();
        assert!(clamped_buffer <= picker.max_buffer_size);

        picker.update_buffer_strategy(0.1, 100_000); // Very low values
        let min_clamped_buffer = picker.calculate_adaptive_buffer_size();
        assert!(min_clamped_buffer >= picker.min_buffer_size);
    }

    #[test]
    fn test_buffer_status_calculation() {
        let mut picker = AdaptiveStreamingPiecePicker::new(20, 32768);

        // Set current position to piece 5
        picker.update_current_position(5 * 32768);

        // Mark some pieces as completed
        picker.mark_completed(PieceIndex::new(4)); // Behind
        picker.mark_completed(PieceIndex::new(5)); // Current
        picker.mark_completed(PieceIndex::new(6)); // Ahead
        picker.mark_completed(PieceIndex::new(7)); // Ahead

        let status = picker.buffer_status();

        assert_eq!(status.current_position, 5 * 32768);
        assert_eq!(status.bytes_ahead, 2 * 32768); // Pieces 6 and 7
        assert_eq!(status.bytes_behind, 32768); // Piece 4
        assert!(status.buffer_health > 0.0);
        assert!(status.buffer_health <= 1.0);
        assert!(status.buffer_duration > 0.0);
    }

    #[test]
    fn test_seek_prioritization_with_buffering() {
        let mut picker = AdaptiveStreamingPiecePicker::new(100, 32768);

        // Start at beginning
        picker.update_current_position(0);

        // Seek to middle of file
        let seek_position = 50 * 32768;
        picker.request_seek_position(seek_position, 5 * 32768); // 5-piece buffer

        // Check that seek pieces have urgent priority
        assert_eq!(picker.calculate_piece_priority(49), PiecePriority::Urgent); // Before seek
        assert_eq!(picker.calculate_piece_priority(50), PiecePriority::Urgent); // Seek piece
        assert_eq!(picker.calculate_piece_priority(51), PiecePriority::Urgent); // After seek
        assert_eq!(picker.calculate_piece_priority(54), PiecePriority::Urgent); // Buffer end

        // Other pieces should have lower priority
        assert_eq!(picker.calculate_piece_priority(10), PiecePriority::Low); // Far behind
        assert_eq!(picker.calculate_piece_priority(80), PiecePriority::Low); // Far ahead
    }

    #[test]
    fn test_priority_piece_selection() {
        let mut picker = AdaptiveStreamingPiecePicker::new(20, 32768);

        // Set current position to piece 5
        picker.update_current_position(5 * 32768);

        // Request urgent piece at position 10
        picker.prioritize_range(10, 12, PiecePriority::Urgent);

        // Next piece should be urgent piece, not current position
        if let Some(next_piece) = picker.next_piece() {
            assert_eq!(next_piece.as_u32(), 10);
        } else {
            panic!("Expected next piece to be selected");
        }

        // Mark piece 10 completed
        picker.mark_completed(PieceIndex::new(10));

        // Next should be piece 11 (also urgent)
        if let Some(next_piece) = picker.next_piece() {
            assert_eq!(next_piece.as_u32(), 11);
        } else {
            panic!("Expected next piece to be selected");
        }
    }

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

    #[test]
    fn test_adaptive_picker_prioritizes_current_position() {
        let piece_size = 1024 * 1024; // 1MB pieces
        let mut picker = AdaptiveStreamingPiecePicker::new(10, piece_size);

        // Set current position to middle of piece 5
        picker.update_current_position(5 * piece_size as u64 + 512 * 1024);

        // Should prioritize pieces around position 5
        let next = picker.next_piece().unwrap();
        assert!(next.as_u32() >= 4 && next.as_u32() <= 6);
    }

    #[test]
    fn test_adaptive_picker_seek_position() {
        let piece_size = 1024 * 1024; // 1MB pieces
        let mut picker = AdaptiveStreamingPiecePicker::new(10, piece_size);

        // Request seek to piece 7
        picker.request_seek_position(7 * piece_size as u64, 2 * piece_size as u64);

        // Should prioritize pieces around seek position
        let next = picker.next_piece().unwrap();
        assert!(next.as_u32() >= 6 && next.as_u32() <= 8);
    }

    #[test]
    fn test_adaptive_picker_mark_completed() {
        let mut picker = AdaptiveStreamingPiecePicker::new(5, 1024);

        picker.mark_completed(PieceIndex::new(2));
        assert!(picker.completed_pieces[2]);

        // Completed pieces should not be returned
        for _ in 0..20 {
            if let Some(piece) = picker.next_piece() {
                assert_ne!(piece.as_u32(), 2);
            }
        }
    }

    #[test]
    fn test_adaptive_picker_priority_range() {
        let piece_size = 1024;
        let mut picker = AdaptiveStreamingPiecePicker::new(10, piece_size);

        // Set current position far from the priority range to avoid interference
        picker.update_current_position(8 * piece_size as u64);

        // Prioritize pieces 3-5 as urgent (higher than any position-based priority)
        picker.prioritize_range(3, 5, PiecePriority::Urgent);

        // Should get pieces from prioritized range first
        let next = picker.next_piece().unwrap();
        assert!(
            next.as_u32() >= 3 && next.as_u32() <= 5,
            "Got piece {}, expected 3-5",
            next.as_u32()
        );
    }

    #[test]
    fn test_adaptive_picker_clear_priorities() {
        let mut picker = AdaptiveStreamingPiecePicker::new(10, 1024);

        picker.prioritize_range(3, 5, PiecePriority::Critical);
        picker.clear_priorities();

        // After clearing, should fall back to sequential
        assert_eq!(picker.next_piece(), Some(PieceIndex::new(0)));
    }
}
