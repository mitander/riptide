//! Core simulation engine for deterministic testing.

use std::collections::BinaryHeap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use riptide_core::config::SimulationConfig;
use riptide_core::torrent::{InfoHash, PieceIndex};
use thiserror::Error;

use super::clock::{DeterministicClock, DeterministicRng};
use super::events::{EventPriority, EventType, SimulationEvent};
use super::invariants::Invariant;
use super::resources::{ResourceLimits, ResourceType, ResourceUsage};
use super::state::{SimulationMetrics, SimulationState};

/// Maximum number of events that can be scheduled.
const MAX_EVENT_QUEUE_SIZE: usize = 100_000;

/// Maximum number of invariant violations before stopping simulation.
const MAX_INVARIANT_VIOLATIONS: usize = 10;

/// Maximum number of events to track in history.
const MAX_EVENT_HISTORY: usize = 10_000;

/// Errors that can occur during simulation.
#[derive(Debug, Error)]
pub enum SimulationError {
    /// Event queue exceeded maximum capacity
    #[error("Event queue overflow: {count} events scheduled")]
    EventQueueOverflow {
        /// Number of events that caused overflow
        count: usize,
    },

    /// Simulation ran longer than allowed time limit
    #[error("Simulation time limit exceeded: {elapsed:?}")]
    TimeLimitExceeded {
        /// Time that elapsed before timeout
        elapsed: Duration,
    },

    /// Resource usage exceeded configured limit
    #[error("Resource limit exceeded: {resource:?} usage {current} > limit {limit}")]
    ResourceLimitExceeded {
        /// Type of resource that exceeded limit
        resource: ResourceType,
        /// Current usage amount
        current: u64,
        /// Maximum allowed limit
        limit: u64,
    },

    /// Too many invariant violations occurred
    #[error("Too many invariant violations: {count}")]
    TooManyInvariantViolations {
        /// Number of violations that occurred
        count: usize,
    },

    /// Event could not be scheduled properly
    #[error("Invalid event scheduling: {reason}")]
    InvalidEventScheduling {
        /// Reason why scheduling failed
        reason: String,
    },

    /// Deterministic seed required but not provided
    #[error("No deterministic seed provided")]
    NoDeterministicSeed,
}

/// Result of a simulation run.
#[derive(Debug, Clone)]
pub struct SimulationReport {
    /// Seed used for reproduction
    pub seed: u64,
    /// Total simulation duration
    pub duration: Duration,
    /// Collected metrics
    pub metrics: SimulationMetrics,
    /// Final simulation state
    pub final_state: SimulationState,
    /// Total events processed
    pub event_count: u64,
    /// Whether simulation completed successfully
    pub success: bool,
}

impl SimulationReport {
    /// Generates human-readable summary.
    pub fn summary(&self) -> String {
        let mut summary = String::new();
        summary.push_str(&format!("Simulation Report (seed: {})\n", self.seed));
        summary.push_str(&format!("Duration: {:?}\n", self.duration));
        summary.push_str(&format!("Events processed: {}\n", self.event_count));
        summary.push_str(&format!("Success: {}\n", self.success));
        summary.push_str("\nEvent breakdown:\n");

        for (event_type, count) in &self.metrics.events_by_type {
            summary.push_str(&format!("  {event_type}: {count}\n"));
        }

        if !self.metrics.invariant_violations.is_empty() {
            summary.push_str("\nInvariant violations:\n");
            for violation in &self.metrics.invariant_violations {
                summary.push_str(&format!("  - {violation}\n"));
            }
        }

        summary.push_str(&format!(
            "\nFinal state:\n  Connected peers: {}\n  Completed pieces: {}\n  Failed pieces: {}\n",
            self.final_state.connected_peers.len(),
            self.final_state.completed_pieces.len(),
            self.final_state.failed_pieces.len()
        ));

        summary
    }
}

/// Deterministic simulation engine for testing BitTorrent scenarios.
pub struct DeterministicSimulation {
    /// Configuration
    config: SimulationConfig,
    /// Controlled clock
    clock: DeterministicClock,
    /// Deterministic RNG
    rng: DeterministicRng,
    /// Event queue (min-heap by timestamp)
    event_queue: BinaryHeap<SimulationEvent>,
    /// Next event ID
    next_event_id: u64,
    /// Current simulation state
    state: SimulationState,
    /// Metrics collector
    metrics: SimulationMetrics,
    /// Resource limits
    resource_limits: ResourceLimits,
    /// Resource usage tracker
    resource_usage: ResourceUsage,
    /// Active invariants
    invariants: Vec<Arc<dyn Invariant>>,
    /// Event history for debugging
    event_history: Vec<SimulationEvent>,
}

