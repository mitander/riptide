//! Development peer implementation for fast streaming iteration.
//!
//! Provides realistic BitTorrent protocol simulation optimized for development speed.
//! Features minimal delays, reliable piece serving, and realistic streaming rates.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use riptide_core::torrent::{
    ConnectionStatus, InfoHash, PeerId, PeerInfo, PeerManager, PeerMessage, PeerMessageEvent,
    PieceIndex, PieceStore, TorrentError,
};
use tokio::sync::mpsc;
use tokio::time::sleep;

/// Development peer implementation for fast streaming iteration.
///
/// Optimized for realistic streaming performance during development with:
/// - Target streaming rate: 8-12 MB/s (configurable)
/// - Minimal protocol delays for fast iteration
/// - Reliable piece serving without network failures
/// - Performance statistics for development insights
pub struct DevelopmentPeers<P: PieceStore> {
    piece_store: Arc<P>,
    connected_peers: HashMap<SocketAddr, MockPeer>,
    message_sender: mpsc::UnboundedSender<PeerMessageEvent>,
    message_receiver: mpsc::UnboundedReceiver<PeerMessageEvent>,
    stats: PerformanceStats,
    rate_limiter: StreamingRateLimiter,
    current_streaming_position: HashMap<InfoHash, u64>,
}

/// Mock peer for development simulation.
#[derive(Clone, Debug)]
struct MockPeer {
    address: SocketAddr,
    info_hash: InfoHash,
    peer_id: PeerId,
    connected_at: Instant,
    bytes_served: u64,
    pieces_served: u64,
}

/// Performance statistics for development peer tracking.
#[derive(Clone, Debug)]
pub struct DevelopmentStats {
    pub pieces_served: u64,
    pub bytes_served: u64,
    pub throughput_mbps: f64,
    pub uptime_seconds: u64,
}

/// Performance tracking for development peers.
#[derive(Debug)]
struct PerformanceStats {
    pieces_served: u64,
    bytes_served: u64,
    startup_time: Instant,
}

/// Realistic streaming rate limiter for development.
///
/// Simulates real-world BitTorrent performance for development testing
/// while maintaining fast iteration speeds.
#[derive(Debug)]
struct StreamingRateLimiter {
    target_bytes_per_second: u64,
    last_transfer_time: Instant,
    bytes_this_second: u64,
}

impl StreamingRateLimiter {
    /// Creates rate limiter targeting realistic streaming performance.
    ///
    /// Default: 8 MB/s (suitable for 4K streaming with buffer).
    fn new_for_streaming() -> Self {
        Self {
            target_bytes_per_second: 8 * 1024 * 1024, // 8 MB/s
            last_transfer_time: Instant::now(),
            bytes_this_second: 0,
        }
    }

    /// Applies rate limiting for realistic streaming simulation.
    async fn apply_rate_limit(&mut self, bytes_transferred: u64) {
        let now = Instant::now();
        let elapsed = now.duration_since(self.last_transfer_time);

        // Reset counter every second
        if elapsed >= Duration::from_secs(1) {
            self.bytes_this_second = 0;
            self.last_transfer_time = now;
        }

        self.bytes_this_second += bytes_transferred;

        // Apply gentle throttling if exceeding target rate
        if self.bytes_this_second > self.target_bytes_per_second {
            let delay_ms = 50; // Light throttling for realism
            sleep(Duration::from_millis(delay_ms)).await;
        }
    }
}

impl MockPeer {
    fn new(address: SocketAddr, info_hash: InfoHash, peer_id: PeerId) -> Self {
        Self {
            address,
            info_hash,
            peer_id,
            connected_at: Instant::now(),
            bytes_served: 0,
            pieces_served: 0,
        }
    }

    fn as_peer_info(&self) -> PeerInfo {
        PeerInfo {
            address: self.address,
            status: ConnectionStatus::Connected,
            connected_at: Some(self.connected_at),
            last_activity: Instant::now(),
            bytes_downloaded: 0,
            bytes_uploaded: self.bytes_served,
        }
    }
}

