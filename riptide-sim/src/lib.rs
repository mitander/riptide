//! Riptide Simulation Framework - Deterministic testing for BitTorrent streaming.
//!
//! This crate provides a comprehensive simulation environment for testing
//! BitTorrent protocol implementations, streaming algorithms, and network
//! behavior under controlled, reproducible conditions.
//!
//! # Features
//!
//! - **Deterministic Execution**: Same seed always produces identical results
//! - **Event-Based Simulation**: Precise control over timing and ordering
//! - **Network Simulation**: Configurable latency, packet loss, and bandwidth
//! - **Invariant Checking**: Validate protocol correctness during execution
//! - **Resource Governance**: Enforce memory, connection, and CPU limits
//! - **Edge Case Testing**: Pre-built scenarios for challenging conditions
//!
//! # Example
//!
//! ```rust,no_run
//! use riptide_sim::{DeterministicSimulation, SimulationConfig};
//! use riptide_core::torrent::InfoHash;
//! use std::time::Duration;
//!
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! // Configure simulation
//! let config = SimulationConfig {
//!     enabled: true,
//!     deterministic_seed: Some(12345),
//!     network_latency_ms: 50,
//!     packet_loss_rate: 0.01,
//!     max_simulated_peers: 20,
//!     simulated_download_speed: 5_242_880, // 5 MB/s
//!     use_mock_data: true,
//! };
//!
//! // Create and run simulation
//! let mut sim = DeterministicSimulation::new(config)?;
//! sim.create_streaming_scenario(100, Duration::from_secs(5))?;
//!
//! let report = sim.run_for(Duration::from_secs(30))?;
//! println!("Completed {} pieces", report.final_state.completed_pieces.len());
//! # Ok(())
//! # }
//! ```
//!
//! # Architecture
//!
//! The simulation framework consists of several key components:
//!
//! - **Deterministic Engine**: Core event scheduler with controlled time
//! - **Mock Components**: Simulated peers, trackers, and network behavior
//! - **Scenario Library**: Pre-built test cases for common situations
//! - **Invariant System**: Runtime validation of protocol correctness
//! - **Metrics Collection**: Detailed performance and behavior analysis

pub mod content_aware_peer_manager;
pub mod deterministic;
pub mod magneto_provider;
pub mod media;
pub mod network;
pub mod peer;
pub mod piece_reconstruction;
pub mod piece_store;
pub mod scenarios;
pub mod simulated_peer_manager;
pub mod streaming_integration_tests;
pub mod tracker;

// Re-export core types for convenience
use std::collections::HashSet;
use std::sync::Arc;
use std::time::{Duration, Instant};

pub use content_aware_peer_manager::ContentAwarePeerManager;
pub use deterministic::{
    BandwidthInvariant, DataIntegrityInvariant, DeterministicClock, DeterministicRng,
    DeterministicSimulation, EventPriority, EventType, Invariant, InvariantViolation,
    MinimumPeersInvariant, PeerBehavior, ResourceLimitInvariant, ResourceLimits, ResourceType,
    ResourceUsage, SimulationError, SimulationEvent, SimulationMetrics, SimulationReport,
    SimulationState, StreamingInvariant, ThrottleDirection,
};
pub use magneto_provider::{
    MockMagnetoProvider, MockMagnetoProviderBuilder, TorrentEntryParams,
    create_mock_magneto_client, create_streaming_test_client,
};
pub use media::{MediaStreamingSimulation, MovieFolder, StreamingResult};
pub use network::{NetworkSimulator, NetworkSimulatorBuilder};
pub use peer::{MockPeer, MockPeerBuilder};
pub use piece_reconstruction::{PieceReconstructionService, ReconstructionProgress, VerifiedPiece};
pub use piece_store::InMemoryPieceStore;
// Re-export config from core for convenience
pub use riptide_core::config::SimulationConfig;
pub use scenarios::{
    ScenarioResult, ScenarioResults, ScenarioRunner, SimulationScenarios, StreamingScenario,
    StressScenario, streaming_edge_cases,
};
pub use simulated_peer_manager::{InMemoryPeerConfig, InMemoryPeerManager};
pub use tracker::{MockTracker, MockTrackerBuilder};

/// Simulation environment for BitTorrent development.
///
/// Combines mock tracker, network simulator, and peer pool for
/// comprehensive testing of BitTorrent functionality.
pub struct SimulationEnvironment {
    pub tracker: MockTracker,
    pub network: NetworkSimulator,
    pub peers: Vec<MockPeer>,
}

impl Default for SimulationEnvironment {
    fn default() -> Self {
        Self::new()
    }
}

impl SimulationEnvironment {
    /// Creates a new simulation environment with sensible defaults.
    pub fn new() -> Self {
        Self {
            tracker: MockTracker::new(),
            network: NetworkSimulator::new(),
            peers: Vec::new(),
        }
    }

