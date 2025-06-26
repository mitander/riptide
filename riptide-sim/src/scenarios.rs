//! Pre-built simulation scenarios for common BitTorrent testing patterns
//!
//! Provides ready-to-use simulation scenarios that reproduce specific
//! network conditions and edge cases for systematic testing.

pub mod streaming_edge_cases;
pub mod streaming_invariants;

use std::time::Duration;

use riptide_core::config::SimulationConfig;
use riptide_core::torrent::PieceIndex;

use crate::EventPriority;
use crate::deterministic::{DeterministicSimulation, EventType, PeerBehavior};

/// Standard streaming scenario configuration.
#[derive(Debug, Clone)]
pub struct StreamingScenario {
    pub total_pieces: u32,
    pub piece_size: u32,
    pub target_bitrate: u64, // bits per second
    pub buffer_size: u32,    // pieces to buffer ahead
    pub network_latency: Duration,
    pub peer_count: usize,
}

impl Default for StreamingScenario {
    fn default() -> Self {
        Self {
            total_pieces: 1000,
            piece_size: 262144,        // 256 KB pieces
            target_bitrate: 5_000_000, // 5 Mbps
            buffer_size: 10,
            network_latency: Duration::from_millis(50),
            peer_count: 15,
        }
    }
}

/// Network stress scenario for testing resilience.
#[derive(Debug, Clone)]
pub struct StressScenario {
    pub peer_churn_rate: f64,     // fraction of peers that disconnect per minute
    pub packet_loss_rate: f64,    // fraction of packets lost
    pub bandwidth_variance: f64,  // how much bandwidth varies (0.0 to 1.0)
    pub connection_failures: u32, // number of connection failures to inject
}