impl<P: PieceStore> DevelopmentPeers<P> {
    /// Creates new development peers manager.
    ///
    /// Optimized for realistic streaming performance during development.
    pub fn new(piece_store: Arc<P>) -> Self {
        tracing::info!("DevelopmentPeers: Initializing with realistic streaming rates");

        let (message_sender, message_receiver) = mpsc::unbounded_channel();

        Self {
            piece_store,
            connected_peers: HashMap::new(),
            message_sender,
            message_receiver,
            stats: PerformanceStats {
                pieces_served: 0,
                bytes_served: 0,
                startup_time: Instant::now(),
            },
            rate_limiter: StreamingRateLimiter::new_for_streaming(),
            current_streaming_position: HashMap::new(),
        }
    }

    /// Returns current performance statistics.
    pub fn performance_stats(&self) -> DevelopmentStats {
        let uptime_seconds = self.stats.startup_time.elapsed().as_secs();
        let throughput_mbps = if uptime_seconds > 0 {
            (self.stats.bytes_served as f64 * 8.0) / (uptime_seconds as f64 * 1_000_000.0)
        } else {
            0.0
        };

        DevelopmentStats {
            pieces_served: self.stats.pieces_served,
            bytes_served: self.stats.bytes_served,
            throughput_mbps,
            uptime_seconds,
        }
    }

    /// Handles piece request with realistic rate limiting.
    async fn handle_piece_request(
        &mut self,
        peer_address: SocketAddr,
        piece_index: PieceIndex,
    ) -> Result<(), TorrentError> {
        // Find the peer's info hash
        let info_hash = self
            .connected_peers
            .get(&peer_address)
            .map(|peer| peer.info_hash)
            .ok_or_else(|| TorrentError::PeerConnectionError {
                reason: "Peer not found".to_string(),
            })?;

        // Get piece data from store
        let piece_data = self.piece_store.piece_data(info_hash, piece_index).await?;
        let block_size = piece_data.len() as u64;

        // Apply realistic rate limiting
        self.rate_limiter.apply_rate_limit(block_size).await;

        // Update statistics
        self.stats.pieces_served += 1;
        self.stats.bytes_served += block_size;

        if let Some(peer) = self.connected_peers.get_mut(&peer_address) {
            peer.pieces_served += 1;
            peer.bytes_served += block_size;
        }

        // Send piece message
        let piece_message = PeerMessage::Piece {
            piece_index,
            offset: 0,
            data: piece_data.into(),
        };

        let event = PeerMessageEvent {
            peer_address,
            message: piece_message,
            received_at: Instant::now(),
        };

        self.message_sender
            .send(event)
            .map_err(|_| TorrentError::PeerConnectionError {
                reason: "DevelopmentPeers message queue closed".to_string(),
            })?;

        tracing::debug!(
            "DevelopmentPeers: Served {}B block for piece {} to {} (rate limited)",
            block_size,
            piece_index.as_u32(),
            peer_address
        );

        Ok(())
    }
}

