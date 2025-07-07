//! Unified simulation framework - mock-object-based approach.
//!
//! This module implements a simplified, focused simulation strategy using
//! mock objects as the foundation. It replaces the overlapping approaches
//! of event-driven simulation, auto-responding mocks, and real network simulation.

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use riptide_core::torrent::{PeerManager, PieceStore};

use crate::piece_store::InMemoryPieceStore;
use crate::simulated_peer_manager::{InMemoryPeerConfig, InMemoryPeerManager};
use crate::tracker::SimulatedTrackerManager;

/// Unified simulation environment using mock objects as foundation.
///
/// This approach replaces the previous overlapping simulation strategies:
/// - No event-driven scheduling complexity
/// - No auto-responding mock behavior
/// - No real network servers for basic testing
///
/// Instead, provides explicit control over mock components with optional
/// deterministic scheduling for complex test scenarios.
pub struct UnifiedSimulation {
    peer_manager: InMemoryPeerManager,
    tracker_manager: SimulatedTrackerManager,
    piece_store: Arc<InMemoryPieceStore>,
    // Optional deterministic scheduler for complex tests
    _deterministic_scheduler: Option<EventScheduler>,
}

impl UnifiedSimulation {
    /// Creates simple mock environment for development.
    ///
    /// Uses default configurations with minimal complexity.
    /// Suitable for basic functionality testing and development.
    pub fn new_simple(total_pieces: u32) -> Self {
        let config = InMemoryPeerConfig {
            message_delay_ms: 1,          // Minimal delay for fast development
            connection_failure_rate: 0.0, // No failures for simple testing
            message_loss_rate: 0.0,       // No message loss
            max_connections: 50,
        };

        let peer_manager = InMemoryPeerManager::with_config(config, total_pieces);
        let tracker_manager = SimulatedTrackerManager::new();
        let piece_store = Arc::new(InMemoryPieceStore::new());

        Self {
            peer_manager,
            tracker_manager,
            piece_store,
            _deterministic_scheduler: None,
        }
    }

    /// Creates deterministic mock environment for testing.
    ///
    /// Uses fixed seed for reproducible behavior.
    /// Includes realistic network conditions and failures.
    /// Suitable for comprehensive testing and CI.
    pub fn new_deterministic(seed: u64, total_pieces: u32) -> Self {
        // Set deterministic seed for reproducible behavior
        use rand::{Rng, SeedableRng};
        let mut rng = rand::rngs::StdRng::seed_from_u64(seed);

        let config = InMemoryPeerConfig {
            message_delay_ms: 10 + rng.random_range(0..50), // 10-60ms variation
            connection_failure_rate: 0.05,                  // 5% failure rate
            message_loss_rate: 0.001,                       // 0.1% message loss
            max_connections: 100,
        };

        let peer_manager = InMemoryPeerManager::with_config(config, total_pieces);
        let tracker_manager = SimulatedTrackerManager::new();
        let piece_store = Arc::new(InMemoryPieceStore::new());

        Self {
            peer_manager,
            tracker_manager,
            piece_store,
            _deterministic_scheduler: Some(EventScheduler::new(seed)),
        }
    }

    /// Get access to the peer manager for explicit control.
    pub fn peer_manager(&mut self) -> &mut InMemoryPeerManager {
        &mut self.peer_manager
    }

    /// Get access to the tracker manager for configuration.
    pub fn tracker_manager(&mut self) -> &mut SimulatedTrackerManager {
        &mut self.tracker_manager
    }

    /// Get access to the piece store for data setup.
    pub fn piece_store(&self) -> &Arc<InMemoryPieceStore> {
        &self.piece_store
    }

    /// Explicit test helper: Make a peer respond with Unchoke message.
    ///
    /// Tests must explicitly control peer interactions instead of relying
    /// on automatic responses. This prevents protocol bugs from being masked.
    pub async fn make_peer_respond_unchoke(
        &mut self,
        peer_addr: SocketAddr,
    ) -> Result<(), SimulationError> {
        self.peer_manager
            .simulate_peer_message(peer_addr, riptide_core::torrent::PeerMessage::Unchoke)
            .await
            .map_err(|e| SimulationError::MockOperationFailed {
                reason: format!("Failed to send unchoke: {e}"),
            })
    }