impl Default for StressScenario {
    fn default() -> Self {
        Self {
            peer_churn_rate: 0.2,    // 20% churn per minute
            packet_loss_rate: 0.05,  // 5% packet loss
            bandwidth_variance: 0.3, // 30% variance
            connection_failures: 5,
        }
    }
}

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
            network_latency_ms: 100,
            packet_loss_rate: 0.05,
            max_simulated_peers: 10,
            simulated_download_speed: 524_288, // 500 KB/s
            use_mock_data: true,
        };
        let mut sim =
            DeterministicSimulation::new(config).expect("Failed to create slow network simulation");

        // Add slow peers with high latency
        for i in 0..10 {
            let peer_id = format!("SLOW{i:03}");
            sim.add_deterministic_peer(peer_id, PeerBehavior::Slow)
                .expect("Failed to add slow network peer");
        }

        // Schedule network degradation events
        sim.schedule_delayed(
            Duration::from_secs(30),
            EventType::NetworkChange {
                latency_ms: 100,
                packet_loss_rate: 0.05,
            },
            EventPriority::High,
        )
        .expect("Failed to schedule first network degradation event");
        sim.schedule_delayed(
            Duration::from_secs(60),
            EventType::NetworkChange {
                latency_ms: 200,
                packet_loss_rate: 0.10,
            },
            EventPriority::High,
        )
        .expect("Failed to schedule second network degradation event");

        sim.create_streaming_scenario(500, Duration::from_secs(2))
            .expect("Failed to create slow network streaming scenario");

        sim
    }

    /// Creates peer churn scenario with frequent disconnections.
    ///
    /// Tests resilience to peers joining and leaving the swarm frequently.
    /// Critical for streaming applications that need continuous data flow.
    pub fn peer_churn(seed: u64) -> DeterministicSimulation {
        let config = SimulationConfig {
            enabled: true,
            deterministic_seed: Some(seed),
            network_latency_ms: 50,
            packet_loss_rate: 0.02,
            max_simulated_peers: 30,
            simulated_download_speed: 2_097_152, // 2 MB/s
            use_mock_data: true,
        };
        let mut sim =
            DeterministicSimulation::new(config).expect("Failed to create peer churn simulation");

        // Add initial peer set
        for i in 0..20 {
            let peer_id = format!("CHURN{i:03}");
            sim.add_deterministic_peer(peer_id.clone(), PeerBehavior::Slow)
                .expect("Failed to add peer for churn scenario");

            // Schedule random disconnections
            if i % 3 == 0 {
                let disconnect_time = Duration::from_secs(10 + (i as u64 * 5));
                sim.schedule_delayed(
                    disconnect_time,
                    EventType::PeerDisconnect {
                        peer_id: peer_id.clone(),
                    },
                    EventPriority::High,
                )
                .expect("Failed to schedule peer disconnect for churn scenario");
            }
        }

        sim.create_streaming_scenario(800, Duration::from_secs(1))
            .expect("Failed to create peer churn streaming scenario");

        sim
    }

    /// Creates piece failure scenario for error handling testing.
    ///
    /// Injects hash failures and download errors to validate
    /// retry logic and error recovery mechanisms.
    pub fn piece_failures(seed: u64) -> DeterministicSimulation {
        let config = SimulationConfig {
            enabled: true,
            deterministic_seed: Some(seed),
            network_latency_ms: 75,
            packet_loss_rate: 0.03,
            max_simulated_peers: 12,
            simulated_download_speed: 3_145_728, // 3 MB/s
            use_mock_data: true,
        };
        let mut sim = DeterministicSimulation::new(config)
            .expect("Failed to create piece failures simulation");

        // Add unreliable peers
        for i in 0..12 {
            let peer_id = format!("UNRELIABLE{i:02}");
            sim.add_deterministic_peer(peer_id, PeerBehavior::Unreliable)
                .expect("Failed to add unreliable peer for piece failures scenario");
        }

        // Schedule specific piece failures for testing
        let failure_pieces = [15, 47, 128, 256, 512];
        for &piece_index in &failure_pieces {
            let fail_time = Duration::from_secs(5 + piece_index as u64 / 10);
            sim.schedule_delayed(
                fail_time,
                EventType::PieceFailed {
                    peer_id: format!("UNRELIABLE{:02}", piece_index % 12),
                    piece_index: PieceIndex::new(piece_index),
                    reason: "Simulated hash mismatch".to_string(),
                },
                EventPriority::High,
            )
            .expect("Failed to schedule piece failure event");
        }

        sim.create_streaming_scenario(1000, Duration::from_secs(1))
            .expect("Failed to create piece failures streaming scenario");

        sim
    }

    /// Creates mixed peer types scenario for realistic conditions.
    ///
    /// Combines fast seeders, slow leechers, and unreliable peers to
    /// simulate real-world BitTorrent swarm composition.
    pub fn mixed_peers(seed: u64) -> DeterministicSimulation {
        let config = SimulationConfig {
            enabled: true,
            deterministic_seed: Some(seed),
            network_latency_ms: 40,
            packet_loss_rate: 0.01,
            max_simulated_peers: 20,
            simulated_download_speed: 5_242_880, // 5 MB/s average
            use_mock_data: true,
        };
        let mut sim =
            DeterministicSimulation::new(config).expect("Failed to create mixed peers simulation");

        // Fast seeders (3)
        for i in 0..3 {
            let peer_id = format!("SEEDER{i:02}");
            sim.add_deterministic_peer(peer_id, PeerBehavior::Fast)
                .expect("Failed to add fast seeder peer");
        }

        // Medium peers (8)
        for i in 0..8 {
            let peer_id = format!("MEDIUM{i:02}");
            sim.add_deterministic_peer(peer_id, PeerBehavior::Fast)
                .expect("Failed to add medium speed peer");
        }

        // Slow peers (6)
        for i in 0..6 {
            let peer_id = format!("SLOWPEER{i:02}");
            sim.add_deterministic_peer(peer_id, PeerBehavior::Slow)
                .expect("Failed to add slow peer");
        }

        // Unreliable peers (3)
        for i in 0..3 {
            let peer_id = format!("UNSTABLE{i:02}");
            sim.add_deterministic_peer(peer_id, PeerBehavior::Unreliable)
                .expect("Failed to add unreliable peer");
        }

        sim.create_streaming_scenario(1200, Duration::from_secs(1))
            .expect("Failed to create mixed peers streaming scenario");

        sim
    }

    /// Creates endgame scenario for testing final piece acquisition.
    ///
    /// Simulates the challenging endgame phase where only a few pieces
    /// remain and they're distributed across different peers.
    pub fn endgame_scenario(seed: u64) -> DeterministicSimulation {
        let config = SimulationConfig {
            enabled: true,
            deterministic_seed: Some(seed),
            network_latency_ms: 60,
            packet_loss_rate: 0.02,
            max_simulated_peers: 15,
            simulated_download_speed: 2_097_152, // 2 MB/s
            use_mock_data: true,
        };
        let mut sim = DeterministicSimulation::new(config).unwrap();

        // Add peers with partial content
        for i in 0..15 {
            let peer_id = format!("PARTIAL{i:02}");
            sim.add_deterministic_peer(peer_id, PeerBehavior::Slow)
                .unwrap();
        }

        // Simulate most pieces already downloaded, only final pieces remain
        let remaining_pieces = [995, 996, 997, 998, 999];

        for &piece_index in &remaining_pieces {
            let request_time = Duration::from_secs(1 + piece_index as u64 - 995);
            sim.schedule_delayed(
                request_time,
                EventType::PieceRequest {
                    peer_id: format!("PARTIAL{:02}", piece_index % 15),
                    piece_index: PieceIndex::new(piece_index),
                },
                EventPriority::Critical,
            )
            .unwrap();
        }

        sim
    }
}

