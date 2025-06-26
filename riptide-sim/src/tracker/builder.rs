//! Builder pattern for MockTracker configuration

use std::collections::HashMap;

use rand::SeedableRng;
use rand_chacha::ChaCha8Rng;

use super::mock::MockTracker;
use crate::magneto_provider::{MockMagnetoProviderBuilder, create_mock_magneto_client};

/// Builder for configuring mock tracker behavior.
pub struct MockTrackerBuilder {
    seeders: u32,
    leechers: u32,
    failure_rate: f32,
    enable_magneto: bool,
    seed: u64,
}

impl MockTrackerBuilder {
    pub(crate) fn new() -> Self {
        Self {
            seeders: 10,
            leechers: 5,
            failure_rate: 0.0,
            enable_magneto: false,
            seed: 42,
        }
    }

    /// Sets number of seeders to report in responses.
    pub fn with_seeders(mut self, count: u32) -> Self {
        self.seeders = count;
        self
    }

    /// Sets number of leechers to report in responses.
    pub fn with_leechers(mut self, count: u32) -> Self {
        self.leechers = count;
        self
    }

    /// Sets probability (0.0-1.0) of simulated connection failures.
    pub fn with_failure_rate(mut self, rate: f32) -> Self {
        self.failure_rate = rate;
        self
    }

    /// Enables magneto integration for magnet link discovery.
    pub fn with_magneto(mut self, enable: bool) -> Self {
        self.enable_magneto = enable;
        self
    }

    /// Sets random seed for deterministic behavior.
    pub fn with_seed(mut self, seed: u64) -> Self {
        self.seed = seed;
        self
    }

    /// Creates mock tracker with configured settings.
    pub fn build(self) -> MockTracker {
        let rng = ChaCha8Rng::seed_from_u64(self.seed);
        let mut peers = Vec::new();

        // Generate realistic peer addresses
        for i in 0..(self.seeders + self.leechers) {
            let ip = format!("192.168.1.{}", 100 + (i % 50));
            let port = 6881 + (i % 100) as u16;
            if let Ok(addr) = format!("{ip}:{port}").parse() {
                peers.push(addr);
            }
        }

        MockTracker {
            seeders: self.seeders,
            leechers: self.leechers,
            failure_rate: self.failure_rate,
            peers,
            torrents: HashMap::new(),
            rng,
            magneto_client: if self.enable_magneto {
                let provider = MockMagnetoProviderBuilder::new()
                    .with_seed(self.seed)
                    .build();
                Some(create_mock_magneto_client(provider))
            } else {
                None
            },
        }
    }
}