    /// Explicit test helper: Make a peer respond with piece data.
    ///
    /// Tests must explicitly define what piece data should be returned
    /// rather than relying on automatic responses.
    pub async fn make_peer_respond_piece(
        &mut self,
        peer_addr: SocketAddr,
        piece_index: riptide_core::torrent::PieceIndex,
        data: Vec<u8>,
    ) -> Result<(), SimulationError> {
        let message = riptide_core::torrent::PeerMessage::Piece {
            piece_index,
            offset: 0,
            data: data.into(),
        };

        self.peer_manager
            .simulate_peer_message(peer_addr, message)
            .await
            .map_err(|e| SimulationError::MockOperationFailed {
                reason: format!("Failed to send piece: {e}"),
            })
    }

    /// Schedule explicit peer action (when using deterministic scheduler).
    ///
    /// Instead of abstract events, this schedules concrete actions on mock objects.
    /// Actions are executed by directly calling mock object methods.
    pub async fn schedule_peer_action(&mut self, delay: Duration, action: PeerAction) {
        if let Some(scheduler) = &mut self._deterministic_scheduler {
            scheduler.schedule_action(delay, action).await;
        } else {
            // Execute immediately for simple simulation
            self.execute_peer_action(action).await;
        }
    }

    /// Execute a peer action immediately.
    async fn execute_peer_action(&mut self, action: PeerAction) {
        match action {
            PeerAction::Disconnect(addr) => {
                let _ = self.peer_manager.disconnect_peer(addr).await;
            }
            PeerAction::SendMessage(addr, message) => {
                let _ = self.peer_manager.simulate_peer_message(addr, message).await;
            }
            PeerAction::TriggerConnectionFailure(addr) => {
                self.peer_manager.trigger_connection_failure(addr).await;
            }
            PeerAction::ConfigurePeerHasPiece(addr, piece_index) => {
                self.peer_manager
                    .configure_peer_has_piece(addr, piece_index)
                    .await;
            }
        }
    }

    /// Create server components using this simulation's mock objects.
    ///
    /// This integrates the unified simulation with the main application
    /// by providing the mock implementations as server components.
    pub async fn create_server_components(
        self,
        config: riptide_core::config::RiptideConfig,
    ) -> Result<riptide_core::server_components::ServerComponents, SimulationError> {
        use riptide_core::server_components::ServerComponents;
        use riptide_core::torrent::spawn_torrent_engine;

        let engine = spawn_torrent_engine(config, self.peer_manager, self.tracker_manager);

        Ok(ServerComponents {
            torrent_engine: engine,
            movie_manager: None, // Not needed for basic simulation
            piece_store: Some(self.piece_store as Arc<dyn PieceStore>),
            conversion_progress: None,
            ffmpeg_processor: Arc::new(riptide_core::streaming::SimulationFfmpegProcessor::new()),
        })
    }
}

/// Concrete actions that can be performed on mock objects.
///
/// These replace abstract events with explicit operations on mock components.
/// Tests must explicitly define what should happen and when.
pub enum PeerAction {
    /// Disconnect a peer from the simulation.
    Disconnect(SocketAddr),
    /// Send a message from a peer to the manager.
    SendMessage(SocketAddr, riptide_core::torrent::PeerMessage),
    /// Trigger connection failure for a specific peer.
    TriggerConnectionFailure(SocketAddr),
    /// Configure a peer to have a specific piece.
    ConfigurePeerHasPiece(SocketAddr, riptide_core::torrent::PieceIndex),
}

