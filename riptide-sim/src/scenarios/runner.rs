//! Scenario test runner and result collection.

use std::collections::HashMap;
use std::time::Duration;

use super::builders::SimulationScenarios;

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
            total_events: report.event_count as u32,
            piece_requests: report
                .metrics
                .events_by_type
                .get("PieceRequest")
                .copied()
                .unwrap_or(0) as u32,
            piece_completions: report
                .metrics
                .events_by_type
                .get("PieceComplete")
                .copied()
                .unwrap_or(0) as u32,
            piece_failures: report
                .metrics
                .events_by_type
                .get("PieceFailed")
                .copied()
                .unwrap_or(0) as u32,
            peer_connections: report
                .metrics
                .events_by_type
                .get("PeerConnect")
                .copied()
                .unwrap_or(0) as u32,
            peer_disconnections: report
                .metrics
                .events_by_type
                .get("PeerDisconnect")
                .copied()
                .unwrap_or(0) as u32,
        }
    }
}

/// Results from running a single scenario.
#[derive(Debug, Clone)]
pub struct ScenarioResult {
    pub duration: Duration,
    pub total_events: u32,
    pub piece_requests: u32,
    pub piece_completions: u32,
    pub piece_failures: u32,
    pub peer_connections: u32,
    pub peer_disconnections: u32,
}

impl ScenarioResult {
    /// Calculates completion percentage.
    pub fn success_rate(&self) -> f64 {
        if self.piece_requests == 0 {
            0.0
        } else {
            self.piece_completions as f64 / self.piece_requests as f64
        }
    }

    /// Calculates peer connection stability.
    pub fn peer_stability(&self) -> f64 {
        if self.peer_connections == 0 {
            0.0
        } else {
            1.0 - (self.peer_disconnections as f64 / self.peer_connections as f64)
        }
    }
}

/// Collection of results from multiple scenarios.
#[derive(Debug)]
pub struct ScenarioResults {
    results: HashMap<String, ScenarioResult>,
}

impl ScenarioResults {
    pub fn new() -> Self {
        Self {
            results: HashMap::new(),
        }
    }

    /// Add a scenario result with the given name.
    pub fn add_result(&mut self, name: String, result: ScenarioResult) {
        self.results.insert(name, result);
    }

    /// Get a scenario result by name.
    pub fn get_result(&self, name: &str) -> Option<&ScenarioResult> {
        self.results.get(name)
    }

    /// Get all scenario results.
    pub fn results(&self) -> &HashMap<String, ScenarioResult> {
        &self.results
    }
}

impl Default for ScenarioResults {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
