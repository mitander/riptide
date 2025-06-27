//! Scenario configuration types and builders.

use std::time::Duration;

/// Standard streaming scenario configuration.
#[derive(Debug, Clone)]
pub struct StreamingScenario {
    pub total_pieces: u32,
    pub piece_size: u32,
    pub target_bitrate: u64, // bits per second
    pub buffer_size: u32,    // pieces to buffer ahead
    pub network_latency: Duration,
    pub peer_count: usize,
}

impl Default for StreamingScenario {
    fn default() -> Self {
        Self {
            total_pieces: 1000,
            piece_size: 16384,         // 16 KiB pieces
            target_bitrate: 5_000_000, // 5 Mbps
            buffer_size: 10,           // Buffer 10 pieces ahead
            network_latency: Duration::from_millis(50),
            peer_count: 8,
        }
    }
}

/// Network stress scenario for testing resilience.
#[derive(Debug, Clone)]
pub struct StressScenario {
    pub packet_loss_rate: f64,    // 0.0 to 1.0
    pub bandwidth_limit: u64,     // bytes per second
    pub connection_failures: u32, // number of failures to inject
}

impl Default for StressScenario {
    fn default() -> Self {
        Self {
            packet_loss_rate: 0.05,   // 5% packet loss
            bandwidth_limit: 100_000, // 100 KB/s
            connection_failures: 3,
        }
    }
}
