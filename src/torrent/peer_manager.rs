//! Peer connection management with bandwidth control and connection pooling

use super::{InfoHash, PieceIndex, TorrentError};
use crate::config::RiptideConfig;
use std::{
    collections::HashMap,
    net::SocketAddr,
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::{
    sync::{RwLock, Semaphore},
    time::sleep,
};

/// Manages multiple peer connections with bandwidth throttling
pub struct PeerManager {
    /// Active connections per torrent
    connections: Arc<RwLock<HashMap<InfoHash, Vec<ManagedPeerConnection>>>>,
    /// Global connection limit
    connection_semaphore: Arc<Semaphore>,
    /// Bandwidth throttling state
    bandwidth_limiter: BandwidthLimiter,
    /// Configuration
    config: RiptideConfig,
}

/// Individual peer connection with connection state and statistics
pub struct ManagedPeerConnection {
    /// Remote peer address
    pub addr: SocketAddr,
    /// Connection state
    pub state: ConnectionState,
    /// Transfer statistics
    pub stats: ConnectionStats,
    /// Last activity timestamp
    pub last_activity: Instant,
}

/// Connection state for peer lifecycle management
#[derive(Debug, Clone, PartialEq)]
pub enum ConnectionState {
    /// Attempting to establish connection
    Connecting,
    /// Handshake in progress
    Handshaking,
    /// Connected and ready for requests
    Connected,
    /// Peer is choking us
    Choked,
    /// We are choking the peer
    Choking,
    /// Connection failed or timed out
    Failed { reason: String },
}

/// Connection transfer statistics
#[derive(Debug, Clone)]
pub struct ConnectionStats {
    /// Bytes downloaded from this peer
    pub bytes_downloaded: u64,
    /// Bytes uploaded to this peer
    pub bytes_uploaded: u64,
    /// Download rate in bytes per second
    pub download_rate: f64,
    /// Upload rate in bytes per second
    pub upload_rate: f64,
    /// Number of pieces downloaded
    pub pieces_downloaded: u32,
    /// Last rate calculation time
    last_rate_update: Instant,
}

/// Bandwidth throttling with token bucket algorithm
pub struct BandwidthLimiter {
    /// Download bandwidth limit in bytes per second
    download_limit: Option<u64>,
    /// Upload bandwidth limit in bytes per second
    upload_limit: Option<u64>,
    /// Download tokens available
    download_tokens: Arc<RwLock<f64>>,
    /// Upload tokens available
    upload_tokens: Arc<RwLock<f64>>,
    /// Last token refill time
    last_refill: Arc<RwLock<Instant>>,
}

impl PeerManager {
    /// Creates new peer manager with configuration
    pub fn new(config: RiptideConfig) -> Self {
        let max_connections = config.network.max_peer_connections;
        
        Self {
            connections: Arc::new(RwLock::new(HashMap::new())),
            connection_semaphore: Arc::new(Semaphore::new(max_connections)),
            bandwidth_limiter: BandwidthLimiter::new(
                config.network.download_limit,
                config.network.upload_limit,
            ),
            config,
        }
    }

    /// Add new peer for torrent
    ///
    /// # Errors
    /// - `TorrentError::ConnectionLimitExceeded` - Too many active connections
    /// - `TorrentError::ConnectionFailed` - Failed to establish connection
    pub async fn add_peer(
        &self,
        info_hash: InfoHash,
        addr: SocketAddr,
    ) -> Result<(), TorrentError> {
        // Acquire connection permit
        let _permit = self.connection_semaphore.acquire().await
            .map_err(|_| TorrentError::ConnectionLimitExceeded)?;

        let connection = ManagedPeerConnection {
            addr,
            state: ConnectionState::Connecting,
            stats: ConnectionStats::default(),
            last_activity: Instant::now(),
        };

        let mut connections = self.connections.write().await;
        connections.entry(info_hash)
            .or_insert_with(Vec::new)
            .push(connection);

        Ok(())
    }

    /// Get available peers for piece requests
    pub async fn get_available_peers(
        &self,
        info_hash: InfoHash,
    ) -> Vec<SocketAddr> {
        let connections = self.connections.read().await;
        
        connections.get(&info_hash)
            .map(|peers| {
                peers.iter()
                    .filter(|peer| peer.state == ConnectionState::Connected)
                    .map(|peer| peer.addr)
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Request piece from best available peer
    ///
    /// # Errors
    /// - `TorrentError::NoPeersAvailable` - No connected peers for torrent
    /// - `TorrentError::BandwidthLimitExceeded` - Download rate limit reached
    pub async fn request_piece(
        &self,
        info_hash: InfoHash,
        piece_index: PieceIndex,
        piece_size: u32,
    ) -> Result<Vec<u8>, TorrentError> {
        // Check bandwidth availability
        self.bandwidth_limiter.acquire_download_tokens(piece_size as u64).await?;

        let connections = self.connections.read().await;
        let torrent_peers = connections.get(&info_hash)
            .ok_or(TorrentError::NoPeersAvailable)?;

        // Find best peer (highest download rate, connected state)
        let best_peer = torrent_peers.iter()
            .filter(|peer| peer.state == ConnectionState::Connected)
            .max_by(|a, b| a.stats.download_rate.partial_cmp(&b.stats.download_rate).unwrap())
            .ok_or(TorrentError::NoPeersAvailable)?;

        // Simulate piece download with rate limiting
        self.simulate_piece_download(piece_index, piece_size, best_peer.addr).await
    }

    /// Update peer statistics after successful download
    pub async fn update_peer_stats(
        &self,
        info_hash: InfoHash,
        addr: SocketAddr,
        bytes_downloaded: u64,
    ) {
        let mut connections = self.connections.write().await;
        
        if let Some(peers) = connections.get_mut(&info_hash) {
            if let Some(peer) = peers.iter_mut().find(|p| p.addr == addr) {
                peer.stats.bytes_downloaded += bytes_downloaded;
                peer.last_activity = Instant::now();
                
                // Update download rate calculation
                peer.stats.update_download_rate(bytes_downloaded);
            }
        }
    }

    /// Remove stale connections and cleanup resources
    pub async fn cleanup_stale_connections(&self) {
        let timeout = Duration::from_secs(self.config.network.peer_timeout_seconds);
        let mut connections = self.connections.write().await;
        
        for peers in connections.values_mut() {
            peers.retain(|peer| {
                peer.last_activity.elapsed() < timeout
            });
        }
        
        // Remove empty torrent entries
        connections.retain(|_, peers| !peers.is_empty());
    }

    /// Get connection statistics for monitoring
    pub async fn get_stats(&self) -> PeerManagerStats {
        let connections = self.connections.read().await;
        
        let mut total_connections = 0;
        let mut total_download = 0;
        let mut total_upload = 0;
        
        for peers in connections.values() {
            total_connections += peers.len();
            for peer in peers {
                total_download += peer.stats.bytes_downloaded;
                total_upload += peer.stats.bytes_uploaded;
            }
        }
        
        PeerManagerStats {
            total_connections,
            bytes_downloaded: total_download,
            bytes_uploaded: total_upload,
            active_torrents: connections.len(),
        }
    }

    /// Simulate piece download with proper delay for bandwidth limiting
    async fn simulate_piece_download(
        &self,
        _piece_index: PieceIndex,
        piece_size: u32,
        _peer_addr: SocketAddr,
    ) -> Result<Vec<u8>, TorrentError> {
        // Calculate download time based on bandwidth limit
        let download_time = if let Some(limit) = self.config.network.download_limit {
            Duration::from_millis((piece_size as u64 * 1000) / limit)
        } else {
            Duration::from_millis(50) // Simulate network latency
        };
        
        sleep(download_time).await;
        
        // Return mock piece data
        Ok(vec![0u8; piece_size as usize])
    }
}

impl BandwidthLimiter {
    /// Creates new bandwidth limiter with token bucket algorithm
    pub fn new(download_limit: Option<u64>, upload_limit: Option<u64>) -> Self {
        let now = Instant::now();
        
        Self {
            download_limit,
            upload_limit,
            download_tokens: Arc::new(RwLock::new(0.0)),
            upload_tokens: Arc::new(RwLock::new(0.0)),
            last_refill: Arc::new(RwLock::new(now)),
        }
    }

    /// Acquire tokens for download, blocking if rate limit exceeded
    ///
    /// # Errors
    /// - `TorrentError::BandwidthLimitExceeded` - No bandwidth limit configured but tokens unavailable
    pub async fn acquire_download_tokens(&self, bytes: u64) -> Result<(), TorrentError> {
        if self.download_limit.is_none() {
            return Ok(()); // No limit configured
        }
        
        self.refill_tokens().await;
        
        let mut tokens = self.download_tokens.write().await;
        if *tokens >= bytes as f64 {
            *tokens -= bytes as f64;
            Ok(())
        } else {
            Err(TorrentError::BandwidthLimitExceeded)
        }
    }

    /// Refill token buckets based on elapsed time
    async fn refill_tokens(&self) {
        let now = Instant::now();
        let mut last_refill = self.last_refill.write().await;
        let elapsed = now.duration_since(*last_refill).as_secs_f64();
        
        if elapsed > 0.1 { // Refill every 100ms minimum
            if let Some(download_limit) = self.download_limit {
                let mut download_tokens = self.download_tokens.write().await;
                *download_tokens = (*download_tokens + (download_limit as f64 * elapsed))
                    .min(download_limit as f64 * 2.0); // Max 2 seconds of burst
            }
            
            if let Some(upload_limit) = self.upload_limit {
                let mut upload_tokens = self.upload_tokens.write().await;
                *upload_tokens = (*upload_tokens + (upload_limit as f64 * elapsed))
                    .min(upload_limit as f64 * 2.0);
            }
            
            *last_refill = now;
        }
    }
}

impl ConnectionStats {
    /// Update download rate using exponential moving average
    fn update_download_rate(&mut self, bytes: u64) {
        let now = Instant::now();
        
        // Calculate elapsed time since last update
        let elapsed = now.duration_since(self.last_rate_update).as_secs_f64();
        
        // Only calculate rate if enough time has passed to avoid division by zero
        if elapsed > 0.001 { // At least 1ms
            let instantaneous_rate = bytes as f64 / elapsed;
            // Exponential moving average with alpha = 0.1
            self.download_rate = 0.1 * instantaneous_rate + 0.9 * self.download_rate;
        }
        
        self.last_rate_update = now;
    }
}

impl Default for ConnectionStats {
    fn default() -> Self {
        Self {
            bytes_downloaded: 0,
            bytes_uploaded: 0,
            download_rate: 0.0,
            upload_rate: 0.0,
            pieces_downloaded: 0,
            last_rate_update: Instant::now(),
        }
    }
}

/// Overall peer manager statistics
#[derive(Debug, Clone)]
pub struct PeerManagerStats {
    /// Total active connections across all torrents
    pub total_connections: usize,
    /// Total bytes downloaded
    pub bytes_downloaded: u64,
    /// Total bytes uploaded
    pub bytes_uploaded: u64,
    /// Number of active torrents
    pub active_torrents: usize,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::torrent::test_data::create_test_info_hash;
    use std::net::{IpAddr, Ipv4Addr};

    #[tokio::test]
    async fn test_peer_manager_add_peer() {
        let config = RiptideConfig::default();
        let manager = PeerManager::new(config);
        let info_hash = create_test_info_hash();
        let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 8080);

        manager.add_peer(info_hash, addr).await.unwrap();
        
        let connections = manager.connections.read().await;
        let peers = connections.get(&info_hash).unwrap();
        assert_eq!(peers.len(), 1);
        assert_eq!(peers[0].addr, addr);
        assert_eq!(peers[0].state, ConnectionState::Connecting);
    }

    #[tokio::test]
    async fn test_bandwidth_limiter_no_limit() {
        let limiter = BandwidthLimiter::new(None, None);
        
        // Should always succeed when no limit is set
        limiter.acquire_download_tokens(1024).await.unwrap();
        limiter.acquire_download_tokens(1024 * 1024).await.unwrap();
    }

    #[tokio::test]
    async fn test_connection_stats_rate_calculation() {
        let mut stats = ConnectionStats::default();
        
        // Wait and do download
        sleep(Duration::from_millis(200)).await;
        stats.update_download_rate(2048);
        
        // Rate should be approximately 10KB/s (2048 bytes / 0.2 seconds)
        // Use exponential moving average starting from 0: 0.1 * 10240 + 0.9 * 0 = 1024
        assert!(stats.download_rate > 500.0);
        assert!(stats.download_rate < 2000.0);
    }

    #[tokio::test]
    async fn test_cleanup_stale_connections() {
        let mut config = RiptideConfig::default();
        config.network.peer_timeout_seconds = 1; // 1 second timeout
        
        let manager = PeerManager::new(config);
        let info_hash = create_test_info_hash();
        let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 8080);

        manager.add_peer(info_hash, addr).await.unwrap();
        
        // Wait for timeout
        sleep(Duration::from_secs(2)).await;
        manager.cleanup_stale_connections().await;
        
        let connections = manager.connections.read().await;
        assert!(connections.is_empty());
    }
}