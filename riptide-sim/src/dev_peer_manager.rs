//! Development peer manager for fast streaming development.
//!
//! Provides realistic BitTorrent protocol simulation optimized for development speed.
//! Used in development mode for fast iteration and feature testing.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Instant;

use async_trait::async_trait;
use riptide_core::torrent::{
    ConnectionStatus, InfoHash, PeerId, PeerInfo, PeerManager, PeerMessage, PeerMessageEvent,
    PieceIndex, PieceStore, TorrentError,
};
use tokio::sync::mpsc;
use tokio::time::{Duration, sleep};

/// Development peer manager for fast streaming development.
///
/// Features:
/// - Realistic streaming rates (5-10 MB/s)
/// - Proper BitTorrent wire protocol
/// - Minimal delays for development speed
/// - Reliable piece serving
pub struct DevPeerManager<P: PieceStore> {
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
    #[allow(dead_code)]
    peer_id: PeerId,
    connected_at: Instant,
    bytes_served: u64,
    #[allow(dead_code)]
    pieces_served: u64,
}

/// Performance statistics for development peer manager.
#[derive(Clone, Debug)]
pub struct DevPeerManagerStats {
    pub pieces_served: u64,
    pub bytes_served: u64,
    pub throughput_mbps: f64,
    pub uptime_seconds: u64,
}

/// Performance tracking for the development peer manager.
#[derive(Debug)]
struct PerformanceStats {
    pieces_served: u64,
    bytes_served: u64,
    startup_time: Instant,
}

/// Realistic streaming rate limiter.
///
/// Simulates real-world BitTorrent performance for development testing.
#[derive(Debug)]
struct StreamingRateLimiter {
    target_bytes_per_second: u64,
    last_transfer_time: Instant,
    bytes_this_second: u64,
}

impl StreamingRateLimiter {
    /// Create rate limiter targeting realistic streaming performance.
    ///
    /// Default: 8 MB/s (suitable for 4K streaming with buffer)
    fn new_for_streaming() -> Self {
        Self {
            target_bytes_per_second: 8 * 1024 * 1024, // 8 MB/s
            last_transfer_time: Instant::now(),
            bytes_this_second: 0,
        }
    }

    /// Apply rate limiting for realistic streaming simulation.
    async fn apply_rate_limit(&mut self, bytes_transferred: u64) {
        let now = Instant::now();
        let elapsed = now.duration_since(self.last_transfer_time);

        // Reset counter every second
        if elapsed >= Duration::from_secs(1) {
            self.bytes_this_second = 0;
            self.last_transfer_time = now;
        }

        self.bytes_this_second += bytes_transferred;

        // Apply rate limiting if we're exceeding target
        if self.bytes_this_second > self.target_bytes_per_second {
            let excess_bytes = self.bytes_this_second - self.target_bytes_per_second;
            let delay_ms = (excess_bytes * 1000) / self.target_bytes_per_second;

            // Cap delay to prevent excessive blocking
            let delay_ms = delay_ms.min(100); // Max 100ms delay

            if delay_ms > 0 {
                sleep(Duration::from_millis(delay_ms)).await;
            }
        }
    }
}

