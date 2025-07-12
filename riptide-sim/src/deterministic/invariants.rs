//! Invariant checking framework for simulation validation.

use std::fmt;
use std::time::Instant;

use super::resources::ResourceLimits;
use super::state::SimulationState;

/// Violation of a simulation invariant.
#[derive(Debug, Clone)]
pub struct InvariantViolation {
    /// Name of the violated invariant
    pub invariant: String,
    /// Detailed description of the violation
    pub description: String,
    /// When the violation occurred
    pub timestamp: Instant,
}

impl fmt::Display for InvariantViolation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Invariant '{}' violated at {:?}: {}",
            self.invariant, self.timestamp, self.description
        )
    }
}

/// Trait for checking simulation invariants.
pub trait Invariant: Send + Sync {
    /// Checks if invariant holds for current state.
    ///
    /// Returns error with violation details if invariant is violated.
    ///
    /// # Errors
    /// Returns `InvariantViolation` if the invariant condition is not met.
    fn check(&self, state: &SimulationState) -> Result<(), InvariantViolation>;

    /// Returns name of this invariant.
    fn name(&self) -> &str;
}

/// Ensures minimum number of connected peers.
pub struct MinimumPeersInvariant {
    min_peers: usize,
}

impl MinimumPeersInvariant {
    /// Creates invariant requiring minimum peer count.
    pub fn new(min_peers: usize) -> Self {
        Self { min_peers }
    }
}

impl Invariant for MinimumPeersInvariant {
    fn check(&self, state: &SimulationState) -> Result<(), InvariantViolation> {
        if state.connected_peers.len() < self.min_peers {
            return Err(InvariantViolation {
                invariant: self.name().to_string(),
                description: format!(
                    "Only {} peers connected, minimum {} required",
                    state.connected_peers.len(),
                    self.min_peers
                ),
                timestamp: Instant::now(),
            });
        }
        Ok(())
    }

    fn name(&self) -> &str {
        "MinimumPeers"
    }
}

/// Ensures resource usage stays within limits.
pub struct ResourceLimitInvariant {
    limits: ResourceLimits,
}

impl ResourceLimitInvariant {
    /// Creates invariant with specified resource limits.
    pub fn new(limits: ResourceLimits) -> Self {
        Self { limits }
    }
}

impl Invariant for ResourceLimitInvariant {
    fn check(&self, state: &SimulationState) -> Result<(), InvariantViolation> {
        if let Some((resource, current, limit)) = state.resource_usage.check_limits(&self.limits) {
            return Err(InvariantViolation {
                invariant: self.name().to_string(),
                description: format!("{resource} usage {current} exceeds limit {limit}"),
                timestamp: Instant::now(),
            });
        }
        Ok(())
    }

    fn name(&self) -> &str {
        "ResourceLimits"
    }
}

/// Ensures bandwidth throttling is respected.
pub struct BandwidthInvariant {
    max_download_rate: u64,
    max_upload_rate: u64,
}

impl BandwidthInvariant {
    /// Creates invariant with bandwidth limits in bytes per second.
    pub fn new(max_download_rate: u64, max_upload_rate: u64) -> Self {
        Self {
            max_download_rate,
            max_upload_rate,
        }
    }
}

impl Invariant for BandwidthInvariant {
    fn check(&self, state: &SimulationState) -> Result<(), InvariantViolation> {
        for (direction, rate) in &state.bandwidth_throttles {
            let limit = if direction.contains("download") {
                self.max_download_rate
            } else {
                self.max_upload_rate
            };

            if *rate > limit {
                return Err(InvariantViolation {
                    invariant: self.name().to_string(),
                    description: format!("{direction} rate {rate} exceeds limit {limit}"),
                    timestamp: Instant::now(),
                });
            }
        }
        Ok(())
    }

    fn name(&self) -> &str {
        "BandwidthLimits"
    }
}

/// Ensures streaming requirements are met.
/// Invariant ensuring streaming buffer requirements are met.
pub struct StreamingInvariant {
    min_buffer_pieces: usize,
    // TODO: Implement stall detection - track time without progress
    _max_stall_duration_ms: u64,
}

impl StreamingInvariant {
    /// Creates invariant for streaming performance.
    pub fn new(min_buffer_pieces: usize, max_stall_duration_ms: u64) -> Self {
        Self {
            min_buffer_pieces,
            _max_stall_duration_ms: max_stall_duration_ms,
        }
    }
}