/// Test runner for systematic scenario validation.
pub struct ScenarioRunner {
    seed: u64,
    #[allow(dead_code)]
    scenarios: Vec<String>,
}

impl ScenarioRunner {
    /// Creates new test runner with specified seed.
    pub fn new(seed: u64) -> Self {
        Self {
            seed,
            scenarios: Vec::new(),
        }
    }

    /// Runs all predefined scenarios and collects results.
    ///
    /// Returns performance metrics and event counts for analysis.
    pub fn run_all_scenarios(&mut self) -> ScenarioResults {
        let mut results = ScenarioResults::new();

        // Run each scenario type
        results.add_result("ideal_streaming".to_string(), self.run_scenario("ideal"));
        results.add_result("slow_network".to_string(), self.run_scenario("slow"));
        results.add_result("peer_churn".to_string(), self.run_scenario("churn"));
        results.add_result("piece_failures".to_string(), self.run_scenario("failures"));
        results.add_result("mixed_peers".to_string(), self.run_scenario("mixed"));
        results.add_result("endgame".to_string(), self.run_scenario("endgame"));

        results
    }

    /// Runs specific scenario and measures performance.
    fn run_scenario(&mut self, scenario_type: &str) -> ScenarioResult {
        let mut sim = match scenario_type {
            "ideal" => SimulationScenarios::ideal_streaming(self.seed),
            "slow" => SimulationScenarios::slow_network(self.seed),
            "churn" => SimulationScenarios::peer_churn(self.seed),
            "failures" => SimulationScenarios::piece_failures(self.seed),
            "mixed" => SimulationScenarios::mixed_peers(self.seed),
            "endgame" => SimulationScenarios::endgame_scenario(self.seed),
            _ => SimulationScenarios::ideal_streaming(self.seed),
        };

        let start_time = sim.clock().now();
        let report = sim
            .run_for(Duration::from_secs(300))
            .expect("Failed to run simulation for scenario"); // 5 minute simulation
        let end_time = sim.clock().now();

        ScenarioResult {
            duration: end_time.duration_since(start_time),
            total_events: report.event_count as usize,
            piece_requests: report
                .metrics
                .events_by_type
                .get("PieceRequest")
                .copied()
                .unwrap_or(0) as usize,
            piece_completions: report
                .metrics
                .events_by_type
                .get("PieceComplete")
                .copied()
                .unwrap_or(0) as usize,
            piece_failures: report
                .metrics
                .events_by_type
                .get("PieceFailed")
                .copied()
                .unwrap_or(0) as usize,
            peer_connections: report
                .metrics
                .events_by_type
                .get("PeerConnect")
                .copied()
                .unwrap_or(0) as usize,
            peer_disconnections: report
                .metrics
                .events_by_type
                .get("PeerDisconnect")
                .copied()
                .unwrap_or(0) as usize,
        }
    }
}

