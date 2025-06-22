//! Integration tests for simulation environment

use riptide::simulation::SimulationEnvironment;

#[tokio::test]
async fn test_simulation_environment_for_streaming() {
    let env = SimulationEnvironment::for_streaming();
    
    // Should create mixed peer types
    assert_eq!(env.peers.len(), 17); // 10 fast + 5 slow + 2 unreliable
    
    // Verify tracker is configured
    let info_hash = [0u8; 20];
    let response = env.tracker.announce(&info_hash).await;
    assert!(response.is_ok());
}

#[tokio::test]
async fn test_network_simulator_bandwidth_delay() {
    let env = SimulationEnvironment::for_streaming();
    
    // 50 Mbps limit should introduce delay for large transfers
    let delay = env.network.bandwidth_delay(1_000_000); // 1 MB
    assert!(delay.as_millis() > 0);
}

#[tokio::test]
async fn test_mock_peer_send_piece() {
    let env = SimulationEnvironment::for_streaming();
    
    // Test first peer can send data
    if let Some(peer) = env.peers.first() {
        let result = peer.send_piece(16384).await; // 16 KiB piece
        assert!(result.is_ok());
        
        let data = result.unwrap();
        assert_eq!(data.len(), 16384);
    }
}