/// Optional event scheduler for deterministic complex tests.
///
/// This is kept minimal and only used when deterministic timing is critical.
/// Most tests should use immediate execution for simplicity.
pub struct EventScheduler {
    _seed: u64,
    _scheduled_actions: Vec<(Duration, PeerAction)>,
}

impl EventScheduler {
    fn new(seed: u64) -> Self {
        Self {
            _seed: seed,
            _scheduled_actions: Vec::new(),
        }
    }

    async fn schedule_action(&mut self, _delay: Duration, _action: PeerAction) {
        // TODO: Implement simple time-based scheduling if needed
        // For now, most tests should use immediate execution
    }
}

/// Simplified error type for unified simulation.
#[derive(Debug, thiserror::Error)]
pub enum SimulationError {
    #[error("Failed to initialize simulation: {reason}")]
    InitializationFailed { reason: String },

    #[error("Mock object operation failed: {reason}")]
    MockOperationFailed { reason: String },
}

#[cfg(test)]
mod tests {
    use riptide_core::torrent::InfoHash;

    use super::*;

    #[tokio::test]
    async fn test_simple_simulation_creation() {
        let sim = UnifiedSimulation::new_simple(100);
        // Test that simulation is created successfully
        assert!(sim._deterministic_scheduler.is_none());
    }

    #[tokio::test]
    async fn test_deterministic_simulation_creation() {
        let sim = UnifiedSimulation::new_deterministic(12345, 100);
        assert!(sim._deterministic_scheduler.is_some());
    }

    #[tokio::test]
    async fn test_explicit_peer_control() {
        let mut sim = UnifiedSimulation::new_simple(100);

        // Explicitly inject a peer for testing
        let addr = "192.168.1.100:6881".parse().unwrap();
        let info_hash = InfoHash::from_hex("0123456789abcdef0123456789abcdef01234567").unwrap();
        let has_pieces = vec![true; 100]; // Peer has all pieces

        sim.peer_manager
            .inject_peer(addr, info_hash, has_pieces)
            .await;

        // Verify peer was added
        assert!(
            sim.peer_manager
                .peer_has_piece(addr, riptide_core::torrent::PieceIndex::new(0))
                .await
        );
    }

    #[tokio::test]
    async fn test_peer_action_execution() {
        let mut sim = UnifiedSimulation::new_simple(100);

        // Setup peer
        let addr = "192.168.1.100:6881".parse().unwrap();
        let info_hash = InfoHash::from_hex("0123456789abcdef0123456789abcdef01234567").unwrap();
        let has_pieces = vec![false; 100];

        sim.peer_manager
            .inject_peer(addr, info_hash, has_pieces)
            .await;

        // Execute action to give peer a piece
        let action =
            PeerAction::ConfigurePeerHasPiece(addr, riptide_core::torrent::PieceIndex::new(5));
        sim.execute_peer_action(action).await;

        // Verify action was executed
        assert!(
            sim.peer_manager
                .peer_has_piece(addr, riptide_core::torrent::PieceIndex::new(5))
                .await
        );
    }

    #[tokio::test]
    async fn test_explicit_mock_control() {
        let mut sim = UnifiedSimulation::new_simple(100);

        // Setup peer
        let addr = "192.168.1.100:6881".parse().unwrap();
        let info_hash = InfoHash::from_hex("0123456789abcdef0123456789abcdef01234567").unwrap();
        let has_pieces = vec![true; 100]; // Peer has all pieces

        sim.peer_manager
            .inject_peer(addr, info_hash, has_pieces)
            .await;

        // Test explicit control: Tests must explicitly make peers respond
        // No automatic responses occur - this is intentional

        // Explicit unchoke response
        sim.make_peer_respond_unchoke(addr).await.unwrap();

        // Explicit piece response with custom data
        let piece_data = b"test piece data".to_vec();
        sim.make_peer_respond_piece(addr, riptide_core::torrent::PieceIndex::new(0), piece_data)
            .await
            .unwrap();

        // This test demonstrates that tests must explicitly control all interactions
        // instead of relying on automatic behaviors that could mask bugs
    }
}