/// Results from running a single scenario.
#[derive(Debug, Clone)]
pub struct ScenarioResult {
    pub duration: Duration,
    pub total_events: usize,
    pub piece_requests: usize,
    pub piece_completions: usize,
    pub piece_failures: usize,
    pub peer_connections: usize,
    pub peer_disconnections: usize,
}

impl ScenarioResult {
    /// Calculates success rate for piece downloads.
    pub fn success_rate(&self) -> f64 {
        if self.piece_requests == 0 {
            0.0
        } else {
            self.piece_completions as f64 / self.piece_requests as f64
        }
    }

    /// Calculates peer stability (connections vs disconnections).
    pub fn peer_stability(&self) -> f64 {
        if self.peer_connections == 0 {
            1.0
        } else {
            1.0 - (self.peer_disconnections as f64 / self.peer_connections as f64)
        }
    }
}

/// Collection of results from multiple scenarios.
#[derive(Debug, Default)]
pub struct ScenarioResults {
    results: std::collections::HashMap<String, ScenarioResult>,
}

impl ScenarioResults {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add_result(&mut self, name: String, result: ScenarioResult) {
        self.results.insert(name, result);
    }

    pub fn get_result(&self, name: &str) -> Option<&ScenarioResult> {
        self.results.get(name)
    }

    /// Prints summary of all scenario results.
    pub fn print_summary(&self) {
        println!("Simulation Scenario Results");
        println!("{:-<60}", "");

        for (name, result) in &self.results {
            println!(
                "{}: Success Rate: {:.1}% | Peer Stability: {:.1}% | Events: {}",
                name,
                result.success_rate() * 100.0,
                result.peer_stability() * 100.0,
                result.total_events
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_scenario_reproducibility() {
        let seed = 12345;

        // Run same scenario twice
        let mut sim1 = SimulationScenarios::ideal_streaming(seed);
        let report1 = sim1
            .run_for(Duration::from_secs(10))
            .expect("Failed to run first simulation for reproducibility test");

        let mut sim2 = SimulationScenarios::ideal_streaming(seed);
        let report2 = sim2
            .run_for(Duration::from_secs(10))
            .expect("Failed to run second simulation for reproducibility test");

        // Should produce identical results
        assert_eq!(report1.event_count, report2.event_count);
    }

    #[test]
    fn test_scenario_runner() {
        let mut runner = ScenarioRunner::new(98765);
        let results = runner.run_all_scenarios();

        // Should have results for all scenarios
        assert!(results.get_result("ideal_streaming").is_some());
        assert!(results.get_result("slow_network").is_some());
        assert!(results.get_result("peer_churn").is_some());
    }

    #[test]
    fn test_scenario_result_metrics() {
        let result = ScenarioResult {
            duration: Duration::from_secs(60),
            total_events: 100,
            piece_requests: 50,
            piece_completions: 45,
            piece_failures: 5,
            peer_connections: 20,
            peer_disconnections: 2,
        };

        assert_eq!(result.success_rate(), 0.9); // 45/50 = 90%
        assert_eq!(result.peer_stability(), 0.9); // 1 - (2/20) = 90%
    }
}
