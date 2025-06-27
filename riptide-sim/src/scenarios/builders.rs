//! Core scenario builder implementations.

use std::time::Duration;

use riptide_core::config::SimulationConfig;

use super::types::StreamingScenario;
use crate::deterministic::{DeterministicSimulation, PeerBehavior};

/// Pre-built simulation scenarios for systematic testing.
pub struct SimulationScenarios;

impl SimulationScenarios {
    /// Creates ideal streaming conditions scenario.
    ///
    /// Fast, reliable peers with low latency. Used as baseline for
    /// performance testing and streaming algorithm validation.
    pub fn ideal_streaming(seed: u64) -> DeterministicSimulation {
        let config = SimulationConfig {
            enabled: true,
            deterministic_seed: Some(seed),
            network_latency_ms: 10,
            packet_loss_rate: 0.0,
            max_simulated_peers: 20,
            simulated_download_speed: 10_485_760, // 10 MB/s
            use_mock_data: true,
        };
        let mut sim = DeterministicSimulation::new(config)
            .expect("Failed to create ideal streaming simulation");
        let scenario = StreamingScenario::default();

        // Add fast, reliable peers
        for i in 0..scenario.peer_count {
            let peer_id = format!("IDEAL{i:03}");
            sim.add_deterministic_peer(peer_id, PeerBehavior::Fast)
                .expect("Failed to add ideal streaming peer");
        }

        // Create sequential piece requests for streaming
        sim.create_streaming_scenario(scenario.total_pieces, Duration::from_secs(1))
            .expect("Failed to create ideal streaming scenario");

        sim
    }

    /// Creates slow network conditions scenario.
    ///
    /// Simulates poor network conditions with high latency and
    /// limited bandwidth to test streaming resilience.
    pub fn slow_network(seed: u64) -> DeterministicSimulation {
        let config = SimulationConfig {
            enabled: true,
            deterministic_seed: Some(seed),
            network_latency_ms: 500, // High latency
            packet_loss_rate: 0.1,   // 10% packet loss
            max_simulated_peers: 15,
            simulated_download_speed: 524_288, // 512 KB/s
            use_mock_data: true,
        };
        let mut sim =
            DeterministicSimulation::new(config).expect("Failed to create slow network simulation");
        let scenario = StreamingScenario::default();

        // Add slower, less reliable peers
        for i in 0..scenario.peer_count {
            let peer_id = format!("SLOW{i:03}");
            let behavior = if i % 3 == 0 {
                PeerBehavior::Unreliable
            } else {
                PeerBehavior::Slow
            };
            sim.add_deterministic_peer(peer_id, behavior)
                .expect("Failed to add slow network peer");
        }

        // Create streaming scenario with longer intervals due to slow network
        sim.create_streaming_scenario(scenario.total_pieces, Duration::from_secs(3))
            .expect("Failed to create slow network streaming scenario");

        sim
    }

    /// Creates peer churn scenario with frequent connections/disconnections.
    ///
    /// Tests the torrent engine's ability to maintain download progress
    /// when peers frequently join and leave the swarm.
    pub fn peer_churn(seed: u64) -> DeterministicSimulation {
        let config = SimulationConfig {
            enabled: true,
            deterministic_seed: Some(seed),
            network_latency_ms: 100,
            packet_loss_rate: 0.02,
            max_simulated_peers: 25,
            simulated_download_speed: 2_097_152, // 2 MB/s
            use_mock_data: true,
        };
        let mut sim =
            DeterministicSimulation::new(config).expect("Failed to create peer churn simulation");
        let scenario = StreamingScenario::default();

        // Add initial peers with mixed reliability
        for i in 0..scenario.peer_count {
            let peer_id = format!("CHURN{i:03}");
            let behavior = match i % 3 {
                0 => PeerBehavior::Fast,
                1 => PeerBehavior::Slow,
                _ => PeerBehavior::Unreliable,
            };
            sim.add_deterministic_peer(peer_id, behavior)
                .expect("Failed to add peer churn peer");
        }

        // Note: Peer churn events would be scheduled here in a more complete implementation

        sim.create_streaming_scenario(scenario.total_pieces, Duration::from_secs(1))
            .expect("Failed to create peer churn streaming scenario");

        sim
    }

