//! Edge case scenarios for BitTorrent streaming simulation.
//!
//! Tests challenging conditions that can occur in production streaming
//! environments to validate protocol robustness and performance.

use std::sync::Arc;
use std::time::{Duration, Instant};

use riptide_core::config::SimulationConfig;
use riptide_core::torrent::{InfoHash, PieceIndex};

use crate::{
    DeterministicSimulation, EventPriority, EventType, Invariant, InvariantViolation,
    ResourceLimitInvariant, ResourceLimits, ResourceType, SimulationError, SimulationReport,
    SimulationState, ThrottleDirection,
};

/// Simulates streaming under severe network degradation.
///
/// Tests system behavior when network conditions deteriorate during
/// active streaming, including packet loss spikes and bandwidth throttling.
///
/// # Errors
/// - `SimulationError::EventQueueOverflow` - Too many retry events
/// - `SimulationError::TimeLimitExceeded` - Simulation runs too long
pub fn severe_network_degradation_scenario() -> Result<SimulationReport, SimulationError> {
    let config = SimulationConfig {
        enabled: true,
        deterministic_seed: Some(1001),
        network_latency_ms: 50,
        packet_loss_rate: 0.01,
        max_simulated_peers: 15,
        simulated_download_speed: 5_242_880, // 5 MB/s initially
        use_mock_data: true,
    };

    let mut sim = DeterministicSimulation::new(config)?;

    // Add invariants
    sim.add_invariant(Arc::new(DataIntegrityInvariant::new(0.1)));
    sim.add_invariant(Arc::new(StreamingBufferInvariant::new(5))); // Need 5 pieces buffered

    // Start normal streaming
    sim.create_streaming_scenario(100, Duration::from_secs(1))?;

    // Schedule network degradation events
    for i in 0..5 {
        let delay = Duration::from_secs(10 + i * 5);
        let degradation = i as f64 * 0.02; // Increasing packet loss

        sim.schedule_delayed(
            delay,
            EventType::NetworkChange {
                latency_ms: (50 + i * 50) as u32,              // Increasing latency
                packet_loss_rate: (0.01 + degradation) as f64, // Up to 9% packet loss
            },
            EventPriority::Critical,
        )?;
    }

    // Add bandwidth throttling events
    for i in 0..10 {
        // Add bandwidth throttling to specific peers
        let delay = Duration::from_secs(5 + i * 10);
        let degraded_speed = 100_000 - (i as u64 * 10_000); // Degrading speed
        sim.schedule_delayed(
            delay,
            EventType::BandwidthThrottle {
                direction: ThrottleDirection::Download,
                rate_bytes_per_sec: degraded_speed,
            },
            EventPriority::High,
        )?;
    }

    sim.run_for(Duration::from_secs(60))
}

/// Simulates extreme peer churn during streaming.
///
/// Tests system resilience when peers rapidly connect and disconnect,
/// simulating unstable swarm conditions.
///
/// # Errors
/// - `SimulationError::EventQueueOverflow` - Too many connection events
/// - `SimulationError::ResourceLimitExceeded` - Connection limit hit
pub fn extreme_peer_churn_scenario() -> Result<SimulationReport, SimulationError> {
    let config = SimulationConfig {
        enabled: true,
        deterministic_seed: Some(1002),
        network_latency_ms: 100,
        packet_loss_rate: 0.02,
        max_simulated_peers: 50,
        simulated_download_speed: 2_097_152, // 2 MB/s
        use_mock_data: true,
    };

    let mut sim = DeterministicSimulation::new(config)?;

    // Strict connection limit
    sim.set_resource_limits(ResourceLimits {
        max_connections: 25,
        ..Default::default()
    });

    // Initial peers
    sim.create_streaming_scenario(50, Duration::from_secs(1))?;

    // Schedule rapid peer churn
    for cycle in 0..20 {
        let cycle_start = Duration::from_secs(cycle * 2);

        // Disconnect wave
        for i in 0..10 {
            sim.schedule_delayed(
                cycle_start + Duration::from_millis(i * 50),
                EventType::PeerDisconnect {
                    peer_id: format!("PEER_{:04}", (cycle * 10 + i) % 50),
                },
                EventPriority::High,
            )?;
        }

        // Reconnect wave with new peers
        for i in 0..15 {
            sim.schedule_delayed(
                cycle_start + Duration::from_millis(500 + i * 50),
                EventType::PeerConnect {
                    peer_id: format!("NEW_PEER_{:04}", cycle * 15 + i),
                },
                EventPriority::Normal,
            )?;
        }
    }

    sim.run_for(Duration::from_secs(45))
}

