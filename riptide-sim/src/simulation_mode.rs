//! Simulation mode configuration for different testing scenarios
//!
//! Provides flexible simulation backends for different use cases:
//! - Deterministic: For bug reproduction and long-running tests
//! - Development: For fast iteration and streaming development
//! - Hybrid: Combines both approaches for comprehensive testing

use std::sync::Arc;

use riptide_core::torrent::PieceStore;

use crate::dev_peer_manager::DevPeerManager;
use crate::{InMemoryPeerConfig, SimPeerManager};

/// Simulation mode configuration for different testing scenarios.
#[derive(Debug, Clone)]
pub enum SimulationMode {
    /// Deterministic simulation for bug reproduction and comprehensive testing.
    /// - Message-based peer communication
    /// - Realistic network delays and failures
    /// - Perfect reproducibility with seeds
    /// - Slower but thorough testing
    Deterministic {
        /// Random seed for reproducible results
        seed: u64,
        /// Network conditions to simulate
        network_conditions: NetworkConditions,
        /// Peer behavior configuration
        peer_config: InMemoryPeerConfig,
    },

    /// Fast development mode for streaming performance.
    /// - Direct piece store access
    /// - No artificial delays
    /// - Maximum throughput (>10 MB/s)
    /// - Minimal simulation overhead
    Development {
        /// Enable performance monitoring
        enable_metrics: bool,
        /// Enable basic peer tracking for compatibility
        enable_peer_tracking: bool,
    },

    /// Hybrid mode for comprehensive testing.
    /// - Uses fast mode for initial data loading
    /// - Switches to deterministic for specific test scenarios
    /// - Best of both worlds for complex test suites
    Hybrid {
        /// Seed for deterministic portions
        seed: u64,
        /// Threshold for switching modes (e.g., after loading X pieces)
        switch_threshold: usize,
    },
}

/// Network conditions for deterministic simulation.
#[derive(Debug, Clone)]
pub struct NetworkConditions {
    /// Base message delay in milliseconds
    pub base_delay_ms: u64,
    /// Probability of message loss (0.0 to 1.0)
    pub message_loss_rate: f64,
    /// Probability of connection failure (0.0 to 1.0)
    pub connection_failure_rate: f64,
    /// Bandwidth limit per peer (bytes per second)
    pub bandwidth_limit: Option<u64>,
    /// Jitter variance as percentage of base delay
    pub jitter_percent: f64,
}

impl Default for NetworkConditions {
    fn default() -> Self {
        Self {
            base_delay_ms: 50,                // 50ms realistic internet delay
            message_loss_rate: 0.01,          // 1% packet loss
            connection_failure_rate: 0.05,    // 5% connection failures
            bandwidth_limit: Some(5_000_000), // 5 MB/s per peer
            jitter_percent: 0.1,              // 10% jitter
        }
    }
}

impl NetworkConditions {
    /// Ideal network conditions for performance testing.
    pub fn ideal() -> Self {
        Self {
            base_delay_ms: 1, // Minimal delay
            message_loss_rate: 0.0,
            connection_failure_rate: 0.0,
            bandwidth_limit: None, // Unlimited bandwidth
            jitter_percent: 0.0,
        }
    }

    /// Poor network conditions for stress testing.
    pub fn poor() -> Self {
        Self {
            base_delay_ms: 200,               // High latency
            message_loss_rate: 0.05,          // 5% packet loss
            connection_failure_rate: 0.15,    // 15% connection failures
            bandwidth_limit: Some(1_000_000), // 1 MB/s limit
            jitter_percent: 0.3,              // 30% jitter
        }
    }

    /// Mobile network conditions.
    pub fn mobile() -> Self {
        Self {
            base_delay_ms: 100,
            message_loss_rate: 0.02,
            connection_failure_rate: 0.1,
            bandwidth_limit: Some(2_000_000), // 2 MB/s typical mobile
            jitter_percent: 0.2,
        }
    }
}

impl Default for SimulationMode {
    fn default() -> Self {
        // Default to development mode for fast iteration
        SimulationMode::Development {
            enable_metrics: true,
            enable_peer_tracking: true,
        }
    }
}

impl SimulationMode {
    /// Creates deterministic simulation mode with default settings.
    pub fn deterministic(seed: u64) -> Self {
        SimulationMode::Deterministic {
            seed,
            network_conditions: NetworkConditions::default(),
            peer_config: InMemoryPeerConfig::default(),
        }
    }

    /// Creates deterministic mode with custom network conditions.
    pub fn deterministic_with_conditions(seed: u64, conditions: NetworkConditions) -> Self {
        SimulationMode::Deterministic {
            seed,
            network_conditions: conditions,
            peer_config: InMemoryPeerConfig::default(),
        }
    }

    /// Creates development mode with performance focus.
    pub fn development() -> Self {
        SimulationMode::Development {
            enable_metrics: true,
            enable_peer_tracking: false, // Minimal overhead
        }
    }

    /// Creates development mode optimized for benchmarking.
    pub fn benchmark() -> Self {
        SimulationMode::Development {
            enable_metrics: true,        // Need metrics for benchmarks
            enable_peer_tracking: false, // Minimal overhead
        }
    }

    /// Creates hybrid mode for comprehensive testing.
    pub fn hybrid(seed: u64, switch_threshold: usize) -> Self {
        SimulationMode::Hybrid {
            seed,
            switch_threshold,
        }
    }

    /// Returns whether this mode should use fast peer manager.
    pub fn uses_fast_manager(&self) -> bool {
        matches!(self, SimulationMode::Development { .. })
    }