impl Invariant for StreamingInvariant {
    fn check(&self, state: &SimulationState) -> Result<(), InvariantViolation> {
        // Check buffer health
        let sequential_pieces = count_sequential_pieces(&state.completed_pieces);
        if sequential_pieces < self.min_buffer_pieces {
            return Err(InvariantViolation {
                invariant: self.name().to_string(),
                description: format!(
                    "Only {} sequential pieces buffered, minimum {} required",
                    sequential_pieces, self.min_buffer_pieces
                ),
                timestamp: Instant::now(),
            });
        }
        Ok(())
    }

    fn name(&self) -> &str {
        "StreamingPerformance"
    }
}

/// Ensures data integrity constraints.
pub struct DataIntegrityInvariant {
    max_failure_rate: f64,
}

impl DataIntegrityInvariant {
    /// Creates invariant with maximum acceptable failure rate (0.0 to 1.0).
    pub fn new(max_failure_rate: f64) -> Self {
        Self { max_failure_rate }
    }
}

impl Invariant for DataIntegrityInvariant {
    fn check(&self, state: &SimulationState) -> Result<(), InvariantViolation> {
        let total_pieces = state.completed_pieces.len() + state.failed_pieces.len();
        if total_pieces == 0 {
            return Ok(());
        }

        let failure_rate = state.failed_pieces.len() as f64 / total_pieces as f64;
        if failure_rate > self.max_failure_rate {
            return Err(InvariantViolation {
                invariant: self.name().to_string(),
                description: format!(
                    "Failure rate {:.2}% exceeds maximum {:.2}%",
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

/// Helper function to count sequential pieces from start.
fn count_sequential_pieces(
    completed_pieces: &std::collections::HashSet<riptide_core::torrent::PieceIndex>,
) -> usize {
    let mut count = 0;
    while completed_pieces.contains(&riptide_core::torrent::PieceIndex::new(count as u32)) {
        count += 1;
    }
    count
}

#[cfg(test)]
mod tests {

    use super::*;

    #[test]
    fn test_minimum_peers_invariant() {
        let invariant = MinimumPeersInvariant::new(2);
        let mut state = SimulationState::new();

        // Should fail with no peers
        assert!(invariant.check(&state).is_err());

        // Should fail with one peer
        state.add_peer("PEER1".to_string());
        assert!(invariant.check(&state).is_err());

        // Should pass with two peers
        state.add_peer("PEER2".to_string());
        assert!(invariant.check(&state).is_ok());
    }

    #[test]
    fn test_resource_limit_invariant() {
        let limits = ResourceLimits {
            max_connections: 5,
            ..Default::default()
        };
        let invariant = ResourceLimitInvariant::new(limits);
        let mut state = SimulationState::new();

        // Should pass initially
        assert!(invariant.check(&state).is_ok());

        // Add connections up to limit
        for i in 0..5 {
            state.resource_usage.connections = i + 1;
            assert!(invariant.check(&state).is_ok());
        }

        // Exceed limit
        state.resource_usage.connections = 6;
        let result = invariant.check(&state);
        assert!(result.is_err());

        let violation = result.unwrap_err();
        assert!(violation.description.contains("Connections"));
    }

    #[test]
    fn test_data_integrity_invariant() {
        let invariant = DataIntegrityInvariant::new(0.1); // 10% max failure rate
        let mut state = SimulationState::new();

        // Complete some pieces
        for i in 0..9 {
            state.complete_piece(riptide_core::torrent::PieceIndex::new(i));
        }

        // Should pass with no failures
        assert!(invariant.check(&state).is_ok());

        // Add one failure (10% rate)
        state.fail_piece(
            riptide_core::torrent::PieceIndex::new(9),
            "Test failure".to_string(),
        );
        assert!(invariant.check(&state).is_ok());

        // Add another failure (exceeds 10%)
        state.fail_piece(
            riptide_core::torrent::PieceIndex::new(10),
            "Test failure".to_string(),
        );
        assert!(invariant.check(&state).is_err());
    }

    #[test]
    fn test_streaming_invariant() {
        let invariant = StreamingInvariant::new(5, 1000);
        let mut state = SimulationState::new();

        // Should fail with no pieces
        assert!(invariant.check(&state).is_err());

        // Complete sequential pieces
        for i in 0..5 {
            state.complete_piece(riptide_core::torrent::PieceIndex::new(i));
        }

        // Should pass with enough sequential pieces
        assert!(invariant.check(&state).is_ok());
    }
}
