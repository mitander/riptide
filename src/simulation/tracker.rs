//! Mock BitTorrent tracker for simulation

use std::net::SocketAddr;
use std::time::Duration;

/// Mock tracker for offline development
pub struct MockTracker {
    seeders: u32,
    leechers: u32,
    failure_rate: f32,
    peers: Vec<SocketAddr>,
}

impl MockTracker {
    pub fn new() -> Self {
        Self {
            seeders: 10,
            leechers: 5,
            failure_rate: 0.0,
            peers: Vec::new(),
        }
    }
    
    pub fn builder() -> MockTrackerBuilder {
        MockTrackerBuilder::new()
    }
    
    /// Simulate tracker announce request
    pub async fn announce(&self, _info_hash: &[u8; 20]) -> Result<AnnounceResponse, TrackerError> {
        // Simulate network delay
        tokio::time::sleep(Duration::from_millis(50)).await;
        
        // Simulate failure if configured
        if rand::random::<f32>() < self.failure_rate {
            return Err(TrackerError::ConnectionFailed);
        }
        
        Ok(AnnounceResponse {
            interval: 1800,
            seeders: self.seeders,
            leechers: self.leechers,
            peers: self.peers.clone(),
        })
    }
}

pub struct MockTrackerBuilder {
    seeders: u32,
    leechers: u32,
    failure_rate: f32,
}

impl MockTrackerBuilder {
    fn new() -> Self {
        Self {
            seeders: 10,
            leechers: 5,
            failure_rate: 0.0,
        }
    }
    
    pub fn with_seeders(mut self, count: u32) -> Self {
        self.seeders = count;
        self
    }
    
    pub fn with_leechers(mut self, count: u32) -> Self {
        self.leechers = count;
        self
    }
    
    pub fn with_failure_rate(mut self, rate: f32) -> Self {
        self.failure_rate = rate;
        self
    }
    
    pub fn build(self) -> MockTracker {
        let mut peers = Vec::new();
        
        // Generate realistic peer addresses
        for i in 0..(self.seeders + self.leechers) {
            let ip = format!("192.168.1.{}", 100 + (i % 50));
            let port = 6881 + (i % 100) as u16;
            if let Ok(addr) = format!("{}:{}", ip, port).parse() {
                peers.push(addr);
            }
        }
        
        MockTracker {
            seeders: self.seeders,
            leechers: self.leechers,
            failure_rate: self.failure_rate,
            peers,
        }
    }
}

#[derive(Debug)]
pub struct AnnounceResponse {
    pub interval: u32,
    pub seeders: u32,
    pub leechers: u32,
    pub peers: Vec<SocketAddr>,
}

#[derive(Debug, thiserror::Error)]
pub enum TrackerError {
    #[error("Connection to tracker failed")]
    ConnectionFailed,
    
    #[error("Invalid response from tracker")]
    InvalidResponse,
}