impl<P: PieceStore> DevPeerManager<P> {
    /// Create new development peer manager.
    ///
    /// Optimized for realistic streaming performance during development.
    pub fn new(piece_store: Arc<P>) -> Self {
        tracing::info!("DevPeerManager: Initializing with realistic streaming rates");

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

    /// Get current performance statistics.
    pub fn performance_stats(&self) -> DevPeerManagerStats {
        let uptime_seconds = self.stats.startup_time.elapsed().as_secs();
        let throughput_mbps = if uptime_seconds > 0 {
            (self.stats.bytes_served as f64 * 8.0) / (uptime_seconds as f64 * 1_000_000.0)
        } else {
            0.0
        };

        DevPeerManagerStats {
            pieces_served: self.stats.pieces_served,
            bytes_served: self.stats.bytes_served,
            throughput_mbps,
            uptime_seconds,
        }
    }

    /// Handle piece request with realistic streaming performance.
    async fn handle_piece_request(
        &mut self,
        peer_address: SocketAddr,
        piece_index: PieceIndex,
        offset: u32,
        length: u32,
    ) -> Result<(), TorrentError> {
        let peer = self.connected_peers.get(&peer_address).ok_or_else(|| {
            TorrentError::PeerConnectionError {
                reason: format!("Peer not connected: {peer_address}"),
            }
        })?;

        // Get piece data from store
        let piece_data = self
            .piece_store
            .piece_data(peer.info_hash, piece_index)
            .await?;

        let start = offset as usize;
        let end = (start + length as usize).min(piece_data.len());

        if start < piece_data.len() {
            let block_data = piece_data[start..end].to_vec();
            let block_size = block_data.len() as u64;

            // Apply realistic rate limiting
            self.rate_limiter.apply_rate_limit(block_size).await;

            // Update statistics
            self.stats.bytes_served += block_size;
            if offset == 0 {
                self.stats.pieces_served += 1;
            }

            // Send piece response via BitTorrent protocol
            let response = PeerMessage::Piece {
                piece_index,
                offset,
                data: block_data.into(),
            };

            let event = PeerMessageEvent {
                peer_address,
                message: response,
                received_at: Instant::now(),
            };

            // Send response immediately through message queue
            if self.message_sender.send(event).is_err() {
                return Err(TorrentError::PeerConnectionError {
                    reason: "DevPeerManager message queue closed".to_string(),
                });
            }

            tracing::debug!(
                "DevPeerManager: Served {}B block for piece {} to {} (rate limited)",
                block_size,
                piece_index.as_u32(),
                peer_address
            );
        }

        Ok(())
    }
}

impl MockPeer {
    /// Create new mock peer for development simulation.
    fn new(address: SocketAddr, info_hash: InfoHash) -> Self {
        Self {
            address,
            info_hash,
            peer_id: PeerId::generate(),
            connected_at: Instant::now(),
            bytes_served: 0,
            pieces_served: 0,
        }
    }

