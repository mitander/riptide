//! Centralized configuration for Riptide.
//!
//! All tunable parameters and settings are defined here to avoid
//! hard-coded values scattered throughout the codebase.

use std::time::Duration;

/// Central configuration for all Riptide components.
///
/// Groups related configuration settings into logical sections.
/// Supports environment variable overrides for runtime customization.
#[derive(Debug, Clone, Default)]
pub struct RiptideConfig {
    pub torrent: TorrentConfig,
    pub network: NetworkConfig,
    pub storage: StorageConfig,
    pub simulation: SimulationConfig,
}

/// BitTorrent protocol-specific configuration.
///
/// Controls torrent downloading behavior, timeouts, and protocol parameters.
#[derive(Debug, Clone)]
pub struct TorrentConfig {
    /// BitTorrent client identifier
    pub client_id: &'static str,
    /// Piece request timeout
    pub piece_timeout: Duration,
    /// Default piece size for new torrents
    pub default_piece_size: u32,
}

impl Default for TorrentConfig {
    fn default() -> Self {
        Self {
            client_id: "-RT0001-",
            piece_timeout: Duration::from_secs(30),
            default_piece_size: 32768, // 32 KiB
        }
    }
}

/// Network communication and tracker configuration.
///
/// Controls HTTP timeouts, peer connection limits, bandwidth throttling,
/// and tracker communication parameters.
#[derive(Debug, Clone)]
pub struct NetworkConfig {
    /// HTTP request timeout for tracker communication
    pub tracker_timeout: Duration,
    /// Minimum announce interval
    pub min_announce_interval: Duration,
    /// Default announce interval
    pub default_announce_interval: Duration,
    /// User agent for HTTP requests
    pub user_agent: &'static str,
    /// Maximum concurrent peer connections
    pub max_peer_connections: usize,
    /// Download bandwidth limit in bytes per second (None = unlimited)
    pub download_limit: Option<u64>,
    /// Upload bandwidth limit in bytes per second (None = unlimited)
    pub upload_limit: Option<u64>,
    /// Peer connection timeout
    pub peer_timeout: Duration,
}

impl Default for NetworkConfig {
    fn default() -> Self {
        Self {
            tracker_timeout: Duration::from_secs(30),
            min_announce_interval: Duration::from_secs(300), // 5 minutes
            default_announce_interval: Duration::from_secs(1800), // 30 minutes
            user_agent: "riptide/0.1.0",
            max_peer_connections: 50,
            download_limit: None,                   // Unlimited by default
            upload_limit: None,                     // Unlimited by default
            peer_timeout: Duration::from_secs(300), // 5 minutes
        }
    }
}

/// File storage and disk I/O configuration.
///
/// Controls buffer sizes, temporary file handling, and storage optimization
/// settings for piece data persistence.
#[derive(Debug, Clone)]
pub struct StorageConfig {
    /// Buffer size for file operations
    pub file_buffer_size: usize,
    /// Temporary file suffix
    pub temp_file_suffix: &'static str,
    /// Whether to use memory-mapped files
    pub use_mmap: bool,
}

impl Default for StorageConfig {
    fn default() -> Self {
        Self {
            file_buffer_size: 65536, // 64 KiB
            temp_file_suffix: ".tmp",
            use_mmap: false, // Start with simple file I/O
        }
    }
}

/// Simulation mode configuration for testing and development.
///
/// Controls whether components use simulated or real implementations,
/// and configures simulation parameters for deterministic testing.
#[derive(Debug, Clone)]
pub struct SimulationConfig {
    /// Enable simulation mode for all components
    pub enabled: bool,
    /// Deterministic seed for reproducible simulations
    pub deterministic_seed: Option<u64>,
    /// Simulated network latency in milliseconds
    pub network_latency_ms: u64,
    /// Simulated packet loss rate (0.0 to 1.0)
    pub packet_loss_rate: f64,
    /// Maximum simulated peers per torrent
    pub max_simulated_peers: usize,
    /// Simulated download speed in bytes per second
    pub simulated_download_speed: u64,
    /// Whether to use mock data instead of real external services
    pub use_mock_data: bool,
}

impl Default for SimulationConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            deterministic_seed: None,
            network_latency_ms: 50,
            packet_loss_rate: 0.01, // 1% packet loss
            max_simulated_peers: 20,
            simulated_download_speed: 1_048_576, // 1 MB/s
            use_mock_data: false,
        }
    }
}

impl SimulationConfig {
    /// Creates a configuration for deterministic testing.
    pub fn deterministic_testing() -> Self {
        Self {
            enabled: true,
            deterministic_seed: Some(42), // Fixed seed for reproducible tests
            network_latency_ms: 0,        // No latency for fast tests
            packet_loss_rate: 0.0,        // No packet loss for reliable tests
            max_simulated_peers: 10,      // Smaller number for faster tests
            simulated_download_speed: 10_485_760, // 10 MB/s for fast downloads
            use_mock_data: true,
        }
    }

