//! Simulation framework for rapid BitTorrent development

pub mod deterministic;
pub mod magneto_provider;
pub mod media;
pub mod network;
pub mod peer;
pub mod scenarios;
pub mod tracker;

use std::time::Duration;

pub use deterministic::{DeterministicClock, DeterministicSimulation, EventType, SimulationEvent};
pub use magneto_provider::{
    MockMagnetoProvider, MockMagnetoProviderBuilder, create_mock_magneto_client,
    create_streaming_test_client,
};
pub use media::{MediaStreamingSimulation, MovieFolder, StreamingResult};
pub use network::NetworkSimulator;
pub use peer::MockPeer;
pub use scenarios::{ScenarioResults, ScenarioRunner, SimulationScenarios};
pub use tracker::MockTracker;

/// Complete simulation environment for BitTorrent development.
///
/// Combines mock tracker, network simulator, and peer pool for comprehensive
/// offline development and testing of BitTorrent functionality.
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
    /// Create a new simulation environment with sensible defaults.
    pub fn new() -> Self {
        Self {
            tracker: MockTracker::new(),
            network: NetworkSimulator::new(),
            peers: Vec::new(),
        }
    }

    /// Create environment optimized for streaming development.
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

    /// Add fast, reliable peers to the simulation.
    pub fn add_fast_peers(&mut self, count: usize) {
        for i in 0..count {
            let peer = MockPeer::builder()
                .upload_speed(5_000_000) // 5 MB/s
                .reliability(0.99)
                .latency(Duration::from_millis(20))
                .peer_id(format!("FAST{:04}", i))
                .build();
            self.peers.push(peer);
        }
    }

    /// Add slow peers that simulate real-world conditions.
    pub fn add_slow_peers(&mut self, count: usize) {
        for i in 0..count {
            let peer = MockPeer::builder()
                .upload_speed(500_000) // 500 KB/s
                .reliability(0.95)
                .latency(Duration::from_millis(100))
                .peer_id(format!("SLOW{:04}", i))
                .build();
            self.peers.push(peer);
        }
    }

    /// Add unreliable peers for testing error handling.
    pub fn add_unreliable_peers(&mut self, count: usize) {
        for i in 0..count {
            let peer = MockPeer::builder()
                .upload_speed(1_000_000) // 1 MB/s
                .reliability(0.70) // Drops connections frequently
                .latency(Duration::from_millis(200))
                .peer_id(format!("UNREL{:03}", i))
                .build();
            self.peers.push(peer);
        }
    }
}
