//! Peer management implementations for simulation and development.
//!
//! This module provides different peer manager implementations optimized for
//! specific use cases:
//!
//! - **Development**: Fast iteration with realistic streaming rates
//! - **Deterministic**: Reproducible simulation for testing and bug reproduction
//! - **Piece Store**: In-memory storage for simulation data
//!
//! # Examples
//!
//! ## Development Mode
//! ```rust,no_run
//! use riptide_sim::peers::{SimulatedPeers, InMemoryPieceStore};
//! use std::sync::Arc;
//!
//! let piece_store = Arc::new(InMemoryPieceStore::new());
//! let peers = SimulatedPeers::new_instant(piece_store);
//! ```
//!
//! ## Simulated Testing
//! ```rust,no_run
//! use riptide_sim::peers::{SimulatedPeers, SimulatedConfig, InMemoryPieceStore};
//! use std::sync::Arc;
//!
//! let piece_store = Arc::new(InMemoryPieceStore::new());
//! let config = SimulatedConfig::ideal();
//! let peers = SimulatedPeers::new(config, piece_store);
//! ```

pub mod piece_store;
pub mod simulated;

use std::sync::Arc;

pub use piece_store::InMemoryPieceStore;
use riptide_core::torrent::PieceStore;
// Re-export main types for convenience
pub use simulated::{SimulatedConfig, SimulatedPeers, SimulationSpeed, SimulationStats};

/// Unified peer manager mode for easy configuration.
#[derive(Debug, Clone)]
pub enum PeersMode {
    /// Fast development mode with realistic streaming performance.
    Development,
    /// Simulated testing with ideal network conditions.
    SimulatedIdeal,
    /// Simulated testing with poor network conditions.
    SimulatedPoor,
    /// Custom simulated testing with specified configuration.
    SimulatedCustom(SimulatedConfig),
}

/// Factory for creating peer managers in different modes.
pub struct PeersBuilder;

impl PeersBuilder {
    /// Creates a development peer manager for fast iteration.
    pub fn development<P: PieceStore>(piece_store: Arc<P>) -> SimulatedPeers<P> {
        SimulatedPeers::new_instant(piece_store)
    }

    /// Creates a simulated peer manager with ideal network conditions.
    pub fn simulated_ideal<P: PieceStore>(piece_store: Arc<P>) -> SimulatedPeers<P> {
        SimulatedPeers::new_ideal(piece_store)
    }

    /// Creates a simulated peer manager with poor network conditions.
    pub fn simulated_poor<P: PieceStore>(piece_store: Arc<P>) -> SimulatedPeers<P> {
        SimulatedPeers::new_poor(piece_store)
    }

    /// Creates a simulated peer manager with custom configuration.
    pub fn simulated_custom<P: PieceStore>(
        config: SimulatedConfig,
        piece_store: Arc<P>,
    ) -> SimulatedPeers<P> {
        SimulatedPeers::new(config, piece_store)
    }

    /// Creates a peer manager based on the specified mode.
    pub fn create_development_mode<P>(
        mode: PeersMode,
        piece_store: Arc<P>,
    ) -> Box<dyn riptide_core::torrent::PeerManager + Send + Sync>
    where
        P: PieceStore + Send + Sync + 'static,
    {
        match mode {
            PeersMode::Development => Box::new(Self::development(piece_store)),
            PeersMode::SimulatedIdeal => Box::new(Self::simulated_ideal(piece_store)),
            PeersMode::SimulatedPoor => Box::new(Self::simulated_poor(piece_store)),
            PeersMode::SimulatedCustom(config) => {
                Box::new(Self::simulated_custom(config, piece_store))
            }
        }
    }
}

/// Convenience functions for common peer manager setups.
impl PeersMode {
    /// Creates a complete setup with in-memory piece store and development peers.
    pub fn setup_development() -> (Arc<InMemoryPieceStore>, SimulatedPeers<InMemoryPieceStore>) {
        let piece_store = Arc::new(InMemoryPieceStore::new());
        let peers = PeersBuilder::development(piece_store.clone());
        (piece_store, peers)
    }

    /// Creates a complete setup with in-memory piece store and ideal simulated peers.
    pub fn setup_simulated_ideal() -> (Arc<InMemoryPieceStore>, SimulatedPeers<InMemoryPieceStore>)
    {
        let piece_store = Arc::new(InMemoryPieceStore::new());
        let peers = PeersBuilder::simulated_ideal(piece_store.clone());
        (piece_store, peers)
    }

    /// Creates a complete setup with in-memory piece store and poor simulated peers.
    pub fn setup_simulated_poor() -> (Arc<InMemoryPieceStore>, SimulatedPeers<InMemoryPieceStore>) {
        let piece_store = Arc::new(InMemoryPieceStore::new());
        let peers = PeersBuilder::simulated_poor(piece_store.clone());
        (piece_store, peers)
    }

    /// Creates a complete setup with custom simulated configuration.
    pub fn setup_simulated_custom(
        config: SimulatedConfig,
    ) -> (Arc<InMemoryPieceStore>, SimulatedPeers<InMemoryPieceStore>) {
        let piece_store = Arc::new(InMemoryPieceStore::new());
        let peers = PeersBuilder::simulated_custom(config, piece_store.clone());
        (piece_store, peers)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_peers_factory() {
        let piece_store = Arc::new(InMemoryPieceStore::new());

        // Test development mode
        let _dev_peers = PeersBuilder::development(piece_store.clone());

        // Test simulated modes
        let _ideal_peers = PeersBuilder::simulated_ideal(piece_store.clone());
        let _poor_peers = PeersBuilder::simulated_poor(piece_store.clone());

        // Test custom configuration
        let config = SimulatedConfig::with_seed(42);
        let _custom_peers = PeersBuilder::simulated_custom(config, piece_store.clone());
    }

    #[test]
    fn test_convenience_setups() {
        // Test development setup
        let (_store, _peers) = PeersMode::setup_development();

        // Test simulated setups
        let (_store, _peers) = PeersMode::setup_simulated_ideal();
        let (_store, _peers) = PeersMode::setup_simulated_poor();

        // Test custom setup
        let config = SimulatedConfig::with_seed(123);
        let (_store, _peers) = PeersMode::setup_simulated_custom(config);
    }

    #[tokio::test]
    async fn test_mode_based_creation() {
        let piece_store = Arc::new(InMemoryPieceStore::new());

        // Test all modes
        let modes = vec![
            PeersMode::Development,
            PeersMode::SimulatedIdeal,
            PeersMode::SimulatedPoor,
            PeersMode::SimulatedCustom(SimulatedConfig::default()),
        ];

        for mode in modes {
            let _peers = PeersBuilder::create_development_mode(mode, piece_store.clone());
        }
    }
}
