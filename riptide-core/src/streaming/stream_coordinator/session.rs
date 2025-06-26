//! Streaming session implementations and helpers

use std::time::{Duration, Instant};

use super::types::{StreamingBufferState, StreamingPerformanceMetrics, StreamingSession};
use crate::torrent::{InfoHash, PieceIndex};

impl StreamingSession {
    /// Create new streaming session.
    pub(super) fn new(info_hash: InfoHash, total_size: u64, position: u64) -> Self {
        let now = Instant::now();

        Self {
            info_hash,
            current_position: position,
            total_size,
            buffer_state: StreamingBufferState::new(),
            active_ranges: Vec::new(),
            session_start: now,
            last_activity: now,
            bytes_served: 0,
            performance_metrics: StreamingPerformanceMetrics::new(),
        }
    }
}

impl StreamingBufferState {
    pub(super) fn new() -> Self {
        Self {
            current_piece: 0,
            buffered_pieces: Vec::new(),
            critical_pieces: Vec::new(),
            prefetch_pieces: Vec::new(),
            buffer_health: 0.0,
        }
    }
}

impl StreamingPerformanceMetrics {
    pub(super) fn new() -> Self {
        Self {
            average_response_time: Duration::from_millis(0),
            throughput_mbps: 0.0,
            buffer_underruns: 0,
            seek_count: 0,
            total_requests: 0,
        }
    }
}

// Convert PieceIndex from u32 for compatibility
impl From<u32> for PieceIndex {
    fn from(index: u32) -> Self {
        PieceIndex::new(index)
    }
}