    /// Returns whether this mode provides deterministic results.
    pub fn is_deterministic(&self) -> bool {
        matches!(
            self,
            SimulationMode::Deterministic { .. } | SimulationMode::Hybrid { .. }
        )
    }

    /// Returns expected performance characteristics.
    pub fn expected_throughput_mbps(&self) -> f64 {
        match self {
            SimulationMode::Deterministic {
                network_conditions, ..
            } => {
                if let Some(bandwidth) = network_conditions.bandwidth_limit {
                    (bandwidth * 8) as f64 / 1_000_000.0 // Convert to Mbps
                } else {
                    100.0 // Unlimited, but realistic expectation
                }
            }
            SimulationMode::Development { .. } => 500.0, // Very high performance
            SimulationMode::Hybrid { .. } => 50.0,       // Mixed performance
        }
    }
}

/// Factory for creating peer managers based on simulation mode.
pub struct SimulationPeerManagerFactory;

impl SimulationPeerManagerFactory {
    /// Creates appropriate peer manager for the given simulation mode.
    pub fn create_peer_manager<P: PieceStore + 'static>(
        mode: &SimulationMode,
        piece_store: Arc<P>,
    ) -> Box<dyn riptide_core::torrent::PeerManager> {
        match mode {
            SimulationMode::Deterministic { peer_config, .. } => {
                tracing::info!("Creating SimPeerManager for deterministic simulation");
                Box::new(SimPeerManager::new(peer_config.clone(), piece_store))
            }
            SimulationMode::Development { .. } => {
                tracing::info!("Creating DevPeerManager for realistic streaming speed");
                Box::new(DevPeerManager::new(piece_store))
            }
            SimulationMode::Hybrid { .. } => {
                // For now, start with fast mode. In future, implement actual hybrid logic.
                tracing::info!("Creating DevPeerManager for hybrid mode (fast phase)");
                Box::new(DevPeerManager::new(piece_store))
            }
        }
    }

    /// Creates peer manager with automatic mode selection based on environment.
    pub fn create_auto<P: PieceStore + 'static>(
        piece_store: Arc<P>,
    ) -> Box<dyn riptide_core::torrent::PeerManager> {
        // Choose mode based on environment
        let mode = if std::env::var("RIPTIDE_MODE").as_deref() == Ok("deterministic") {
            SimulationMode::deterministic(0x12345)
        } else if std::env::var("RIPTIDE_MODE").as_deref() == Ok("benchmark") {
            SimulationMode::benchmark()
        } else {
            SimulationMode::development() // Default for development
        };

        Self::create_peer_manager(&mode, piece_store)
    }
}

/// Configuration builder for simulation scenarios.
pub struct SimulationConfigBuilder {
    mode: SimulationMode,
}

impl SimulationConfigBuilder {
    /// Start building deterministic simulation configuration.
    pub fn deterministic(seed: u64) -> Self {
        Self {
            mode: SimulationMode::deterministic(seed),
        }
    }

    /// Start building development configuration.
    pub fn development() -> Self {
        Self {
            mode: SimulationMode::development(),
        }
    }

    /// Configure network conditions for deterministic mode.
    pub fn with_network_conditions(mut self, conditions: NetworkConditions) -> Self {
        if let SimulationMode::Deterministic {
            network_conditions, ..
        } = &mut self.mode
        {
            *network_conditions = conditions;
        }
        self
    }

    /// Configure peer behavior for deterministic mode.
    pub fn with_peer_config(mut self, config: InMemoryPeerConfig) -> Self {
        if let SimulationMode::Deterministic { peer_config, .. } = &mut self.mode {
            *peer_config = config;
        }
        self
    }

    /// Enable/disable metrics for development mode.
    pub fn with_metrics(mut self, enable: bool) -> Self {
        if let SimulationMode::Development { enable_metrics, .. } = &mut self.mode {
            *enable_metrics = enable;
        }
        self
    }

    /// Build the final simulation mode.
    pub fn build(self) -> SimulationMode {
        self.mode
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_simulation_mode_characteristics() {
        let dev_mode = SimulationMode::development();
        assert!(dev_mode.uses_fast_manager());
        assert!(!dev_mode.is_deterministic());
        assert!(dev_mode.expected_throughput_mbps() > 100.0);

        let det_mode = SimulationMode::deterministic(12345);
        assert!(!det_mode.uses_fast_manager());
        assert!(det_mode.is_deterministic());

        let hybrid_mode = SimulationMode::hybrid(12345, 100);
        assert!(hybrid_mode.is_deterministic());
    }

    #[test]
    fn test_network_conditions() {
        let ideal = NetworkConditions::ideal();
        assert_eq!(ideal.message_loss_rate, 0.0);
        assert_eq!(ideal.connection_failure_rate, 0.0);
        assert!(ideal.bandwidth_limit.is_none());

        let poor = NetworkConditions::poor();
        assert!(poor.message_loss_rate > 0.0);
        assert!(poor.connection_failure_rate > 0.0);
        assert!(poor.bandwidth_limit.is_some());
    }

    #[test]
    fn test_config_builder() {
        let mode = SimulationConfigBuilder::deterministic(0xABCD)
            .with_network_conditions(NetworkConditions::ideal())
            .build();

        if let SimulationMode::Deterministic {
            seed,
            network_conditions,
            ..
        } = mode
        {
            assert_eq!(seed, 0xABCD);
            assert_eq!(network_conditions.message_loss_rate, 0.0);
        } else {
            panic!("Expected deterministic mode");
        }
    }
}