    /// Creates environment optimized for streaming development.
    pub fn for_streaming() -> Self {
        let mut env = Self::new();

        // Configure network for realistic streaming conditions
        env.network = NetworkSimulator::builder()
            .latency(10..100) // 10-100ms typical home internet
            .packet_loss(0.001) // 0.1% loss
            .bandwidth_limit(50_000_000) // 50 Mbps
            .build();

        // Add mix of fast and slow peers
        env.add_fast_peers(10);
        env.add_slow_peers(5);
        env.add_unreliable_peers(2);

        env
    }

    /// Adds fast, reliable peers to the simulation.
    pub fn add_fast_peers(&mut self, count: usize) {
        for i in 0..count {
            let peer = MockPeer::builder()
                .upload_speed(5_000_000) // 5 MB/s
                .reliability(0.99)
                .latency(Duration::from_millis(20))
                .peer_id(format!("FAST{i:04}"))
                .build();
            self.peers.push(peer);
        }
    }

    /// Adds slow peers that simulate real-world conditions.
    pub fn add_slow_peers(&mut self, count: usize) {
        for i in 0..count {
            let peer = MockPeer::builder()
                .upload_speed(500_000) // 500 KB/s
                .reliability(0.95)
                .latency(Duration::from_millis(100))
                .peer_id(format!("SLOW{i:04}"))
                .build();
            self.peers.push(peer);
        }
    }

    /// Adds unreliable peers for testing error handling.
    pub fn add_unreliable_peers(&mut self, count: usize) {
        for i in 0..count {
            let peer = MockPeer::builder()
                .upload_speed(1_000_000) // 1 MB/s
                .reliability(0.70) // Drops connections frequently
                .latency(Duration::from_millis(200))
                .peer_id(format!("UNREL{i:03}"))
                .build();
            self.peers.push(peer);
        }
    }
}

/// Invariant that prevents duplicate piece downloads.
struct NoDuplicateDownloadsInvariant;

impl Invariant for NoDuplicateDownloadsInvariant {
    fn check(&self, state: &SimulationState) -> std::result::Result<(), InvariantViolation> {
        let mut seen_pieces = HashSet::new();

        for piece_index in state.downloading_pieces.keys() {
            if !seen_pieces.insert(*piece_index) {
                return Err(InvariantViolation {
                    invariant: "NoDuplicateDownloads".to_string(),
                    description: format!("Piece {piece_index} is being downloaded multiple times"),
                    timestamp: Instant::now(),
                });
            }
        }

        Ok(())
    }

    fn name(&self) -> &str {
        "NoDuplicateDownloads"
    }
}

/// Invariant that enforces maximum peer count.
struct MaxPeersInvariant {
    max_peers: usize,
}

impl MaxPeersInvariant {
    fn new(max_peers: usize) -> Self {
        Self { max_peers }
    }
}

impl Invariant for MaxPeersInvariant {
    fn check(&self, state: &SimulationState) -> std::result::Result<(), InvariantViolation> {
        if state.connected_peers.len() > self.max_peers {
            return Err(InvariantViolation {
                invariant: "MaxPeers".to_string(),
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

/// Creates a standard streaming simulation with common invariants.
///
/// This is a convenience function for quick testing that sets up
/// a simulation with reasonable defaults and common invariants.
///
/// # Errors
/// - `SimulationError::InvalidEventScheduling` - Invalid configuration
pub fn create_standard_streaming_simulation(
    seed: u64,
    piece_count: u32,
) -> Result<DeterministicSimulation> {
    let config = SimulationConfig {
        enabled: true,
        deterministic_seed: Some(seed),
        network_latency_ms: 50,
        packet_loss_rate: 0.01,
        max_simulated_peers: 20,
        simulated_download_speed: 5_242_880, // 5 MB/s
        use_mock_data: true,
    };

    let mut sim = DeterministicSimulation::new(config)?;

    // Add standard invariants
    sim.add_invariant(Arc::new(MaxPeersInvariant::new(25)));
    sim.add_invariant(Arc::new(NoDuplicateDownloadsInvariant));
    sim.add_invariant(Arc::new(ResourceLimitInvariant::new(
        ResourceLimits::default(),
    )));

    // Create streaming scenario
    sim.create_streaming_scenario(piece_count, std::time::Duration::from_secs(1))?;

    Ok(sim)
}

/// Common simulation error type for convenience.
pub type Result<T> = std::result::Result<T, SimulationError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_simulation_environment_creation() {
        let env = SimulationEnvironment::new();
        assert_eq!(env.peers.len(), 0);

        let streaming_env = SimulationEnvironment::for_streaming();
        assert_eq!(streaming_env.peers.len(), 17); // 10 fast + 5 slow + 2 unreliable
    }

    #[test]
    fn test_standard_streaming_simulation() {
        let sim = create_standard_streaming_simulation(12345, 100).unwrap();
        assert_eq!(sim.seed(), 12345);
    }
}