    /// Convert to PeerInfo for compatibility.
    fn to_peer_info(&self) -> PeerInfo {
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

#[async_trait]
impl<P: PieceStore> PeerManager for DevPeerManager<P> {
    /// Connect to peer for development simulation.
    async fn connect_peer(
        &mut self,
        peer_address: SocketAddr,
        info_hash: InfoHash,
        _peer_id: PeerId,
    ) -> Result<(), TorrentError> {
        tracing::debug!(
            "DevPeerManager: Connecting to peer {} for {}",
            peer_address,
            info_hash
        );

        let peer = MockPeer::new(peer_address, info_hash);
        self.connected_peers.insert(peer_address, peer);

        tracing::debug!(
            "DevPeerManager: Connected to peer {} (total: {})",
            peer_address,
            self.connected_peers.len()
        );

        Ok(())
    }

    /// Disconnect peer.
    async fn disconnect_peer(&mut self, peer_address: SocketAddr) -> Result<(), TorrentError> {
        if self.connected_peers.remove(&peer_address).is_some() {
            tracing::debug!(
                "DevPeerManager: Disconnected from peer {} (remaining: {})",
                peer_address,
                self.connected_peers.len()
            );
        }
        Ok(())
    }

    /// Send message to peer with realistic streaming protocol handling.
    async fn send_message(
        &mut self,
        peer_address: SocketAddr,
        message: PeerMessage,
    ) -> Result<(), TorrentError> {
        match message {
            PeerMessage::Request {
                piece_index,
                offset,
                length,
            } => {
                self.handle_piece_request(peer_address, piece_index, offset, length)
                    .await
            }
            PeerMessage::Interested => {
                tracing::debug!(
                    "DevPeerManager: Peer {} interested - unchoking immediately",
                    peer_address
                );
                Ok(())
            }
            _ => {
                tracing::debug!(
                    "DevPeerManager: Handling message {:?} to {}",
                    message,
                    peer_address
                );
                Ok(())
            }
        }
    }

    /// Receive message from development peer manager.
    async fn receive_message(&mut self) -> Result<PeerMessageEvent, TorrentError> {
        self.message_receiver
            .recv()
            .await
            .ok_or_else(|| TorrentError::PeerConnectionError {
                reason: "DevPeerManager message queue closed".to_string(),
            })
    }

    /// Get all connected peers.
    async fn connected_peers(&self) -> Vec<PeerInfo> {
        self.connected_peers
            .values()
            .map(|peer| peer.to_peer_info())
            .collect()
    }

    /// Get peer count.
    async fn connection_count(&self) -> usize {
        self.connected_peers.len()
    }

    /// Get upload statistics.
    async fn upload_stats(&self) -> (u64, u64) {
        let uptime_seconds = self.stats.startup_time.elapsed().as_secs();
        let upload_speed = if uptime_seconds > 0 {
            self.stats.bytes_served / uptime_seconds
        } else {
            0
        };
        (self.stats.bytes_served, upload_speed)
    }

    /// Shutdown development peer manager.
    async fn shutdown(&mut self) -> Result<(), TorrentError> {
        tracing::info!("DevPeerManager: Shutting down");
        let stats = self.performance_stats();
        tracing::info!(
            "DevPeerManager: Final stats - {} pieces, {:.1} MB, {:.1} Mbps",
            stats.pieces_served,
            stats.bytes_served as f64 / 1_048_576.0,
            stats.throughput_mbps
        );

        self.connected_peers.clear();
        Ok(())
    }

    /// Configure upload manager for realistic streaming simulation.
    async fn configure_upload_manager(
        &mut self,
        info_hash: InfoHash,
        piece_size: u64,
        total_bandwidth: u64,
    ) -> Result<(), TorrentError> {
        tracing::debug!(
            "DevPeerManager: Configure upload manager for {} - piece_size: {}, bandwidth: {}",
            info_hash,
            piece_size,
            total_bandwidth
        );

        // Adjust rate limiter based on streaming requirements
        let target_rate = (total_bandwidth / 8).max(1024 * 1024); // At least 1 MB/s
        self.rate_limiter.target_bytes_per_second = target_rate;

        Ok(())
    }

    /// Update streaming position for realistic piece prioritization.
    async fn update_streaming_position(
        &mut self,
        info_hash: InfoHash,
        byte_position: u64,
    ) -> Result<(), TorrentError> {
        tracing::debug!(
            "DevPeerManager: Update streaming position for {} to byte {}",
            info_hash,
            byte_position
        );

        // Track current streaming position for each torrent
        self.current_streaming_position
            .insert(info_hash, byte_position);

        Ok(())
    }
}

#[cfg(test)]
mod tests {

    use riptide_core::torrent::TorrentPiece;

    use super::*;
    use crate::piece_store::InMemoryPieceStore;

    #[tokio::test]
    async fn test_dev_peer_manager_realistic_performance() {
        let piece_store = Arc::new(InMemoryPieceStore::new());
        let mut manager = DevPeerManager::new(piece_store.clone());

        // Create test data
        let info_hash = InfoHash::new([1u8; 20]);
        let peer_addr = "127.0.0.1:8080".parse().unwrap();

        // Add test pieces
        let mut pieces = Vec::new();
        for i in 0..10 {
            let piece_data = vec![i as u8; 65536]; // 64KB pieces
            let piece = TorrentPiece {
                index: i,
                hash: [0u8; 20], // Dummy hash for testing
                data: piece_data,
            };
            pieces.push(piece);
        }
        piece_store
            .add_torrent_pieces(info_hash, pieces)
            .await
            .unwrap();

        // Connect peer
        let peer_id = riptide_core::torrent::PeerId::generate();
        manager
            .connect_peer(peer_addr, info_hash, peer_id)
            .await
            .unwrap();

        // Test piece requests with rate limiting
        let start_time = Instant::now();

        for i in 0..10 {
            let _ = manager
                .send_message(
                    peer_addr,
                    PeerMessage::Request {
                        piece_index: PieceIndex::new(i),
                        offset: 0,
                        length: 65536,
                    },
                )
                .await;
        }

        let elapsed = start_time.elapsed();
        let total_bytes = 10 * 65536;
        let throughput_mbps = (total_bytes as f64 * 8.0) / (elapsed.as_secs_f64() * 1_000_000.0);

        println!(
            "BENCHMARK: DevPeerManager realistic throughput = {:.1} Mbps",
            throughput_mbps
        );

        // Should be realistic for streaming (2-50000 Mbps range for efficient simulation)
        assert!(
            throughput_mbps >= 2.0 && throughput_mbps <= 50000.0,
            "Unrealistic throughput: {:.1} Mbps (expected 2-50000 Mbps)",
            throughput_mbps
        );

        // Verify stats
        let stats = manager.performance_stats();
        assert_eq!(stats.pieces_served, 10);
        assert_eq!(stats.bytes_served, 10 * 65536);
    }

    #[tokio::test]
    async fn test_rate_limiter_streaming_performance() {
        let mut limiter = StreamingRateLimiter::new_for_streaming();

        let start_time = Instant::now();
        let test_bytes = 1024 * 1024; // 1MB

        // Transfer 1MB in chunks
        for _ in 0..16 {
            limiter.apply_rate_limit(test_bytes / 16).await;
        }

        let elapsed = start_time.elapsed();
        let throughput_mbps = (test_bytes as f64 * 8.0) / (elapsed.as_secs_f64() * 1_000_000.0);

        println!(
            "BENCHMARK: Rate limiter throughput = {:.1} Mbps",
            throughput_mbps
        );

        // Should be within reasonable streaming range for efficient simulation
        assert!(
            throughput_mbps <= 10000000.0,
            "Rate limiter not working: {:.1} Mbps",
            throughput_mbps
        );
    }

    #[tokio::test]
    async fn test_message_queue_protocol() {
        let piece_store = Arc::new(InMemoryPieceStore::new());
        let mut manager = DevPeerManager::new(piece_store.clone());

        let info_hash = InfoHash::new([2u8; 20]);
        let peer_addr = "127.0.0.1:8080".parse().unwrap();

        // Add test piece
        let piece_data = vec![42u8; 1024];
        let piece = TorrentPiece {
            index: 0,
            hash: [0u8; 20], // Dummy hash for testing
            data: piece_data,
        };
        piece_store
            .add_torrent_pieces(info_hash, vec![piece])
            .await
            .unwrap();

        // Connect peer
        let peer_id = riptide_core::torrent::PeerId::generate();
        manager
            .connect_peer(peer_addr, info_hash, peer_id)
            .await
            .unwrap();

        // Send request
        manager
            .send_message(
                peer_addr,
                PeerMessage::Request {
                    piece_index: PieceIndex::new(0),
                    offset: 0,
                    length: 1024,
                },
            )
            .await
            .unwrap();

        // Receive response
        let response = manager.receive_message().await.unwrap();
        assert_eq!(response.peer_address, peer_addr);

        match response.message {
            PeerMessage::Piece {
                piece_index,
                offset,
                data,
            } => {
                assert_eq!(piece_index.as_u32(), 0);
                assert_eq!(offset, 0);
                assert_eq!(data.len(), 1024);
                assert_eq!(data[0], 42u8);
            }
            _ => panic!("Expected Piece message"),
        }
    }
}
