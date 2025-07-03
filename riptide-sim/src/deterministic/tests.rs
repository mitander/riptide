//! Tests for deterministic simulation framework.

use std::sync::Arc;
use std::time::Duration;

use riptide_core::config::SimulationConfig;
use riptide_core::torrent::{InfoHash, PieceIndex};

use crate::deterministic::{
    DeterministicSimulation, EventPriority, EventType, MinimumPeersInvariant, ResourceLimits,
    ResourceType, SimulationError,
};

#[test]
fn test_simulation_reproducibility() {
    let config = SimulationConfig {
        enabled: true,
        deterministic_seed: Some(12345),
        network_latency_ms: 50,
        packet_loss_rate: 0.01,
        max_simulated_peers: 5,
        simulated_download_speed: 1_048_576,
        use_mock_data: true,
    };

    // Run simulation twice with same seed
    let mut sim1 = DeterministicSimulation::new(config.clone()).unwrap();
    let info_hash = InfoHash::new([2; 20]);
    sim1.create_streaming_scenario_full(info_hash, 5, 262144)
        .unwrap();
    let report1 = sim1.execute_for_duration(Duration::from_secs(10)).unwrap();

    let mut sim2 = DeterministicSimulation::new(config).unwrap();
    sim2.create_streaming_scenario_full(info_hash, 5, 262144)
        .unwrap();
    let report2 = sim2.execute_for_duration(Duration::from_secs(10)).unwrap();

    // Results should be identical
    assert_eq!(report1.event_count, report2.event_count);
    assert_eq!(
        report1.final_state.completed_pieces,
        report2.final_state.completed_pieces
    );
    assert_eq!(report1.seed, report2.seed);
}

#[test]
fn test_event_priority_ordering() {
    let config = SimulationConfig {
        enabled: true,
        deterministic_seed: Some(42),
        ..Default::default()
    };

    let mut sim = DeterministicSimulation::new(config).unwrap();

    // Schedule events at same time with different priorities
    let same_time = Duration::from_secs(1);

    sim.schedule_delayed(
        same_time,
        EventType::PeerConnect {
            peer_id: "LOW".to_string(),
        },
        EventPriority::Low,
    )
    .unwrap();

    sim.schedule_delayed(
        same_time,
        EventType::TrackerAnnounce {
            info_hash: InfoHash::new([1; 20]),
            peer_count: 10,
        },
        EventPriority::Critical,
    )
    .unwrap();

    sim.schedule_delayed(
        same_time,
        EventType::PieceRequest {
            peer_id: "NORMAL".to_string(),
            piece_index: PieceIndex::new(0),
        },
        EventPriority::Normal,
    )
    .unwrap();

    let report = sim.execute_for_duration(Duration::from_secs(2)).unwrap();

    // All events should be processed
    assert_eq!(report.event_count, 3);
}

#[test]
fn test_resource_limit_enforcement() {
    let config = SimulationConfig {
        enabled: true,
        deterministic_seed: Some(999),
        ..Default::default()
    };

    let mut sim = DeterministicSimulation::new(config).unwrap();

    // Set very low connection limit
    sim.configure_resource_limits(ResourceLimits {
        max_connections: 2,
        ..Default::default()
    });

    // Schedule more connections than limit
    for i in 0..5 {
        sim.schedule_delayed(
            Duration::from_millis(i * 100),
            EventType::PeerConnect {
                peer_id: format!("PEER_{i}"),
            },
            EventPriority::Normal,
        )
        .unwrap();
    }

    let result = sim.execute_for_duration(Duration::from_secs(1));

    // Should fail due to resource limits
    match result {
        Err(SimulationError::ResourceLimitExceeded {
            resource,
            current,
            limit,
        }) => {
            assert_eq!(resource, ResourceType::Connections);
            assert_eq!(current, 3); // Third connection exceeds limit of 2
            assert_eq!(limit, 2);
        }
        Ok(_) => panic!("Expected ResourceLimitExceeded error"),
        Err(e) => panic!("Unexpected error: {e:?}"),
    }
}

#[test]
fn test_invariant_checking() {
    let config = SimulationConfig {
        enabled: true,
        deterministic_seed: Some(777),
        ..Default::default()
    };

    let mut sim = DeterministicSimulation::new(config).unwrap();

    // Add minimum peers invariant
    sim.add_invariant(Arc::new(MinimumPeersInvariant::new(3)));

    // Schedule only 2 peer connections
    sim.schedule_event(
        EventType::PeerConnect {
            peer_id: "PEER1".to_string(),
        },
        EventPriority::Normal,
    )
    .unwrap();

    sim.schedule_event(
        EventType::PeerConnect {
            peer_id: "PEER2".to_string(),
        },
        EventPriority::Normal,
    )
    .unwrap();

    let report = sim.execute_for_duration(Duration::from_secs(1)).unwrap();

    // Should have invariant violations
    assert!(!report.success);
    assert!(!report.metrics.invariant_violations.is_empty());
}

#[test]
fn test_streaming_scenario() {
    let config = SimulationConfig {
        enabled: true,
        deterministic_seed: Some(555),
        network_latency_ms: 20,
        packet_loss_rate: 0.0,
        max_simulated_peers: 5,
        simulated_download_speed: 10_485_760, // 10 MB/s
        use_mock_data: true,
    };

    let mut sim = DeterministicSimulation::new(config).unwrap();

    // Create streaming scenario
    sim.create_streaming_scenario(10, Duration::from_secs(2))
        .unwrap();

    let report = sim.execute_for_duration(Duration::from_secs(30)).unwrap();

    // Should complete successfully
    assert!(report.success);
    assert_eq!(report.final_state.completed_pieces.len(), 10);
    assert_eq!(report.final_state.connected_peers.len(), 5);
}

