//! Mock BitTorrent peer for simulation

use std::time::Duration;

/// Mock peer for BitTorrent protocol simulation
pub struct MockPeer {
    peer_id: String,
    upload_speed: u64, // bytes per second
    reliability: f32,  // 0.0 to 1.0
    latency: Duration,
}

impl MockPeer {
    pub fn new(peer_id: String) -> Self {
        Self {
            peer_id,
            upload_speed: 1_000_000, // 1 MB/s default
            reliability: 0.95,
            latency: Duration::from_millis(50),
        }
    }

    pub fn builder() -> MockPeerBuilder {
        MockPeerBuilder::new()
    }

    pub fn peer_id(&self) -> &str {
        &self.peer_id
    }

    pub fn upload_speed(&self) -> u64 {
        self.upload_speed
    }

    /// Simulate sending a piece of data
    ///
    /// # Errors
    /// - `PeerError::ConnectionLost` - Simulated connection failure based on reliability
    pub async fn send_piece(&self, piece_size: usize) -> Result<Vec<u8>, PeerError> {
        // Simulate connection reliability
        if rand::random::<f32>() > self.reliability {
            return Err(PeerError::ConnectionLost);
        }

        // Simulate network latency
        tokio::time::sleep(self.latency).await;

        // Simulate upload speed limitation
        let transfer_time = Duration::from_secs_f64(piece_size as f64 / self.upload_speed as f64);
        tokio::time::sleep(transfer_time).await;

        // Return mock piece data
        Ok(vec![0u8; piece_size])
    }
}

pub struct MockPeerBuilder {
    peer_id: Option<String>,
    upload_speed: u64,
    reliability: f32,
    latency: Duration,
}

impl MockPeerBuilder {
    fn new() -> Self {
        Self {
            peer_id: None,
            upload_speed: 1_000_000,
            reliability: 0.95,
            latency: Duration::from_millis(50),
        }
    }

    pub fn peer_id(mut self, id: String) -> Self {
        self.peer_id = Some(id);
        self
    }

    pub fn upload_speed(mut self, bytes_per_second: u64) -> Self {
        self.upload_speed = bytes_per_second;
        self
    }

    pub fn reliability(mut self, rate: f32) -> Self {
        self.reliability = rate.clamp(0.0, 1.0);
        self
    }

    pub fn latency(mut self, duration: Duration) -> Self {
        self.latency = duration;
        self
    }

    pub fn build(self) -> MockPeer {
        let peer_id = self
            .peer_id
            .unwrap_or_else(|| format!("MOCK{:08}", rand::random::<u32>()));

        MockPeer {
            peer_id,
            upload_speed: self.upload_speed,
            reliability: self.reliability,
            latency: self.latency,
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum PeerError {
    #[error("Peer connection lost")]
    ConnectionLost,

    #[error("Peer protocol error: {message}")]
    ProtocolError { message: String },
}