impl DeterministicSimulation {
    /// Creates new simulation with given configuration.
    ///
    /// # Errors
    /// - `SimulationError::NoDeterministicSeed` - No seed provided in config
    pub fn new(config: SimulationConfig) -> Result<Self, SimulationError> {
        let seed = config
            .deterministic_seed
            .ok_or(SimulationError::NoDeterministicSeed)?;

        Ok(Self {
            config,
            clock: DeterministicClock::new(),
            rng: DeterministicRng::from_seed(seed),
            event_queue: BinaryHeap::new(),
            next_event_id: 0,
            state: SimulationState::new(),
            metrics: SimulationMetrics::new(),
            resource_limits: ResourceLimits::default(),
            resource_usage: ResourceUsage::new(),
            invariants: Vec::new(),
            event_history: Vec::new(),
        })
    }

    /// Returns the seed used for this simulation.
    pub fn simulation_seed(&self) -> u64 {
        self.rng.seed()
    }

    /// Returns current simulation time.
    pub fn simulation_time(&self) -> Instant {
        self.clock.now()
    }

    /// Returns elapsed simulation time.
    pub fn simulation_elapsed(&self) -> Duration {
        self.clock.elapsed()
    }

    /// Sets resource limits for simulation.
    pub fn configure_resource_limits(&mut self, limits: ResourceLimits) {
        self.resource_limits = limits;
    }

    /// Adds an invariant to check during simulation.
    pub fn add_invariant(&mut self, invariant: Arc<dyn Invariant>) {
        self.invariants.push(invariant);
    }

    /// Schedules an event to occur immediately.
    ///
    /// # Errors
    /// - `SimulationError::EventQueueOverflow` - Too many events scheduled
    pub fn schedule_event(
        &mut self,
        event_type: EventType,
        priority: EventPriority,
    ) -> Result<(), SimulationError> {
        self.schedule_delayed(Duration::ZERO, event_type, priority)
    }

    /// Gets reference to the deterministic clock.
    pub fn deterministic_clock(&self) -> &DeterministicClock {
        &self.clock
    }

    /// Schedules an event to occur after specified delay.
    ///
    /// # Errors
    /// - `SimulationError::EventQueueOverflow` - Event queue is full
    pub fn schedule_delayed(
        &mut self,
        delay: Duration,
        event_type: EventType,
        priority: EventPriority,
    ) -> Result<(), SimulationError> {
        if self.event_queue.len() >= MAX_EVENT_QUEUE_SIZE {
            return Err(SimulationError::EventQueueOverflow {
                count: self.event_queue.len(),
            });
        }

        let event = SimulationEvent::new(
            self.next_event_id,
            self.clock.now() + delay,
            event_type,
            priority,
        );

        self.next_event_id += 1;
        self.event_queue.push(event);

        Ok(())
    }

    /// Runs simulation for specified duration.
    ///
    /// # Errors
    /// - `SimulationError::TimeLimitExceeded` - Simulation time limit exceeded
    /// - `SimulationError::ResourceLimitExceeded` - Resource limit exceeded
    /// - `SimulationError::TooManyInvariantViolations` - Too many invariant violations
    pub fn execute_for_duration(
        &mut self,
        duration: Duration,
    ) -> Result<SimulationReport, SimulationError> {
        let target_time = self.clock.now() + duration;
        self.execute_until(target_time)
    }

    /// Runs simulation until target time.
    ///
    /// # Errors
    /// - `SimulationError::TimeLimitExceeded` - Simulation time limit exceeded
    /// - `SimulationError::ResourceLimitExceeded` - Resource limit exceeded
    /// - `SimulationError::TooManyInvariantViolations` - Too many invariant violations
    pub fn execute_until(
        &mut self,
        target_time: Instant,
    ) -> Result<SimulationReport, SimulationError> {
        // TODO: Track simulation execution time for performance metrics
        // let start_time = self.clock.now();

        while let Some(event) = self.event_queue.pop() {
            if event.timestamp > target_time {
                self.event_queue.push(event);
                break;
            }

            self.clock.advance_to(event.timestamp)?;
            self.process_event(&event)?;

            if self.event_history.len() < MAX_EVENT_HISTORY {
                self.event_history.push(event.clone());
            }

            self.metrics.record_event(event.event_type.as_str());
            self.metrics
                .update_peak_connections(self.state.connected_peers.len());

            self.check_invariants()?;
            self.check_resource_limits()?;
        }

        Ok(self.generate_report())
    }