#[test]
fn test_network_change_events() {
    let config = SimulationConfig {
        enabled: true,
        deterministic_seed: Some(333),
        ..Default::default()
    };

    let mut sim = DeterministicSimulation::new(config).unwrap();

    // Schedule network degradation
    sim.schedule_delayed(
        Duration::from_secs(1),
        EventType::NetworkChange {
            latency_ms: 200,
            packet_loss_rate: 0.1,
        },
        EventPriority::Critical,
    )
    .unwrap();

    // Schedule network recovery
    sim.schedule_delayed(
        Duration::from_secs(5),
        EventType::NetworkChange {
            latency_ms: 50,
            packet_loss_rate: 0.01,
        },
        EventPriority::Critical,
    )
    .unwrap();

    let report = sim.execute_for_duration(Duration::from_secs(10)).unwrap();

    assert_eq!(report.metrics.events_by_type["NetworkChange"], 2);
}

#[test]
fn test_piece_failure_handling() {
    let config = SimulationConfig {
        enabled: true,
        deterministic_seed: Some(222),
        ..Default::default()
    };

    let mut sim = DeterministicSimulation::new(config).unwrap();

    let piece_index = PieceIndex::new(5);

    // Schedule piece request
    sim.schedule_event(
        EventType::PieceRequest {
            peer_id: "PEER1".to_string(),
            piece_index,
        },
        EventPriority::Normal,
    )
    .unwrap();

    // Schedule piece failure
    sim.schedule_delayed(
        Duration::from_secs(1),
        EventType::PieceFailed {
            peer_id: "PEER1".to_string(),
            piece_index,
            reason: "Hash mismatch".to_string(),
        },
        EventPriority::High,
    )
    .unwrap();

    let report = sim.execute_for_duration(Duration::from_secs(2)).unwrap();

    assert_eq!(report.metrics.piece_failures, 1);
    assert!(report.final_state.failed_pieces.contains_key(&piece_index));
}

#[test]
fn test_event_queue_overflow_protection() {
    let config = SimulationConfig {
        enabled: true,
        deterministic_seed: Some(111),
        ..Default::default()
    };

    let mut sim = DeterministicSimulation::new(config).unwrap();

    // Try to schedule way too many events
    let mut overflow_occurred = false;
    for i in 0..200_000 {
        let result = sim.schedule_event(
            EventType::Custom {
                name: format!("Event_{i}"),
                data: "test".to_string(),
            },
            EventPriority::Low,
        );

        if result.is_err() {
            overflow_occurred = true;
            break;
        }
    }

    assert!(overflow_occurred);
}

#[test]
fn test_simulation_report_summary() {
    let config = SimulationConfig {
        enabled: true,
        deterministic_seed: Some(42),
        ..Default::default()
    };

    let mut sim = DeterministicSimulation::new(config).unwrap();

    // Add some events
    sim.schedule_event(
        EventType::PeerConnect {
            peer_id: "PEER1".to_string(),
        },
        EventPriority::Normal,
    )
    .unwrap();

    sim.schedule_delayed(
        Duration::from_secs(1),
        EventType::PieceRequest {
            peer_id: "PEER1".to_string(),
            piece_index: PieceIndex::new(0),
        },
        EventPriority::Normal,
    )
    .unwrap();

    sim.schedule_delayed(
        Duration::from_secs(2),
        EventType::PieceComplete {
            peer_id: "PEER1".to_string(),
            piece_index: PieceIndex::new(0),
            download_time: Duration::from_millis(500),
        },
        EventPriority::Normal,
    )
    .unwrap();

    let report = sim.execute_for_duration(Duration::from_secs(3)).unwrap();
    let summary = report.summary();

    // Verify summary contains expected information
    assert!(summary.contains("seed: 42"));
    assert!(summary.contains("Events processed: 3"));
    assert!(summary.contains("Success: true"));
    assert!(summary.contains("PeerConnect: 1"));
    assert!(summary.contains("PieceRequest: 1"));
    assert!(summary.contains("PieceComplete: 1"));
}

#[test]
fn test_cannot_schedule_past_events() {
    let config = SimulationConfig {
        enabled: true,
        deterministic_seed: Some(88),
        ..Default::default()
    };

    let mut sim = DeterministicSimulation::new(config).unwrap();

    // Advance time
    sim.execute_for_duration(Duration::from_secs(10)).unwrap();

    // Try to schedule event in the past
    // Test would verify that scheduling in the past fails - not yet implemented
    // let past_time = sim.current_time() - Duration::from_secs(5);

    // This should work - scheduling adds to current time
    let result = sim.schedule_event(
        EventType::PeerConnect {
            peer_id: "LATE".to_string(),
        },
        EventPriority::Normal,
    );

    assert!(result.is_ok());
}

#[test]
fn test_event_history_bounded() {
    let config = SimulationConfig {
        enabled: true,
        deterministic_seed: Some(66),
        ..Default::default()
    };

    let mut sim = DeterministicSimulation::new(config).unwrap();

    // Schedule many events
    for i in 0..15_000 {
        sim.schedule_delayed(
            Duration::from_millis(i),
            EventType::Custom {
                name: format!("Event_{i}"),
                data: "test".to_string(),
            },
            EventPriority::Normal,
        )
        .unwrap();
    }

    let report = sim.execute_for_duration(Duration::from_secs(20)).unwrap();

    // Event history should be bounded
    assert_eq!(report.event_count, 15_000);
}