    /// Creates a configuration for realistic simulation.
    pub fn realistic_simulation() -> Self {
        Self {
            enabled: true,
            deterministic_seed: None, // Random for realistic behavior
            network_latency_ms: 100,  // Realistic internet latency
            packet_loss_rate: 0.02,   // 2% packet loss
            max_simulated_peers: 50,  // Realistic peer count
            simulated_download_speed: 2_097_152, // 2 MB/s realistic speed
            use_mock_data: false,
        }
    }
}

impl RiptideConfig {
    /// Creates configuration with environment variable overrides.
    ///
    /// Allows runtime configuration via environment variables while
    /// maintaining sensible defaults.
    pub fn from_env() -> Self {
        let mut config = Self::default();

        // Network configuration overrides
        if let Ok(timeout) = std::env::var("RIPTIDE_TRACKER_TIMEOUT") {
            if let Ok(seconds) = timeout.parse::<u64>() {
                config.network.tracker_timeout = Duration::from_secs(seconds);
            }
        }

        if let Ok(max_peers) = std::env::var("RIPTIDE_MAX_PEERS") {
            if let Ok(count) = max_peers.parse::<usize>() {
                config.network.max_peer_connections = count;
            }
        }

        // Simulation configuration overrides
        if let Ok(enabled) = std::env::var("RIPTIDE_SIMULATION_MODE") {
            config.simulation.enabled = enabled.parse().unwrap_or(false);
        }

        if let Ok(seed) = std::env::var("RIPTIDE_SIMULATION_SEED") {
            if let Ok(seed_value) = seed.parse::<u64>() {
                config.simulation.deterministic_seed = Some(seed_value);
            }
        }

        if let Ok(mock_data) = std::env::var("RIPTIDE_USE_MOCK_DATA") {
            config.simulation.use_mock_data = mock_data.parse().unwrap_or(false);
        }

        config
    }

    /// Creates a configuration optimized for testing.
    pub fn for_testing() -> Self {
        Self {
            simulation: SimulationConfig::deterministic_testing(),
            ..Default::default()
        }
    }

    /// Creates a configuration for development with realistic simulation.
    pub fn for_development() -> Self {
        Self {
            simulation: SimulationConfig::realistic_simulation(),
            ..Default::default()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config_values() {
        let config = RiptideConfig::default();

        assert_eq!(config.torrent.client_id, "-RT0001-");
        assert_eq!(config.network.max_peer_connections, 50);
        assert_eq!(config.network.tracker_timeout, Duration::from_secs(30));
        assert_eq!(config.storage.file_buffer_size, 65536);
        assert_eq!(config.network.peer_timeout, Duration::from_secs(300));
        assert!(!config.simulation.enabled);
        assert!(!config.simulation.use_mock_data);
    }

    #[test]
    fn test_simulation_config_presets() {
        let testing_config = SimulationConfig::deterministic_testing();
        assert!(testing_config.enabled);
        assert!(testing_config.use_mock_data);
        assert_eq!(testing_config.deterministic_seed, Some(42));
        assert_eq!(testing_config.network_latency_ms, 0);
        assert_eq!(testing_config.packet_loss_rate, 0.0);

        let realistic_config = SimulationConfig::realistic_simulation();
        assert!(realistic_config.enabled);
        assert!(!realistic_config.use_mock_data);
        assert_eq!(realistic_config.deterministic_seed, None);
        assert!(realistic_config.network_latency_ms > 0);
        assert!(realistic_config.packet_loss_rate > 0.0);
    }

    #[test]
    fn test_config_presets() {
        let testing_config = RiptideConfig::for_testing();
        assert!(testing_config.simulation.enabled);
        assert!(testing_config.simulation.use_mock_data);

        let dev_config = RiptideConfig::for_development();
        assert!(dev_config.simulation.enabled);
        assert!(!dev_config.simulation.use_mock_data);
    }

    #[test]
    fn test_env_override() {
        unsafe {
            std::env::set_var("RIPTIDE_TRACKER_TIMEOUT", "60");
            std::env::set_var("RIPTIDE_MAX_PEERS", "100");
            std::env::set_var("RIPTIDE_SIMULATION_MODE", "true");
            std::env::set_var("RIPTIDE_SIMULATION_SEED", "12345");
            std::env::set_var("RIPTIDE_USE_MOCK_DATA", "true");
        }

        let config = RiptideConfig::from_env();

        assert_eq!(config.network.tracker_timeout, Duration::from_secs(60));
        assert_eq!(config.network.max_peer_connections, 100);
        assert!(config.simulation.enabled);
        assert_eq!(config.simulation.deterministic_seed, Some(12345));
        assert!(config.simulation.use_mock_data);

        // Cleanup
        unsafe {
            std::env::remove_var("RIPTIDE_TRACKER_TIMEOUT");
            std::env::remove_var("RIPTIDE_MAX_PEERS");
            std::env::remove_var("RIPTIDE_SIMULATION_MODE");
            std::env::remove_var("RIPTIDE_SIMULATION_SEED");
            std::env::remove_var("RIPTIDE_USE_MOCK_DATA");
        }
    }
}
