//! Network condition simulation

use std::ops::Range;
use std::time::Duration;

/// Simulates various network conditions for testing
pub struct NetworkSimulator {
    latency: Range<u64>,
    packet_loss: f32,
    bandwidth_limit: u64, // bytes per second
}

impl Default for NetworkSimulator {
    fn default() -> Self {
        Self::new()
    }
}

impl NetworkSimulator {
    pub fn new() -> Self {
        Self {
            latency: 10..50, // 10-50ms
            packet_loss: 0.0,
            bandwidth_limit: u64::MAX,
        }
    }

    pub fn builder() -> NetworkSimulatorBuilder {
        NetworkSimulatorBuilder::new()
    }

    /// Simulate network latency
    pub async fn simulate_latency(&self) {
        let delay_ms =
            rand::random::<u64>() % (self.latency.end - self.latency.start) + self.latency.start;
        tokio::time::sleep(Duration::from_millis(delay_ms)).await;
    }

    /// Check if packet should be dropped
    pub fn should_drop_packet(&self) -> bool {
        rand::random::<f32>() < self.packet_loss
    }

    /// Calculate bandwidth delay for data transfer
    pub fn bandwidth_delay(&self, bytes: usize) -> Duration {
        if self.bandwidth_limit == u64::MAX {
            Duration::ZERO
        } else {
            let seconds = bytes as f64 / self.bandwidth_limit as f64;
            Duration::from_secs_f64(seconds)
        }
    }
}

pub struct NetworkSimulatorBuilder {
    latency: Range<u64>,
    packet_loss: f32,
    bandwidth_limit: u64,
}

impl NetworkSimulatorBuilder {
    fn new() -> Self {
        Self {
            latency: 10..50,
            packet_loss: 0.0,
            bandwidth_limit: u64::MAX,
        }
    }

    pub fn latency(mut self, range: Range<u64>) -> Self {
        self.latency = range;
        self
    }

    pub fn packet_loss(mut self, rate: f32) -> Self {
        self.packet_loss = rate;
        self
    }

    pub fn bandwidth_limit(mut self, bytes_per_second: u64) -> Self {
        self.bandwidth_limit = bytes_per_second;
        self
    }

    pub fn build(self) -> NetworkSimulator {
        NetworkSimulator {
            latency: self.latency,
            packet_loss: self.packet_loss,
            bandwidth_limit: self.bandwidth_limit,
        }
    }
}
