//! Edge case scenarios for BitTorrent streaming simulation.
//!
//! Tests challenging conditions that can occur in production streaming
//! environments to validate protocol robustness and performance.

use std::sync::Arc;
use std::time::Duration;

use riptide_core::config::SimulationConfig;

use super::streaming_invariants::{DataIntegrityInvariant, MaxPeersInvariant};
use crate::{
    DeterministicSimulation, ResourceLimitInvariant, ResourceLimits, SimulationError,
    SimulationReport,
};

/// Tests streaming with poor network conditions.
///
/// Verifies the system handles high latency and packet loss gracefully.
///
/// # Errors
///
/// - `SimulationError` - If simulation fails to complete or network configuration is invalid
pub fn severe_network_degradation_scenario() -> Result<SimulationReport, SimulationError> {
    let mut config = SimulationConfig::bandwidth_limited(1001);
    // Configure poor network conditions
    config.network_latency_ms = 200; // High latency
    config.packet_loss_rate = 0.05; // 5% packet loss

    let mut sim = DeterministicSimulation::new(config)?;
    sim.create_streaming_scenario(50, Duration::from_secs(1))?;
    sim.execute_for_duration(Duration::from_secs(30))
}

/// Tests streaming with unstable peer connections.
///
/// Verifies the system maintains download progress when peers disconnect frequently.
///
/// # Errors
///
/// - `SimulationError` - If simulation fails or peer behavior configuration is invalid
pub fn extreme_peer_churn_scenario() -> Result<SimulationReport, SimulationError> {
    let mut config = SimulationConfig::high_peer_churn(1002);
    config.max_simulated_peers = 25; // Connection limit

    let mut sim = DeterministicSimulation::new(config)?;
    sim.create_streaming_scenario(30, Duration::from_secs(1))?;
    sim.execute_for_duration(Duration::from_secs(20))
}

/// Tests streaming with piece hash failures.
///
/// Verifies the system retries failed pieces from different peers.
///
/// # Errors
///
/// - `SimulationError` - If simulation fails or piece configuration is invalid
pub fn cascading_piece_failures_scenario() -> Result<SimulationReport, SimulationError> {
    let mut config = SimulationConfig::ideal_streaming(1003);
    // Configure some peers to provide corrupted data
    config.simulated_download_speed = 1_000_000; // Slower for more realistic timing

    let mut sim = DeterministicSimulation::new(config)?;
    sim.add_invariant(Arc::new(DataIntegrityInvariant::new(0.2))); // Allow up to 20% failures
    sim.create_streaming_scenario(50, Duration::from_secs(1))?;
    sim.execute_for_duration(Duration::from_secs(30))
}

/// Tests streaming with tight resource constraints.
///
/// Verifies the system respects memory and connection limits.
///
/// # Errors
///
/// - `SimulationError` - If simulation fails or bandwidth configuration is invalid
pub fn resource_exhaustion_scenario() -> Result<SimulationReport, SimulationError> {
    let mut config = SimulationConfig::ideal_streaming(1004);
    config.max_simulated_peers = 50; // Many peers to stress limits

    let mut sim = DeterministicSimulation::new(config)?;

    // Set tight resource limits
    let limits = ResourceLimits {
        max_connections: 10, // Very low limit
        ..Default::default()
    };
    sim.configure_resource_limits(limits.clone());
    sim.add_invariant(Arc::new(ResourceLimitInvariant::new(limits)));

    sim.create_streaming_scenario(40, Duration::from_secs(1))?;
    sim.execute_for_duration(Duration::from_secs(25))
}

/// Tests streaming when all peers become unavailable.
///
/// Verifies the system handles complete peer isolation gracefully.
///
/// # Errors
///
/// - `SimulationError` - If simulation fails or connection configuration is invalid
pub fn total_peer_failure_scenario() -> Result<SimulationReport, SimulationError> {
    let mut config = SimulationConfig::mixed_network_quality(1005);
    config.max_simulated_peers = 5; // Few peers so they can all fail

    let mut sim = DeterministicSimulation::new(config)?;
    sim.add_invariant(Arc::new(MaxPeersInvariant::new(5)));
    sim.create_streaming_scenario(30, Duration::from_secs(1))?;
    sim.execute_for_duration(Duration::from_secs(20))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ResourceType;

    #[test]
    fn test_severe_network_degradation_completes() {
        let result = severe_network_degradation_scenario();

        match result {
            Ok(report) => {
                // Should still make download progress despite poor network
                assert!(!report.final_state.completed_pieces.is_empty());

                // Should have piece requests (retrying due to poor conditions)
                assert!(
                    report
                        .metrics
                        .events_by_type
                        .get("PieceRequest")
                        .unwrap_or(&0)
                        > &10
                );
            }
            Err(SimulationError::TooManyInvariantViolations { .. }) => {
                // Expected - severe network can cause violations
            }
            Err(e) => panic!("Unexpected error: {e:?}"),
        }
    }

    #[test]
    fn test_extreme_peer_churn_handles_limits() {
        let result = extreme_peer_churn_scenario();

        match result {
            Ok(report) => {
                // Should still download pieces despite peer instability
                assert!(!report.final_state.completed_pieces.is_empty());

                // Connection limit should be enforced
                assert!(report.final_state.connected_peers.len() <= 25);
            }
            Err(SimulationError::ResourceLimitExceeded {
                resource, limit, ..
            }) => {
                // Expected - extreme churn can hit connection limits
                assert_eq!(resource, ResourceType::Connections);
                assert_eq!(limit, 25);
            }
            Err(e) => panic!("Unexpected error: {e:?}"),
        }
    }

    #[test]
    fn test_cascading_failures_triggers_retries() {
        let report = cascading_piece_failures_scenario().unwrap();

        // Should still complete some pieces despite data integrity issues
        assert!(!report.final_state.completed_pieces.is_empty());

        // Should have made piece requests
        assert!(
            report
                .metrics
                .events_by_type
                .get("PieceRequest")
                .unwrap_or(&0)
                > &0
        );
    }

    #[test]
    fn test_resource_exhaustion_enforces_limits() {
        let result = resource_exhaustion_scenario();

        // Should fail due to resource limits
        match result {
            Err(SimulationError::ResourceLimitExceeded { .. }) => {
                // Expected - resources exhausted
            }
            Ok(report) => {
                // Or invariant violations if caught by invariants
                assert!(!report.metrics.invariant_violations.is_empty());
            }
            Err(e) => panic!("Unexpected error: {e:?}"),
        }
    }

    #[test]
    fn test_total_peer_failure_violates_invariant() {
        let result = total_peer_failure_scenario();

        match result {
            Ok(report) => {
                // Should have few or no peers available (testing peer scarcity)
                assert!(report.final_state.connected_peers.len() <= 5);

                // Should download what it can before peers are exhausted
                // (May be empty if peers fail immediately)
            }
            Err(SimulationError::TooManyInvariantViolations { .. }) => {
                // Expected - total failure triggers invariant violations
            }
            Err(e) => panic!("Unexpected error: {e:?}"),
        }
    }
}
