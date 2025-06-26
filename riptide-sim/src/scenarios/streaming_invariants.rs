//! Custom invariants for streaming edge case scenarios.

use std::time::Instant;

use crate::{Invariant, InvariantViolation, SimulationState};

/// Invariant ensuring minimum streaming buffer is maintained.
pub struct StreamingBufferInvariant {
    min_buffer_pieces: usize,
}

impl StreamingBufferInvariant {
    pub fn new(min_buffer_pieces: usize) -> Self {
        Self { min_buffer_pieces }
    }
}

impl Invariant for StreamingBufferInvariant {
    fn check(&self, state: &SimulationState) -> Result<(), InvariantViolation> {
        let downloading = state.downloading_pieces.len();
        let completed = state.completed_pieces.len();

        if completed > 0 && downloading < self.min_buffer_pieces {
            return Err(InvariantViolation {
                invariant: self.name().to_string(),
                description: format!(
                    "Insufficient buffer: {} pieces downloading, need at least {}",
                    downloading, self.min_buffer_pieces
                ),
                timestamp: Instant::now(),
            });
        }

        Ok(())
    }

    fn name(&self) -> &str {
        "StreamingBuffer"
    }
}

/// Invariant ensuring data integrity thresholds.
pub struct DataIntegrityInvariant {
    max_failure_rate: f64,
}

impl DataIntegrityInvariant {
    pub fn new(max_failure_rate: f64) -> Self {
        Self { max_failure_rate }
    }
}

impl Invariant for DataIntegrityInvariant {
    fn check(&self, state: &SimulationState) -> Result<(), InvariantViolation> {
        let total = state.completed_pieces.len() + state.failed_pieces.len();
        if total == 0 {
            return Ok(());
        }

        let failure_rate = state.failed_pieces.len() as f64 / total as f64;

        if failure_rate > self.max_failure_rate {
            return Err(InvariantViolation {
                invariant: self.name().to_string(),
                description: format!(
                    "Data integrity failure rate {:.2}% exceeds maximum {:.2}%",
                    failure_rate * 100.0,
                    self.max_failure_rate * 100.0
                ),
                timestamp: Instant::now(),
            });
        }

        Ok(())
    }

    fn name(&self) -> &str {
        "DataIntegrity"
    }
}

/// Custom invariant ensuring maximum peer count for edge case testing.
pub struct MaxPeersInvariant {
    max_peers: usize,
}

impl MaxPeersInvariant {
    pub fn new(max_peers: usize) -> Self {
        Self { max_peers }
    }
}

impl Invariant for MaxPeersInvariant {
    fn check(&self, state: &SimulationState) -> Result<(), InvariantViolation> {
        if state.connected_peers.len() > self.max_peers {
            return Err(InvariantViolation {
                invariant: "MaxPeersInvariant".to_string(),
                description: format!(
                    "Too many peers connected: {} > {}",
                    state.connected_peers.len(),
                    self.max_peers
                ),
                timestamp: Instant::now(),
            });
        }
        Ok(())
    }

    fn name(&self) -> &str {
        "MaxPeers"
    }
}