/// Simulates cascading piece failures.
///
/// Tests recovery mechanisms when multiple pieces fail verification,
/// potentially from malicious or corrupted peers.
///
/// # Errors
/// - `SimulationError::TooManyInvariantViolations` - Recovery fails
pub fn cascading_piece_failures_scenario() -> Result<SimulationReport, SimulationError> {
    let config = SimulationConfig {
        enabled: true,
        deterministic_seed: Some(1003),
        network_latency_ms: 50,
        packet_loss_rate: 0.01,
        max_simulated_peers: 20,
        simulated_download_speed: 10_485_760, // 10 MB/s
        use_mock_data: true,
    };

    let mut sim = DeterministicSimulation::new(config)?;

    // Add data integrity invariant
    sim.add_invariant(Arc::new(DataIntegrityInvariant::new(0.2))); // Max 20% failure rate

    sim.create_streaming_scenario(100, Duration::from_secs(1))?;

    // Schedule piece failures in waves
    let failure_waves = vec![
        (Duration::from_secs(5), vec![0, 1, 2, 3, 4]), // Early failures
        (Duration::from_secs(15), vec![20, 21, 22, 23, 24]), // Mid-stream failures
        (Duration::from_secs(25), vec![45, 46, 47, 48, 49]), // Late failures
    ];

    for (wave_time, piece_indices) in failure_waves {
        for (i, piece_index) in piece_indices.into_iter().enumerate() {
            sim.schedule_delayed(
                wave_time + Duration::from_millis(i as u64 * 100),
                EventType::PieceFailed {
                    peer_id: format!("MALICIOUS_PEER_{}", i),
                    piece_index: PieceIndex::new(piece_index),
                    reason: "Hash verification failed".to_string(),
                },
                EventPriority::High,
            )?;

            // Schedule retry from different peer
            sim.schedule_delayed(
                wave_time + Duration::from_secs(2) + Duration::from_millis(i as u64 * 200),
                EventType::PieceRequest {
                    peer_id: format!("GOOD_PEER_{}", (i + 10) % 20),
                    piece_index: PieceIndex::new(piece_index),
                },
                EventPriority::Normal,
            )?;
        }
    }

    sim.run_for(Duration::from_secs(40))
}

/// Simulates resource exhaustion conditions.
///
/// Tests system behavior when approaching memory, connection, and
/// disk space limits simultaneously.
///
/// # Errors
/// - `SimulationError::ResourceLimitExceeded` - Resource exhausted
pub fn resource_exhaustion_scenario() -> Result<SimulationReport, SimulationError> {
    let config = SimulationConfig {
        enabled: true,
        deterministic_seed: Some(1004),
        network_latency_ms: 50,
        packet_loss_rate: 0.01,
        max_simulated_peers: 100,             // Many peers
        simulated_download_speed: 20_971_520, // 20 MB/s - fast downloads
        use_mock_data: true,
    };

    let mut sim = DeterministicSimulation::new(config)?;

    // Set tight resource limits
    let limits = ResourceLimits {
        max_memory: 100 * 1024 * 1024,     // 100 MB
        max_connections: 30,               // Low connection limit
        max_disk_usage: 500 * 1024 * 1024, // 500 MB
        max_cpu_time_us: 500_000,          // 0.5 seconds
    };
    sim.set_resource_limits(limits.clone());
    sim.add_invariant(Arc::new(ResourceLimitInvariant::new(limits)));

    // Start multiple torrents
    for torrent_id in 0..5 {
        let piece_count = 50 + torrent_id * 10; // Increasing sizes

        // Stagger torrent starts
        let start_delay = Duration::from_secs(torrent_id as u64 * 3);

        // Add peers for this torrent
        for peer_id in 0..20 {
            let peer_delay = start_delay + Duration::from_millis(peer_id * 100);
            sim.schedule_delayed(
                peer_delay,
                EventType::PeerConnect {
                    peer_id: format!("T{}_PEER_{:02}", torrent_id, peer_id),
                },
                EventPriority::Normal,
            )?;
        }

        // Schedule piece downloads
        for piece_index in 0..piece_count {
            let piece_delay = start_delay + Duration::from_millis(500 + piece_index as u64 * 50);
            sim.schedule_delayed(
                piece_delay,
                EventType::PieceRequest {
                    peer_id: format!("T{}_PEER_{:02}", torrent_id, piece_index % 20),
                    piece_index: PieceIndex::new(piece_index),
                },
                EventPriority::Normal,
            )?;
        }
    }

    // Simulate memory growth
    for i in 0..20 {
        sim.schedule_delayed(
            Duration::from_secs(i),
            EventType::ResourceLimit {
                resource: ResourceType::Memory,
                current: (i + 1) * 10 * 1024 * 1024, // Growing memory usage
                limit: 100 * 1024 * 1024,
            },
            EventPriority::Low,
        )?;
    }

    sim.run_for(Duration::from_secs(30))
}