#[async_trait]
impl<P: PieceStore + Send + Sync + 'static> PeerManager for DevelopmentPeers<P> {
    /// Connects to peer for development simulation.
    async fn connect_peer(
        &mut self,
        peer_address: SocketAddr,
        info_hash: InfoHash,
        peer_id: PeerId,
    ) -> Result<(), TorrentError> {
        tracing::debug!(
            "DevelopmentPeers: Connecting to peer {} for {}",
            peer_address,
            info_hash
        );

        let mock_peer = MockPeer::new(peer_address, info_hash, peer_id);
        self.connected_peers.insert(peer_address, mock_peer);

        tracing::debug!(
            "DevelopmentPeers: Connected to peer {} (total: {})",
            peer_address,
            self.connected_peers.len()
        );

        Ok(())
    }

    /// Disconnects from peer.
    async fn disconnect_peer(&mut self, peer_address: SocketAddr) -> Result<(), TorrentError> {
        if self.connected_peers.remove(&peer_address).is_some() {
            tracing::debug!(
                "DevelopmentPeers: Disconnected from peer {} (remaining: {})",
                peer_address,
                self.connected_peers.len()
            );
        }
        Ok(())
    }

    /// Sends message to peer with immediate processing for development speed.
    async fn send_message(
        &mut self,
        peer_address: SocketAddr,
        message: PeerMessage,
    ) -> Result<(), TorrentError> {
        match message {
            PeerMessage::Request {
                piece_index,
                offset: _,
                length: _,
            } => {
                // Handle piece request immediately for development speed
                self.handle_piece_request(peer_address, piece_index).await?;
            }
            PeerMessage::Interested => {
                tracing::debug!(
                    "DevelopmentPeers: Peer {} interested - unchoking immediately",
                    peer_address
                );
                // Auto-unchoke for development convenience
            }
            _ => {
                tracing::debug!(
                    "DevelopmentPeers: Handling message {:?} to {}",
                    message,
                    peer_address
                );
            }
        }
        Ok(())
    }

    /// Receives next message from development peers.
    async fn receive_message(&mut self) -> Result<PeerMessageEvent, TorrentError> {
        self.message_receiver
            .recv()
            .await
            .ok_or_else(|| TorrentError::PeerConnectionError {
                reason: "DevelopmentPeers message queue closed".to_string(),
            })
    }

    /// Returns list of connected development peers.
    async fn connected_peers(&self) -> Vec<PeerInfo> {
        self.connected_peers
            .values()
            .map(|peer| peer.as_peer_info())
            .collect()
    }

    /// Returns count of active peer connections.
    async fn connection_count(&self) -> usize {
        self.connected_peers.len()
    }

    /// Returns upload statistics aggregated across all peers.
    async fn upload_stats(&self) -> (u64, u64) {
        let total_uploaded = self.stats.bytes_served;
        let upload_rate = if self.stats.startup_time.elapsed().as_secs() > 0 {
            total_uploaded / self.stats.startup_time.elapsed().as_secs()
        } else {
            0
        };
        (total_uploaded, upload_rate)
    }

    /// Shuts down development peers with final statistics.
    async fn shutdown(&mut self) -> Result<(), TorrentError> {
        tracing::info!("DevelopmentPeers: Shutting down");
        let stats = self.performance_stats();
        tracing::info!(
            "DevelopmentPeers: Final stats - {} pieces, {:.1} MB, {:.1} Mbps",
            stats.pieces_served,
            stats.bytes_served as f64 / 1_048_576.0,
            stats.throughput_mbps
        );

        self.connected_peers.clear();
        Ok(())
    }

    /// Configures upload management for development scenarios.
    async fn configure_upload_manager(
        &mut self,
        info_hash: InfoHash,
        piece_size: u64,
        total_bandwidth: u64,
    ) -> Result<(), TorrentError> {
        tracing::debug!(
            "DevelopmentPeers: Configure upload manager for {} - piece_size: {}, bandwidth: {}",
            info_hash,
            piece_size,
            total_bandwidth
        );

        // Update rate limiter based on bandwidth allocation
        self.rate_limiter.target_bytes_per_second = total_bandwidth.min(50 * 1024 * 1024); // Cap at 50 MB/s

        Ok(())
    }

    /// Updates streaming position for development tracking.
    async fn update_streaming_position(
        &mut self,
        info_hash: InfoHash,
        byte_position: u64,
    ) -> Result<(), TorrentError> {
        tracing::debug!(
            "DevelopmentPeers: Update streaming position for {} to byte {}",
            info_hash,
            byte_position
        );

        self.current_streaming_position
            .insert(info_hash, byte_position);

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use riptide_core::torrent::test_data::create_test_piece_store;

    use super::*;

    #[tokio::test]
    async fn test_development_peers_creation() {
        let piece_store = create_test_piece_store();
        let peers = DevelopmentPeers::new(piece_store);

        let stats = peers.performance_stats();
        assert_eq!(stats.pieces_served, 0);
        assert_eq!(stats.bytes_served, 0);
    }

    #[tokio::test]
    async fn test_peer_connection() {
        let piece_store = create_test_piece_store();
        let mut peers = DevelopmentPeers::new(piece_store);

        let peer_addr = "127.0.0.1:8080".parse().unwrap();
        let info_hash = InfoHash::new([1u8; 20]);
        let peer_id = PeerId::new([0u8; 20]);

        let result = peers.connect_peer(peer_addr, info_hash, peer_id).await;
        assert!(result.is_ok());

        let connected = peers.connected_peers().await;
        assert_eq!(connected.len(), 1);
        assert_eq!(connected[0].address, peer_addr);
    }

    #[tokio::test]
    async fn test_realistic_performance() {
        let piece_store = create_test_piece_store();
        let peers = DevelopmentPeers::new(piece_store);

        // Should target realistic streaming rates
        assert_eq!(peers.rate_limiter.target_bytes_per_second, 8 * 1024 * 1024);
    }
}
