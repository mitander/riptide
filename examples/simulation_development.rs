//! Example demonstrating the deterministic simulation framework.
//!
//! Shows how to use Riptide's simulation capabilities to test BitTorrent
//! streaming scenarios, find bugs, and validate performance characteristics.

use std::sync::Arc;
use std::time::Duration;

use riptide_core::torrent::{InfoHash, PieceIndex};
use riptide_sim::{
    streaming_edge_cases, DeterministicSimulation, EventPriority, EventType, MaxPeersInvariant,
    NoDuplicateDownloadsInvariant, ResourceLimitInvariant, ResourceLimits, SimulationConfig,
    SimulationError, SimulationReport,
};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Example 1: Basic simulation setup
    println!("=== Example 1: Basic Simulation ===\n");
    demonstrate_basic_simulation()?;

    // Example 2: Testing edge cases
    println!("\n=== Example 2: Edge Case Testing ===\n");
    demonstrate_edge_case_tests()?;

    // Example 3: Performance validation
    println!("\n=== Example 3: Performance Validation ===\n");
    demonstrate_performance_tests()?;

    // Example 4: Bug reproduction
    println!("\n=== Example 4: Bug Reproduction ===\n");
    reproduce_specific_bug()?;

    Ok(())
}

/// Demonstrates basic simulation setup and execution.
fn demonstrate_basic_simulation() -> Result<(), SimulationError> {
    // Configure simulation
    let config = SimulationConfig {
        enabled: true,
        deterministic_seed: Some(12345), // Fixed seed for reproducibility
        network_latency_ms: 50,
        packet_loss_rate: 0.01,
        max_simulated_peers: 20,
        simulated_download_speed: 5_242_880, // 5 MB/s
        use_mock_data: true,
    };

    // Create simulation
    let mut sim = DeterministicSimulation::new(config)?;

    // Add invariants to check
    sim.add_invariant(Arc::new(MaxPeersInvariant::new(25)));
    sim.add_invariant(Arc::new(NoDuplicateDownloadsInvariant));

    // Set resource limits
    sim.set_resource_limits(ResourceLimits {
        max_memory: 512 * 1024 * 1024, // 512 MB
        max_connections: 50,
        max_disk_usage: 2 * 1024 * 1024 * 1024, // 2 GB
        max_cpu_time_us: 1_000_000,             // 1 second
    });

    // Create streaming scenario
    let info_hash = InfoHash::new([0x42; 20]);
    sim.create_streaming_scenario(info_hash, 100, 262144)?; // 100 pieces, 256KB each

    // Schedule some custom events
    sim.schedule_delayed(
        Duration::from_secs(10),
        EventType::NetworkChange {
            latency_ms: 200,
            packet_loss: 0.05,
            bandwidth_mbps: 20,
        },
        EventPriority::High,
    )?;

    // Run simulation
    let report = sim.run_for(Duration::from_secs(30))?;

    // Print results
    println!("Simulation completed in {:?}", report.duration);
    println!("Events processed: {}", report.event_count);
    println!("Success: {}", report.success);
    println!("\nMetrics:");
    println!(
        "  Completed pieces: {}",
        report.final_state.completed_pieces.len()
    );
    println!(
        "  Failed pieces: {}",
        report.final_state.failed_pieces.len()
    );
    println!("  Peak connections: {}", report.metrics.peak_connections);
    println!(
        "  Peak memory: {} MB",
        report.metrics.peak_memory / 1_048_576
    );

    if !report.metrics.invariant_violations.is_empty() {
        println!("\nInvariant violations:");
        for violation in &report.metrics.invariant_violations {
            println!("  - {}", violation);
        }
    }

    Ok(())
}

/// Demonstrates testing various edge cases.
fn demonstrate_edge_case_tests() -> Result<(), Box<dyn std::error::Error>> {
    // Test 1: Severe network degradation
    println!("Testing severe network degradation...");
    match streaming_edge_cases::severe_network_degradation_scenario() {
        Ok(report) => {
            println!(
                "  Completed: {} pieces",
                report.final_state.completed_pieces.len()
            );
            println!(
                "  Network changes: {}",
                report
                    .metrics
                    .events_by_type
                    .get("NetworkChange")
                    .unwrap_or(&0)
            );
        }
        Err(e) => {
            println!("  Failed as expected: {}", e);
        }
    }

    // Test 2: Extreme peer churn
    println!("\nTesting extreme peer churn...");
    match streaming_edge_cases::extreme_peer_churn_scenario() {
        Ok(report) => {
            println!("  Peak connections: {}", report.metrics.peak_connections);
            println!("  Peer disconnects: {}", report.metrics.peer_disconnects);
        }
        Err(e) => {
            println!("  Failed: {}", e);
        }
    }

    // Test 3: Cascading failures
    println!("\nTesting cascading piece failures...");
    match streaming_edge_cases::cascading_piece_failures_scenario() {
        Ok(report) => {
            println!("  Piece failures: {}", report.metrics.piece_failures);
            println!(
                "  Retry attempts: {}",
                report
                    .metrics
                    .events_by_type
                    .get("PieceRequest")
                    .unwrap_or(&0)
            );
        }
        Err(e) => {
            println!("  Failed: {}", e);
        }
    }

    Ok(())
}