    /// Processes a single event.
    fn process_event(&mut self, event: &SimulationEvent) -> Result<(), SimulationError> {
        self.resource_usage.update_for_event(&event.event_type);

        match &event.event_type {
            EventType::PeerConnect { peer_id } => {
                self.state.add_peer(peer_id.clone());
            }
            EventType::PeerDisconnect { peer_id } => {
                self.state.remove_peer(peer_id);
                self.metrics.increment_peer_disconnects();
            }
            EventType::PieceRequest {
                peer_id,
                piece_index,
            } => {
                self.state
                    .start_piece_download(*piece_index, peer_id.clone());
            }
            EventType::PieceComplete {
                piece_index,
                download_time,
                ..
            } => {
                self.state.complete_piece(*piece_index);
                self.metrics.record_piece_complete(*download_time);
            }
            EventType::PieceFailed {
                piece_index,
                reason,
                ..
            } => {
                self.state.fail_piece(*piece_index, reason.clone());
                self.metrics.record_piece_failure();
            }
            EventType::NetworkChange { .. } => {}
            EventType::BandwidthThrottle {
                direction,
                rate_bytes_per_sec,
            } => {
                self.state
                    .bandwidth_throttles
                    .insert(format!("{direction:?}"), *rate_bytes_per_sec);
            }
            EventType::ResourceLimit {
                resource,
                current,
                limit,
            } => {
                return Err(SimulationError::ResourceLimitExceeded {
                    resource: *resource,
                    current: *current,
                    limit: *limit,
                });
            }
            _ => {}
        }

        Ok(())
    }

    /// Checks all invariants.
    fn check_invariants(&mut self) -> Result<(), SimulationError> {
        for invariant in &self.invariants {
            if let Err(violation) = invariant.check(&self.state) {
                self.metrics.record_invariant_violation(violation);

                if self.metrics.invariant_violations.len() >= MAX_INVARIANT_VIOLATIONS {
                    return Err(SimulationError::TooManyInvariantViolations {
                        count: self.metrics.invariant_violations.len(),
                    });
                }
            }
        }
        Ok(())
    }

    /// Checks resource limits are not exceeded.
    fn check_resource_limits(&self) -> Result<(), SimulationError> {
        if self.resource_usage.memory > self.resource_limits.max_memory {
            return Err(SimulationError::ResourceLimitExceeded {
                resource: ResourceType::Memory,
                current: self.resource_usage.memory as u64,
                limit: self.resource_limits.max_memory as u64,
            });
        }

        if self.resource_usage.connections > self.resource_limits.max_connections {
            return Err(SimulationError::ResourceLimitExceeded {
                resource: ResourceType::Connections,
                current: self.resource_usage.connections as u64,
                limit: self.resource_limits.max_connections as u64,
            });
        }

        if self.resource_usage.disk_usage > self.resource_limits.max_disk_usage {
            return Err(SimulationError::ResourceLimitExceeded {
                resource: ResourceType::DiskSpace,
                current: self.resource_usage.disk_usage,
                limit: self.resource_limits.max_disk_usage,
            });
        }

        Ok(())
    }

    /// Generates simulation report.
    fn generate_report(&self) -> SimulationReport {
        SimulationReport {
            seed: self.simulation_seed(),
            duration: self.clock.elapsed(),
            metrics: self.metrics.clone(),
            final_state: self.state.clone(),
            event_count: self.metrics.events_processed,
            success: self.metrics.invariant_violations.is_empty(),
        }
    }

    /// Creates a simple streaming scenario with default parameters.
    ///
    /// This is a convenience method that uses sensible defaults for streaming tests.
    ///
    /// # Errors
    /// - `SimulationError::InvalidEventScheduling` - Failed to schedule simulation events
    pub fn create_streaming_scenario(
        &mut self,
        total_pieces: u32,
        _playback_start_delay: Duration, // For future use
    ) -> Result<(), SimulationError> {
        // Use default info hash and piece size for simple scenarios
        let info_hash = InfoHash::new([0x42; 20]); // Arbitrary but deterministic
        let piece_size = 262144; // 256 KB default

        self.create_streaming_scenario_full(info_hash, total_pieces, piece_size)
    }

