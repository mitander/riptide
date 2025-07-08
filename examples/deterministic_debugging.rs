//! Example of using deterministic simulation for bug reproduction
//!
//! Demonstrates how to create reproducible test scenarios that help
//! identify and fix streaming-related bugs in BitTorrent implementation.

use std::time::Duration;

use riptide::simulation::{DeterministicSimulation, ScenarioRunner, SimulationScenarios};

/// Example of reproducing a specific bug scenario.
///
/// This demonstrates how a bug report with a specific seed can be
/// reproduced exactly for debugging and validation.
fn reproduce_bug_report() {
    println!("Reproducing Bug Report #42: Streaming stalls during peer churn");
    println!("{:-<60}", "");

    // Bug report provides this seed that reproduces the issue
    let bug_seed = 0xDEADBEEF;

    let mut simulation = SimulationScenarios::peer_churn(bug_seed);

    println!("Running simulation with seed: 0x{bug_seed:X}");

    // Run simulation and capture events
    let events = simulation.run_for(Duration::from_secs(120));

    println!("Simulation completed. Total events: {}", events.len());

    // Analyze events for the specific bug pattern
    let piece_requests = events
        .iter()
        .filter(|e| matches!(e.event_type, riptide::simulation::EventType::PieceRequest))
        .count();

    let piece_completions = events
        .iter()
        .filter(|e| matches!(e.event_type, riptide::simulation::EventType::PieceComplete))
        .count();

    let peer_disconnects = events
        .iter()
        .filter(|e| matches!(e.event_type, riptide::simulation::EventType::PeerDisconnect))
        .count();

    println!("Analysis:");
    println!("  Piece requests: {piece_requests}");
    println!("  Piece completions: {piece_completions}");
    println!("  Peer disconnections: {peer_disconnects}");

    let completion_rate = if piece_requests > 0 {
        piece_completions as f64 / piece_requests as f64 * 100.0
    } else {
        0.0
    };

    println!("  Completion rate: {completion_rate:.1}%");

    if completion_rate < 85.0 {
        println!("  BUG REPRODUCED: Low completion rate during peer churn");
        println!("  Investigation needed: piece request retry logic");
    } else {
        println!("  Bug appears to be fixed");
    }
}

/// Example of testing streaming performance under different conditions.
fn streaming_performance_analysis() {
    println!("\nStreaming Performance Analysis");
    println!("{:-<60}", "");

    let test_seed = 12345;

    // Test multiple scenarios for streaming performance
    let scenarios = [
        ("Ideal conditions", "ideal"),
        ("Slow network", "slow"),
        ("Peer churn", "churn"),
        ("Mixed peers", "mixed"),
    ];

    for (name, scenario_type) in &scenarios {
        println!("Testing {name}");

        let mut simulation = match *scenario_type {
            "ideal" => SimulationScenarios::ideal_streaming(test_seed),
            "slow" => SimulationScenarios::slow_network(test_seed),
            "churn" => SimulationScenarios::peer_churn(test_seed),
            "mixed" => SimulationScenarios::mixed_peers(test_seed),
            _ => SimulationScenarios::ideal_streaming(test_seed),
        };

        let start_time = simulation.clock().now();
        let events = simulation.run_for(Duration::from_secs(60));
        let end_time = simulation.clock().now();

        // Calculate streaming metrics
        let piece_requests = events
            .iter()
            .filter(|e| matches!(e.event_type, riptide::simulation::EventType::PieceRequest))
            .count();

        let piece_completions = events
            .iter()
            .filter(|e| matches!(e.event_type, riptide::simulation::EventType::PieceComplete))
            .count();

        let success_rate = if piece_requests > 0 {
            piece_completions as f64 / piece_requests as f64
        } else {
            0.0
        };

        println!("  Success rate: {:.1}%", success_rate * 100.0);
        println!("  Total events: {}", events.len());
        println!(
            "  Simulation time: {:?}",
            end_time.duration_since(start_time)
        );
        println!();
    }
}

