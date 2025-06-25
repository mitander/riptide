//! Pre-built simulation scenarios for common BitTorrent testing patterns
//!
//! Provides ready-to-use simulation scenarios that reproduce specific
//! network conditions and edge cases for systematic testing.

use std::time::Duration;

use super::deterministic::{DeterministicSimulation, EventType};
use crate::torrent::PieceIndex;

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
        let mut sim = DeterministicSimulation::from_seed(seed);
        let scenario = StreamingScenario::default();

        // Add fast, reliable peers
        for i in 0..scenario.peer_count {
            let peer_id = format!("IDEAL{i:03}");
            sim.add_deterministic_peer(peer_id, 10_000_000); // 10 MB/s
        }

        // Create sequential piece requests for streaming
        sim.create_streaming_scenario(scenario.total_pieces, Duration::from_secs(1));

        sim
    }

    /// Creates slow network conditions scenario.
    ///
    /// Simulates poor network conditions with high latency and
    /// limited bandwidth to test streaming resilience.
    pub fn slow_network(seed: u64) -> DeterministicSimulation {
        let mut sim = DeterministicSimulation::from_seed(seed);

        // Add slow peers with high latency
        for i in 0..10 {
            let peer_id = format!("SLOW{i:03}");
            sim.add_deterministic_peer(peer_id, 500_000); // 500 KB/s
        }

        // Schedule network degradation events
        sim.schedule_delayed(Duration::from_secs(30), EventType::NetworkChange);
        sim.schedule_delayed(Duration::from_secs(60), EventType::NetworkChange);

        sim.create_streaming_scenario(500, Duration::from_secs(2));

        sim
    }

    /// Creates peer churn scenario with frequent disconnections.
    ///
    /// Tests resilience to peers joining and leaving the swarm frequently.
    /// Critical for streaming applications that need continuous data flow.
    pub fn peer_churn(seed: u64) -> DeterministicSimulation {
        let mut sim = DeterministicSimulation::from_seed(seed);

        // Add initial peer set
        for i in 0..20 {
            let peer_id = format!("CHURN{i:03}");
            sim.add_deterministic_peer(peer_id.clone(), 2_000_000); // 2 MB/s

            // Schedule random disconnections
            if i % 3 == 0 {
                let disconnect_time = Duration::from_secs(10 + (i as u64 * 5));
                sim.schedule_delayed(disconnect_time, EventType::PeerDisconnect);
            }
        }

        sim.create_streaming_scenario(800, Duration::from_secs(1));

        sim
    }

    /// Creates piece failure scenario for error handling testing.
    ///
    /// Injects hash failures and download errors to validate
    /// retry logic and error recovery mechanisms.
    pub fn piece_failures(seed: u64) -> DeterministicSimulation {
        let mut sim = DeterministicSimulation::from_seed(seed);

        // Add unreliable peers
        for i in 0..12 {
            let peer_id = format!("UNRELIABLE{i:02}");
            sim.add_deterministic_peer(peer_id, 3_000_000); // 3 MB/s
        }

        // Schedule specific piece failures for testing
        let failure_pieces = [15, 47, 128, 256, 512];
        for &piece_idx in &failure_pieces {
            let fail_time = Duration::from_secs(5 + piece_idx as u64 / 10);
            let fail_event = super::deterministic::SimulationEvent {
                timestamp: sim.clock().now() + fail_time,
                event_type: EventType::PieceFailed,
                peer_id: None,
                piece_index: Some(PieceIndex::new(piece_idx)),
            };
            sim.schedule_event(fail_event);
        }

        sim.create_streaming_scenario(1000, Duration::from_secs(1));

        sim
    }

    /// Creates mixed peer types scenario for realistic conditions.
    ///
    /// Combines fast seeders, slow leechers, and unreliable peers to
    /// simulate real-world BitTorrent swarm composition.
    pub fn mixed_peers(seed: u64) -> DeterministicSimulation {
        let mut sim = DeterministicSimulation::from_seed(seed);

        // Fast seeders (3)
        for i in 0..3 {
            let peer_id = format!("SEEDER{i:02}");
            sim.add_deterministic_peer(peer_id, 15_000_000); // 15 MB/s
        }

        // Medium peers (8)
        for i in 0..8 {
            let peer_id = format!("MEDIUM{i:02}");
            sim.add_deterministic_peer(peer_id, 3_000_000); // 3 MB/s
        }

        // Slow peers (6)
        for i in 0..6 {
            let peer_id = format!("SLOWPEER{i:02}");
            sim.add_deterministic_peer(peer_id, 800_000); // 800 KB/s
        }

        // Unreliable peers (3)
        for i in 0..3 {
            let peer_id = format!("UNSTABLE{i:02}");
            sim.add_deterministic_peer(peer_id, 5_000_000); // 5 MB/s fast but unreliable
        }

        sim.create_streaming_scenario(1200, Duration::from_secs(1));

        sim
    }

    /// Creates endgame scenario for testing final piece acquisition.
    ///
    /// Simulates the challenging endgame phase where only a few pieces
    /// remain and they're distributed across different peers.
    pub fn endgame_scenario(seed: u64) -> DeterministicSimulation {
        let mut sim = DeterministicSimulation::from_seed(seed);

        // Add peers with partial content
        for i in 0..15 {
            let peer_id = format!("PARTIAL{i:02}");
            sim.add_deterministic_peer(peer_id, 2_000_000); // 2 MB/s
        }

        // Simulate most pieces already downloaded, only final pieces remain
        let _total_pieces = 1000;
        let remaining_pieces = [995, 996, 997, 998, 999];

        for &piece_idx in &remaining_pieces {
            let request_time = Duration::from_secs(1 + piece_idx as u64 - 995);
            let request_event = super::deterministic::SimulationEvent {
                timestamp: sim.clock().now() + request_time,
                event_type: EventType::PieceRequest,
                peer_id: None,
                piece_index: Some(PieceIndex::new(piece_idx)),
            };
            sim.schedule_event(request_event);
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
        let events = sim.run_for(Duration::from_secs(300)); // 5 minute simulation
        let end_time = sim.clock().now();

        ScenarioResult {
            duration: end_time.duration_since(start_time),
            total_events: events.len(),
            piece_requests: events
                .iter()
                .filter(|e| e.event_type == EventType::PieceRequest)
                .count(),
            piece_completions: events
                .iter()
                .filter(|e| e.event_type == EventType::PieceComplete)
                .count(),
            piece_failures: events
                .iter()
                .filter(|e| e.event_type == EventType::PieceFailed)
                .count(),
            peer_connections: events
                .iter()
                .filter(|e| e.event_type == EventType::PeerConnect)
                .count(),
            peer_disconnections: events
                .iter()
                .filter(|e| e.event_type == EventType::PeerDisconnect)
                .count(),
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
        let events1 = sim1.run_for(Duration::from_secs(10));

        let mut sim2 = SimulationScenarios::ideal_streaming(seed);
        let events2 = sim2.run_for(Duration::from_secs(10));

        // Should produce identical results
        assert_eq!(events1.len(), events2.len());
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