    /// Creates a streaming scenario with full parameters.
    ///
    /// # Errors
    /// - `SimulationError::InvalidEventScheduling` - Failed to schedule simulation events
    pub fn create_streaming_scenario_full(
        &mut self,
        info_hash: InfoHash,
        total_pieces: u32,
        piece_size: u32,
    ) -> Result<(), SimulationError> {
        self.schedule_delayed(
            Duration::from_secs(1),
            EventType::TrackerAnnounce {
                info_hash,
                peer_count: self.config.max_simulated_peers,
            },
            EventPriority::High,
        )?;

        for i in 0..self.config.max_simulated_peers {
            let connect_delay = Duration::from_millis(100 * i as u64);
            self.schedule_delayed(
                connect_delay,
                EventType::PeerConnect {
                    peer_id: format!("PEER_{i:04}"),
                },
                EventPriority::Normal,
            )?;
        }

        for piece_index in 0..total_pieces {
            let piece_delay = Duration::from_millis(500 + piece_index as u64 * 100);
            let peer_index = (piece_index as usize) % self.config.max_simulated_peers;

            self.schedule_delayed(
                piece_delay,
                EventType::PieceRequest {
                    peer_id: format!("PEER_{peer_index:04}"),
                    piece_index: PieceIndex::new(piece_index),
                },
                EventPriority::Normal,
            )?;

            // A minimum download time is enforced to prevent division-by-zero with infinite bandwidth
            // and to ensure simulation events have a non-zero duration, which helps avoid scheduling conflicts.
            let bytes_per_second = self.config.simulated_download_speed;
            let download_seconds = piece_size as f64 / bytes_per_second as f64;
            let download_time = Duration::from_secs_f64(download_seconds.max(0.001)); // Remove artificial 100ms minimum that limits speed
            self.schedule_delayed(
                piece_delay + download_time,
                EventType::PieceComplete {
                    peer_id: format!("PEER_{peer_index:04}"),
                    piece_index: PieceIndex::new(piece_index),
                    download_time,
                },
                EventPriority::Normal,
            )?;
        }

        Ok(())
    }

    /// Adds a deterministic peer to the simulation.
    ///
    /// # Errors
    /// - `SimulationError::InvalidEventScheduling` - Failed to schedule peer connection event
    pub fn add_deterministic_peer(
        &mut self,
        peer_id: String,
        behavior: PeerBehavior,
    ) -> Result<(), SimulationError> {
        self.schedule_event(
            EventType::PeerConnect {
                peer_id: peer_id.clone(),
            },
            EventPriority::Normal,
        )?;

        match behavior {
            PeerBehavior::Fast => {}
            PeerBehavior::Slow => {}
            PeerBehavior::Unreliable => {
                let disconnect_time = Duration::from_secs(self.rng.random_range(5, 30));
                self.schedule_delayed(
                    disconnect_time,
                    EventType::PeerDisconnect { peer_id },
                    EventPriority::Normal,
                )?;
            }
        }

        Ok(())
    }
}

/// Peer behavior patterns for simulation.
#[derive(Debug, Clone, Copy)]
pub enum PeerBehavior {
    /// Fast, reliable peer with high upload speeds
    Fast,
    /// Slow peer with limited bandwidth
    Slow,
    /// Unreliable peer that frequently disconnects
    Unreliable,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_simulation_initialization() {
        let config = SimulationConfig {
            enabled: true,
            deterministic_seed: Some(42),
            network_latency_ms: 50,
            packet_loss_rate: 0.01,
            max_simulated_peers: 10,
            simulated_download_speed: 1_048_576,
            use_mock_data: true,
        };

        let sim = DeterministicSimulation::new(config).unwrap();
        assert_eq!(sim.simulation_seed(), 42);
        assert_eq!(sim.simulation_elapsed(), Duration::ZERO);
    }

    #[test]
    fn test_simulation_without_seed_fails() {
        let config = SimulationConfig {
            enabled: true,
            deterministic_seed: None,
            ..Default::default()
        };

        let result = DeterministicSimulation::new(config);
        assert!(matches!(result, Err(SimulationError::NoDeterministicSeed)));
    }

    #[test]
    fn test_event_scheduling() {
        let config = SimulationConfig {
            enabled: true,
            deterministic_seed: Some(123),
            ..Default::default()
        };

        let mut sim = DeterministicSimulation::new(config).unwrap();

        // Schedule immediate event
        sim.schedule_event(
            EventType::PeerConnect {
                peer_id: "PEER1".to_string(),
            },
            EventPriority::Normal,
        )
        .unwrap();

        // Schedule delayed event
        sim.schedule_delayed(
            Duration::from_secs(5),
            EventType::PeerDisconnect {
                peer_id: "PEER1".to_string(),
            },
            EventPriority::Normal,
        )
        .unwrap();

        assert_eq!(sim.event_queue.len(), 2);
    }
}
