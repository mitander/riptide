//! Network condition simulation

use std::ops::Range;
use std::time::Duration;

/// Simulates various network conditions for testing.
///
/// Provides controllable network latency, packet loss, and bandwidth limits
/// for testing BitTorrent behavior under different network conditions.
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

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_network_simulator_default() {
        let sim = NetworkSimulator::new();
        assert_eq!(sim.latency.start, 10);
        assert_eq!(sim.latency.end, 50);
        assert_eq!(sim.packet_loss, 0.0);
        assert_eq!(sim.bandwidth_limit, u64::MAX);
    }

    #[tokio::test]
    async fn test_network_simulator_builder() {
        let sim = NetworkSimulator::builder()
            .latency(100..200)
            .packet_loss(0.05)
            .bandwidth_limit(1_000_000)
            .build();

        assert_eq!(sim.latency.start, 100);
        assert_eq!(sim.latency.end, 200);
        assert_eq!(sim.packet_loss, 0.05);
        assert_eq!(sim.bandwidth_limit, 1_000_000);
    }

    #[tokio::test]
    async fn test_simulate_latency() {
        let sim = NetworkSimulator::builder()
            .latency(50..51) // Very narrow range for predictable testing
            .build();

        let start = std::time::Instant::now();
        sim.simulate_latency().await;
        let elapsed = start.elapsed();

        // Should be approximately 50ms, allowing for some variance
        assert!(elapsed >= std::time::Duration::from_millis(49));
        assert!(elapsed <= std::time::Duration::from_millis(60));
    }

    #[test]
    fn test_packet_drop_no_loss() {
        let sim = NetworkSimulator::builder()
            .packet_loss(0.0)
            .build();

        // With 0% packet loss, should never drop
        for _ in 0..100 {
            assert!(!sim.should_drop_packet());
        }
    }

    #[test]
    fn test_packet_drop_always_loss() {
        let sim = NetworkSimulator::builder()
            .packet_loss(1.0)
            .build();

        // With 100% packet loss, should always drop
        for _ in 0..100 {
            assert!(sim.should_drop_packet());
        }
    }

    #[test]
    fn test_bandwidth_delay_unlimited() {
        let sim = NetworkSimulator::new(); // Default has unlimited bandwidth

        let delay = sim.bandwidth_delay(1_000_000); // 1MB
        assert_eq!(delay, Duration::ZERO);
    }

    #[test]
    fn test_bandwidth_delay_limited() {
        let sim = NetworkSimulator::builder()
            .bandwidth_limit(1_000_000) // 1MB/s
            .build();

        let delay = sim.bandwidth_delay(500_000); // 500KB
        assert_eq!(delay, Duration::from_millis(500)); // Should take 0.5 seconds
    }

    #[test]
    fn test_bandwidth_delay_calculation() {
        let sim = NetworkSimulator::builder()
            .bandwidth_limit(2_000_000) // 2MB/s
            .build();

        let delay = sim.bandwidth_delay(1_000_000); // 1MB
        assert_eq!(delay, Duration::from_millis(500)); // Should take 0.5 seconds
    }
}

impl NetworkSimulator {
    /// Creates network simulator with default low-latency settings.
    pub fn new() -> Self {
        Self {
            latency: 10..50, // 10-50ms
            packet_loss: 0.0,
            bandwidth_limit: u64::MAX,
        }
    }

    /// Returns builder for customizing network conditions.
    pub fn builder() -> NetworkSimulatorBuilder {
        NetworkSimulatorBuilder::new()
    }

    /// Simulate network latency.
    pub async fn simulate_latency(&self) {
        let delay_ms =
            rand::random::<u64>() % (self.latency.end - self.latency.start) + self.latency.start;
        tokio::time::sleep(Duration::from_millis(delay_ms)).await;
    }

    /// Check if packet should be dropped.
    pub fn should_drop_packet(&self) -> bool {
        rand::random::<f32>() < self.packet_loss
    }

    /// Calculate bandwidth delay for data transfer.
    pub fn bandwidth_delay(&self, bytes: usize) -> Duration {
        if self.bandwidth_limit == u64::MAX {
            Duration::ZERO
        } else {
            let seconds = bytes as f64 / self.bandwidth_limit as f64;
            Duration::from_secs_f64(seconds)
        }
    }
}

/// Builder for configuring network simulation parameters.
///
/// Allows fine-tuning of latency ranges, packet loss rates, and bandwidth
/// limits before creating the network simulator.
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

    /// Sets latency range in milliseconds.
    pub fn latency(mut self, range: Range<u64>) -> Self {
        self.latency = range;
        self
    }

    /// Sets packet loss rate as probability (0.0-1.0).
    pub fn packet_loss(mut self, rate: f32) -> Self {
        self.packet_loss = rate;
        self
    }

    /// Sets bandwidth limit in bytes per second.
    pub fn bandwidth_limit(mut self, bytes_per_second: u64) -> Self {
        self.bandwidth_limit = bytes_per_second;
        self
    }

    /// Creates network simulator with configured settings.
    pub fn build(self) -> NetworkSimulator {
        NetworkSimulator {
            latency: self.latency,
            packet_loss: self.packet_loss,
            bandwidth_limit: self.bandwidth_limit,
        }
    }
}