    /// Creates scenario with piece hash failures and corruption.
    ///
    /// Tests error recovery and retry mechanisms when pieces fail
    /// hash validation or arrive corrupted.
    pub fn piece_failures(seed: u64) -> DeterministicSimulation {
        let config = SimulationConfig {
            enabled: true,
            deterministic_seed: Some(seed),
            network_latency_ms: 75,
            packet_loss_rate: 0.05,
            max_simulated_peers: 12,
            simulated_download_speed: 1_572_864, // 1.5 MB/s
            use_mock_data: true,
        };
        let mut sim = DeterministicSimulation::new(config)
            .expect("Failed to create piece failures simulation");
        let scenario = StreamingScenario::default();

        // Add peers with varying reliability
        for i in 0..scenario.peer_count {
            let peer_id = format!("FAIL{i:03}");
            let behavior = if i % 3 == 0 {
                PeerBehavior::Unreliable // Unreliable peers may provide bad data
            } else {
                PeerBehavior::Fast
            };
            sim.add_deterministic_peer(peer_id, behavior)
                .expect("Failed to add piece failure peer");
        }

        // Note: Piece corruption events would be scheduled here in a more complete implementation

        sim.create_streaming_scenario(scenario.total_pieces, Duration::from_secs(1))
            .expect("Failed to create piece failures streaming scenario");

        sim
    }

    /// Creates mixed peer quality scenario for realistic testing.
    ///
    /// Combines different peer types to simulate real-world swarm
    /// conditions with varied peer capabilities and behaviors.
    pub fn mixed_peers(seed: u64) -> DeterministicSimulation {
        let config = SimulationConfig {
            enabled: true,
            deterministic_seed: Some(seed),
            network_latency_ms: 150,
            packet_loss_rate: 0.03,
            max_simulated_peers: 18,
            simulated_download_speed: 3_145_728, // 3 MB/s
            use_mock_data: true,
        };
        let mut sim =
            DeterministicSimulation::new(config).expect("Failed to create mixed peers simulation");
        let scenario = StreamingScenario::default();

        // Create realistic peer distribution
        let peer_types = [
            (3, PeerBehavior::Fast),       // 3 fast seeders
            (3, PeerBehavior::Slow),       // 3 slow peers
            (2, PeerBehavior::Unreliable), // 2 unreliable peers
        ];

        let mut peer_id_counter = 0;
        for (count, behavior) in peer_types {
            for _ in 0..count {
                let peer_id = format!("MIXED{peer_id_counter:03}");
                sim.add_deterministic_peer(peer_id, behavior)
                    .expect("Failed to add mixed peer");
                peer_id_counter += 1;
            }
        }

        sim.create_streaming_scenario(scenario.total_pieces, Duration::from_secs(2))
            .expect("Failed to create mixed peers streaming scenario");

        sim
    }

    /// Creates endgame scenario for testing final piece completion.
    ///
    /// Simulates the challenging endgame phase where only a few pieces
    /// remain and they must be requested from multiple peers.
    pub fn endgame_scenario(seed: u64) -> DeterministicSimulation {
        let config = SimulationConfig {
            enabled: true,
            deterministic_seed: Some(seed),
            network_latency_ms: 80,
            packet_loss_rate: 0.02,
            max_simulated_peers: 15,
            simulated_download_speed: 2_621_440, // 2.5 MB/s
            use_mock_data: true,
        };
        let mut sim =
            DeterministicSimulation::new(config).expect("Failed to create endgame simulation");
        let scenario = StreamingScenario::default();

        // Add peers for endgame testing
        for i in 0..scenario.peer_count {
            let peer_id = format!("END{i:03}");
            sim.add_deterministic_peer(peer_id, PeerBehavior::Fast)
                .expect("Failed to add endgame peer");
        }

        // Note: Endgame piece management would be implemented here in a more complete simulation

        sim.create_streaming_scenario(scenario.total_pieces, Duration::from_millis(500))
            .expect("Failed to create endgame streaming scenario");

        sim
    }
}
