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
//! use riptide_sim::peers::{DevelopmentPeers, InMemoryPieceStore};
//! use std::sync::Arc;
//!
//! let piece_store = Arc::new(InMemoryPieceStore::new());
//! let peers = DevelopmentPeers::new(piece_store);
//! ```
//!
//! ## Deterministic Simulation
//! ```rust,no_run
//! use riptide_sim::peers::{DeterministicPeers, DeterministicConfig, InMemoryPieceStore};
//! use std::sync::Arc;
//!
//! let piece_store = Arc::new(InMemoryPieceStore::new());
//! let config = DeterministicConfig::ideal();
//! let peers = DeterministicPeers::new(config, piece_store);
//! ```

pub mod deterministic;
pub mod development;
pub mod piece_store;

use std::sync::Arc;

// Re-export main types for convenience
pub use deterministic::{DeterministicConfig, DeterministicPeers, SimulationStats};
pub use development::{DevelopmentPeers, DevelopmentStats};
pub use piece_store::InMemoryPieceStore;
use riptide_core::torrent::PieceStore;

/// Unified peer manager mode for easy configuration.
#[derive(Debug, Clone)]
pub enum PeersMode {
    /// Fast development mode with realistic streaming performance.
    Development,
    /// Deterministic simulation with ideal network conditions.
    DeterministicIdeal,
    /// Deterministic simulation with poor network conditions.
    DeterministicPoor,
    /// Custom deterministic simulation with specified configuration.
    DeterministicCustom(DeterministicConfig),
}

/// Factory for creating peer managers in different modes.
pub struct PeersBuilder;

impl PeersBuilder {
    /// Creates a development peer manager for fast iteration.
    pub fn development<P: PieceStore>(piece_store: Arc<P>) -> DevelopmentPeers<P> {
        DevelopmentPeers::new(piece_store)
    }

    /// Creates a deterministic peer manager with ideal network conditions.
    pub fn deterministic_ideal<P: PieceStore>(piece_store: Arc<P>) -> DeterministicPeers<P> {
        DeterministicPeers::new_ideal(piece_store)
    }

    /// Creates a deterministic peer manager with poor network conditions.
    pub fn deterministic_poor<P: PieceStore>(piece_store: Arc<P>) -> DeterministicPeers<P> {
        DeterministicPeers::new_poor(piece_store)
    }

    /// Creates a deterministic peer manager with custom configuration.
    pub fn deterministic_custom<P: PieceStore>(
        config: DeterministicConfig,
        piece_store: Arc<P>,
    ) -> DeterministicPeers<P> {
        DeterministicPeers::new(config, piece_store)
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
            PeersMode::DeterministicIdeal => Box::new(Self::deterministic_ideal(piece_store)),
            PeersMode::DeterministicPoor => Box::new(Self::deterministic_poor(piece_store)),
            PeersMode::DeterministicCustom(config) => {
                Box::new(Self::deterministic_custom(config, piece_store))
            }
        }
    }
}

/// Convenience functions for common peer manager setups.
impl PeersMode {
    /// Creates a complete setup with in-memory piece store and development peers.
    pub fn setup_development() -> (
        Arc<InMemoryPieceStore>,
        DevelopmentPeers<InMemoryPieceStore>,
    ) {
        let piece_store = Arc::new(InMemoryPieceStore::new());
        let peers = PeersBuilder::development(piece_store.clone());
        (piece_store, peers)
    }

    /// Creates a complete setup with in-memory piece store and ideal deterministic peers.
    pub fn setup_deterministic_ideal() -> (
        Arc<InMemoryPieceStore>,
        DeterministicPeers<InMemoryPieceStore>,
    ) {
        let piece_store = Arc::new(InMemoryPieceStore::new());
        let peers = PeersBuilder::deterministic_ideal(piece_store.clone());
        (piece_store, peers)
    }

    /// Creates a complete setup with in-memory piece store and poor deterministic peers.
    pub fn setup_deterministic_poor() -> (
        Arc<InMemoryPieceStore>,
        DeterministicPeers<InMemoryPieceStore>,
    ) {
        let piece_store = Arc::new(InMemoryPieceStore::new());
        let peers = PeersBuilder::deterministic_poor(piece_store.clone());
        (piece_store, peers)
    }

    /// Creates a complete setup with custom deterministic configuration.
    pub fn setup_deterministic_custom(
        config: DeterministicConfig,
    ) -> (
        Arc<InMemoryPieceStore>,
        DeterministicPeers<InMemoryPieceStore>,
    ) {
        let piece_store = Arc::new(InMemoryPieceStore::new());
        let peers = PeersBuilder::deterministic_custom(config, piece_store.clone());
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

        // Test deterministic modes
        let _ideal_peers = PeersBuilder::deterministic_ideal(piece_store.clone());
        let _poor_peers = PeersBuilder::deterministic_poor(piece_store.clone());

        // Test custom configuration
        let config = DeterministicConfig::with_seed(42);
        let _custom_peers = PeersBuilder::deterministic_custom(config, piece_store.clone());
    }

    #[test]
    fn test_convenience_setups() {
        // Test development setup
        let (_store, _peers) = PeersMode::setup_development();

        // Test deterministic setups
        let (_store, _peers) = PeersMode::setup_deterministic_ideal();
        let (_store, _peers) = PeersMode::setup_deterministic_poor();

        // Test custom setup
        let config = DeterministicConfig::with_seed(123);
        let (_store, _peers) = PeersMode::setup_deterministic_custom(config);
    }

    #[tokio::test]
    async fn test_mode_based_creation() {
        let piece_store = Arc::new(InMemoryPieceStore::new());

        // Test all modes
        let modes = vec![
            PeersMode::Development,
            PeersMode::DeterministicIdeal,
            PeersMode::DeterministicPoor,
            PeersMode::DeterministicCustom(DeterministicConfig::default()),
        ];

        for mode in modes {
            let _peers = PeersBuilder::create_development_mode(mode, piece_store.clone());
        }
    }
}
