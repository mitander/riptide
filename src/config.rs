//! Centralized configuration for Riptide.
//!
//! All tunable parameters and settings are defined here to avoid
//! hard-coded values scattered throughout the codebase.

use std::time::Duration;

/// Central configuration for Riptide components.
#[derive(Debug, Clone, Default)]
pub struct RiptideConfig {
    pub torrent: TorrentConfig,
    pub network: NetworkConfig,
    pub storage: StorageConfig,
}

/// BitTorrent protocol configuration.
#[derive(Debug, Clone)]
pub struct TorrentConfig {
    /// BitTorrent client identifier
    pub client_id: &'static str,
    /// Maximum number of peer connections
    pub max_peer_connections: usize,
    /// Piece request timeout
    pub piece_timeout: Duration,
    /// Default piece size for new torrents
    pub default_piece_size: u32,
}

impl Default for TorrentConfig {
    fn default() -> Self {
        Self {
            client_id: "-RT0001-",
            max_peer_connections: 50,
            piece_timeout: Duration::from_secs(30),
            default_piece_size: 32768, // 32 KiB
        }
    }
}

/// Network and tracker configuration.
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
}

impl Default for NetworkConfig {
    fn default() -> Self {
        Self {
            tracker_timeout: Duration::from_secs(30),
            min_announce_interval: Duration::from_secs(300), // 5 minutes
            default_announce_interval: Duration::from_secs(1800), // 30 minutes
            user_agent: "riptide/0.1.0",
        }
    }
}

/// Storage and file management configuration.
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

impl RiptideConfig {
    /// Creates configuration with environment variable overrides.
    ///
    /// Allows runtime configuration via environment variables while
    /// maintaining sensible defaults.
    pub fn from_env() -> Self {
        let mut config = Self::default();

        // Override with environment variables if present
        if let Ok(timeout) = std::env::var("RIPTIDE_TRACKER_TIMEOUT") {
            if let Ok(seconds) = timeout.parse::<u64>() {
                config.network.tracker_timeout = Duration::from_secs(seconds);
            }
        }

        if let Ok(max_peers) = std::env::var("RIPTIDE_MAX_PEERS") {
            if let Ok(count) = max_peers.parse::<usize>() {
                config.torrent.max_peer_connections = count;
            }
        }

        config
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config_values() {
        let config = RiptideConfig::default();

        assert_eq!(config.torrent.client_id, "-RT0001-");
        assert_eq!(config.torrent.max_peer_connections, 50);
        assert_eq!(config.network.tracker_timeout, Duration::from_secs(30));
        assert_eq!(config.storage.file_buffer_size, 65536);
    }

    #[test]
    fn test_env_override() {
        unsafe {
            std::env::set_var("RIPTIDE_TRACKER_TIMEOUT", "60");
            std::env::set_var("RIPTIDE_MAX_PEERS", "100");
        }

        let config = RiptideConfig::from_env();

        assert_eq!(config.network.tracker_timeout, Duration::from_secs(60));
        assert_eq!(config.torrent.max_peer_connections, 100);

        // Cleanup
        unsafe {
            std::env::remove_var("RIPTIDE_TRACKER_TIMEOUT");
            std::env::remove_var("RIPTIDE_MAX_PEERS");
        }
    }
}