/// Demonstrates performance validation through simulation.
fn demonstrate_performance_tests() -> Result<(), SimulationError> {
    // Test different peer counts
    let peer_counts = vec![5, 10, 20, 50];
    let mut results = Vec::new();

    for peer_count in peer_counts {
        let config = SimulationConfig {
            enabled: true,
            deterministic_seed: Some(99999),
            network_latency_ms: 50,
            packet_loss_rate: 0.01,
            max_simulated_peers: peer_count,
            simulated_download_speed: 10_485_760, // 10 MB/s
            use_mock_data: true,
        };

        let mut sim = DeterministicSimulation::new(config)?;
        let info_hash = InfoHash::new([0x99; 20]);

        sim.create_streaming_scenario(info_hash, 200, 262144)?;

        let start = std::time::Instant::now();
        let report = sim.run_for(Duration::from_secs(60))?;
        let elapsed = start.elapsed();

        results.push((peer_count, report, elapsed));
    }

    // Analyze results
    println!("Performance test results:");
    println!(
        "{:<10} {:<15} {:<15} {:<15}",
        "Peers", "Completed", "Failed", "Sim Time (ms)"
    );
    println!("{:-<55}", "");

    for (peer_count, report, elapsed) in results {
        println!(
            "{:<10} {:<15} {:<15} {:<15}",
            peer_count,
            report.final_state.completed_pieces.len(),
            report.final_state.failed_pieces.len(),
            elapsed.as_millis()
        );
    }

    Ok(())
}

/// Demonstrates how to reproduce a specific bug using deterministic simulation.
fn reproduce_specific_bug() -> Result<(), SimulationError> {
    // Scenario: Bug where pieces are requested multiple times from same peer
    // causing bandwidth waste and potential corruption

    let config = SimulationConfig {
        enabled: true,
        deterministic_seed: Some(666), // Seed that reproduces the bug
        network_latency_ms: 100,
        packet_loss_rate: 0.03,
        max_simulated_peers: 10,
        simulated_download_speed: 2_097_152, // 2 MB/s
        use_mock_data: true,
    };

    let mut sim = DeterministicSimulation::new(config)?;

    // Add custom invariant to detect duplicate requests
    sim.add_invariant(Arc::new(NoDuplicateRequestsInvariant::new()));

    // Set up scenario that triggers the bug
    let info_hash = InfoHash::new([0x66; 20]);

    // Manually schedule events that expose the bug
    for i in 0..5 {
        let peer_id = format!("PEER_{}", i);

        // Connect peer
        sim.schedule_delayed(
            Duration::from_millis(i * 100),
            EventType::PeerConnect {
                peer_id: peer_id.clone(),
            },
            EventPriority::Normal,
        )?;

        // Request same piece from multiple peers (bug condition)
        sim.schedule_delayed(
            Duration::from_millis(500 + i * 50),
            EventType::PieceRequest {
                peer_id: peer_id.clone(),
                piece_index: PieceIndex::new(42), // Same piece!
            },
            EventPriority::Normal,
        )?;
    }

    // Run simulation
    match sim.run_for(Duration::from_secs(10)) {
        Ok(report) => {
            println!("Bug NOT reproduced - invariants held");
            println!("Summary: {}", report.summary());
        }
        Err(e) => {
            println!("Bug reproduced! Error: {}", e);
            println!("This demonstrates the duplicate request issue");
            println!("Fix: Implement proper piece request tracking");
        }
    }

    Ok(())
}

/// Custom invariant to detect duplicate piece requests.
struct NoDuplicateRequestsInvariant {
    active_requests: std::sync::Mutex<std::collections::HashMap<PieceIndex, String>>,
}

impl NoDuplicateRequestsInvariant {
    fn new() -> Self {
        Self {
            active_requests: std::sync::Mutex::new(std::collections::HashMap::new()),
        }
    }
}

impl riptide_sim::SimulationInvariant for NoDuplicateRequestsInvariant {
    fn check(
        &self,
        state: &riptide_sim::SimulationState,
    ) -> Result<(), riptide_sim::InvariantViolation> {
        // Check if multiple peers are downloading same piece
        for (piece, peer) in &state.downloading_pieces {
            let mut active = self.active_requests.lock().unwrap();

            if let Some(existing_peer) = active.get(piece) {
                if existing_peer != peer {
                    return Err(riptide_sim::InvariantViolation {
                        invariant: self.name().to_string(),
                        description: format!(
                            "Piece {} requested by {} but already being downloaded by {}",
                            piece, peer, existing_peer
                        ),
                        timestamp: state.timestamp,
                    });
                }
            } else {
                active.insert(*piece, peer.clone());
            }
        }

        Ok(())
    }

    fn name(&self) -> &str {
        "NoDuplicateRequests"
    }
}

/// Example of using simulation for A/B testing different strategies.
#[allow(dead_code)]
fn ab_test_piece_selection_strategies() -> Result<(), SimulationError> {
    // Strategy A: Sequential piece selection
    let report_a = execute_simulation_with_strategy(12345, "sequential")?;

    // Strategy B: Rarest-first piece selection
    let report_b = execute_simulation_with_strategy(12345, "rarest_first")?;

    // Compare results
    println!("A/B Test Results:");
    println!(
        "Sequential: {} pieces in {:?}",
        report_a.final_state.completed_pieces.len(),
        report_a.duration
    );
    println!(
        "Rarest-first: {} pieces in {:?}",
        report_b.final_state.completed_pieces.len(),
        report_b.duration
    );

    Ok(())
}

fn execute_simulation_with_strategy(seed: u64, _strategy: &str) -> Result<SimulationReport, SimulationError> {
    let config = SimulationConfig {
        enabled: true,
        deterministic_seed: Some(seed),
        network_latency_ms: 50,
        packet_loss_rate: 0.01,
        max_simulated_peers: 20,
        simulated_download_speed: 5_242_880,
        use_mock_data: true,
    };

    let mut sim = DeterministicSimulation::new(config)?;
    let info_hash = InfoHash::new([0xAB; 20]);

    // In real implementation, would configure piece picker strategy here
    sim.create_streaming_scenario(info_hash, 100, 262144)?;

    sim.run_for(Duration::from_secs(30))
}