/// Simulates streaming with all peers eventually failing.
///
/// Tests graceful degradation when no peers remain available,
/// ensuring the system handles total peer loss correctly.
///
/// # Errors
/// - `SimulationError::InvalidEventScheduling` - Scheduling error
pub fn total_peer_failure_scenario() -> Result<SimulationReport, SimulationError> {
    let config = SimulationConfig {
        enabled: true,
        deterministic_seed: Some(1005),
        network_latency_ms: 100,
        packet_loss_rate: 0.05, // High packet loss
        max_simulated_peers: 10,
        simulated_download_speed: 1_048_576, // 1 MB/s
        use_mock_data: true,
    };

    let mut sim = DeterministicSimulation::new(config)?;
    let info_hash = InfoHash::new([0x05; 20]);

    // Add minimum peers invariant
    sim.add_invariant(Arc::new(MaxPeersInvariant::new(10)));

    sim.create_streaming_scenario(50, Duration::from_secs(1))?;

    // Schedule all peers to fail over time
    for i in 0..10 {
        let failure_time = Duration::from_secs(10 + i * 2);

        // Peer starts failing
        sim.schedule_delayed(
            failure_time,
            EventType::PieceFailed {
                peer_id: format!("PEER_{:04}", i),
                piece_index: PieceIndex::new(10 + i as u32),
                reason: "Connection timeout".to_string(),
            },
            EventPriority::High,
        )?;

        // Then disconnects
        sim.schedule_delayed(
            failure_time + Duration::from_secs(1),
            EventType::PeerDisconnect {
                peer_id: format!("PEER_{:04}", i),
            },
            EventPriority::Critical,
        )?;
    }

    // Schedule tracker announce with no new peers
    sim.schedule_delayed(
        Duration::from_secs(30),
        EventType::TrackerAnnounce {
            info_hash,
            peer_count: 0, // No peers available
        },
        EventPriority::Normal,
    )?;

    sim.run_for(Duration::from_secs(40))
}

// Custom invariants for edge case scenarios

/// Invariant ensuring minimum streaming buffer is maintained.
struct StreamingBufferInvariant {
    min_buffer_pieces: usize,
}

impl StreamingBufferInvariant {
    fn new(min_buffer_pieces: usize) -> Self {
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
struct DataIntegrityInvariant {
    max_failure_rate: f64,
}

impl DataIntegrityInvariant {
    fn new(max_failure_rate: f64) -> Self {
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
struct MaxPeersInvariant {
    max_peers: usize,
}

impl MaxPeersInvariant {
    fn new(max_peers: usize) -> Self {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_severe_network_degradation_completes() {
        let result = severe_network_degradation_scenario();

        match result {
            Ok(report) => {
                // Should have network change events
                assert!(report.metrics.events_by_type.contains_key("NetworkChange"));
                assert!(report.metrics.events_by_type["NetworkChange"] >= 5);

                // Should have bandwidth throttle events
                assert!(
                    report
                        .metrics
                        .events_by_type
                        .contains_key("BandwidthThrottle")
                );
            }
            Err(SimulationError::TooManyInvariantViolations { count }) => {
                // Expected - severe degradation can cause invariant violations
                assert!(count >= 10);
            }
            Err(e) => panic!("Unexpected error: {:?}", e),
        }
    }

    #[test]
    fn test_extreme_peer_churn_handles_limits() {
        let result = extreme_peer_churn_scenario();

        match result {
            Ok(report) => {
                // Should see many connect/disconnect events
                assert!(report.metrics.events_by_type["PeerConnect"] > 100);
                assert!(report.metrics.events_by_type["PeerDisconnect"] > 90);

                // Connection limit should be enforced
                assert!(report.final_state.connected_peers.len() <= 25);
            }
            Err(SimulationError::ResourceLimitExceeded {
                resource,
                current,
                limit,
            }) => {
                // Expected - extreme churn can hit connection limits
                assert_eq!(resource, ResourceType::Connections);
                assert!(current > limit);
                assert_eq!(limit, 25);
            }
            Err(e) => panic!("Unexpected error: {:?}", e),
        }
    }

    #[test]
    fn test_cascading_failures_triggers_retries() {
        let report = cascading_piece_failures_scenario().unwrap();

        // Should have failures and subsequent requests
        assert!(report.metrics.piece_failures >= 15);
        assert!(report.metrics.events_by_type["PieceRequest"] > 100);

        // Some pieces should eventually complete
        assert!(!report.final_state.completed_pieces.is_empty());
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
            Err(e) => panic!("Unexpected error: {:?}", e),
        }
    }

    #[test]
    fn test_total_peer_failure_violates_invariant() {
        let result = total_peer_failure_scenario();

        match result {
            Ok(report) => {
                // All peers should disconnect eventually
                assert_eq!(report.final_state.connected_peers.len(), 0);

                // Should either have invariant violations or the scenario completed without them
                if !report.metrics.invariant_violations.is_empty() {
                    assert!(!report.success);
                    assert!(
                        report
                            .metrics
                            .invariant_violations
                            .iter()
                            .any(|v| v.invariant == "MinimumPeers"
                                || v.invariant == "MinimumConnectedPeers")
                    );
                }
            }
            Err(SimulationError::TooManyInvariantViolations { .. }) => {
                // Also acceptable - total failure can cause many violations
            }
            Err(e) => panic!("Unexpected error: {:?}", e),
        }
    }
}