/// Example of systematic regression testing.
fn regression_test_suite() {
    println!("Regression Test Suite");
    println!("{:-<60}", "");

    let regression_seeds = [
        (0xABCDEF, "Known good baseline"),
        (0x123456, "Previous bug reproduction"),
        (0xDEADBEEF, "Peer churn issue"),
        (0xCAFEBABE, "Piece failure cascade"),
    ];

    for (seed, description) in &regression_seeds {
        println!("Testing seed 0x{seed:X}: {description}");

        let mut runner = ScenarioRunner::new(*seed);
        let results = runner.run_all_scenarios();

        // Check for regression indicators
        let mut passed = true;

        if let Some(ideal) = results.get_result("ideal_streaming") {
            if ideal.success_rate() < 0.95 {
                println!(
                    "  REGRESSION: Ideal scenario success rate: {:.1}%",
                    ideal.success_rate() * 100.0
                );
                passed = false;
            }
        }

        if let Some(churn) = results.get_result("peer_churn") {
            if churn.success_rate() < 0.75 {
                println!(
                    "  REGRESSION: Peer churn scenario success rate: {:.1}%",
                    churn.success_rate() * 100.0
                );
                passed = false;
            }
        }

        if passed {
            println!("  All scenarios passed");
        }

        println!();
    }
}

/// Example of performance benchmarking with deterministic results.
fn performance_benchmarking() {
    println!("Performance Benchmarking");
    println!("{:-<60}", "");

    let benchmark_seed = 0x1337BEEF;

    // Create baseline scenario for consistent benchmarking
    let mut sim = DeterministicSimulation::from_seed(benchmark_seed);

    // Add consistent peer set
    for i in 0..20 {
        let peer_id = format!("BENCH{i:03}");
        sim.add_deterministic_peer(peer_id, 5_000_000); // 5 MB/s
    }

    // Create streaming workload: 1000 pieces, 256KB each = ~256MB
    sim.create_streaming_scenario(1000, Duration::from_secs(1));

    println!("Benchmarking 256MB streaming download...");

    let start_time = sim.clock().now();
    let events = sim.run_for(Duration::from_secs(300)); // 5 minute max
    let end_time = sim.clock().now();

    let piece_completions = events
        .iter()
        .filter(|e| matches!(e.event_type, riptide::simulation::EventType::PieceComplete))
        .count();

    let total_mb = (piece_completions * 256) as f64 / 1024.0; // 256KB pieces -> MB
    let duration_secs = end_time.duration_since(start_time).as_secs_f64();
    let throughput_mbps = (total_mb * 8.0) / duration_secs; // Convert to Mbps

    println!("Results:");
    println!("  Pieces completed: {piece_completions} / 1000");
    println!("  Data transferred: {total_mb:.1} MB");
    println!("  Duration: {duration_secs:.1} seconds");
    println!("  Throughput: {throughput_mbps:.1} Mbps");
    println!("  Events processed: {}", events.len());

    // Performance thresholds
    if throughput_mbps < 10.0 {
        println!("  Performance below threshold (10 Mbps)");
    } else {
        println!("  Performance meets requirements");
    }
}

fn main() {
    println!("Deterministic BitTorrent Simulation Examples");
    println!("============================================\n");

    // Example 1: Bug reproduction
    reproduce_bug_report();

    // Example 2: Performance analysis
    streaming_performance_analysis();

    // Example 3: Regression testing
    regression_test_suite();

    // Example 4: Performance benchmarking
    performance_benchmarking();

    println!("Examples completed. Use these patterns for:");
    println!("• Reproducing user-reported bugs with specific seeds");
    println!("• Validating streaming performance under various conditions");
    println!("• Automated regression testing in CI/CD");
    println!("• Consistent performance benchmarking");
}
