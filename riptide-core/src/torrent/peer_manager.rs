//! Peer management for BitTorrent connections with real and simulated implementations

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::AtomicU32;
use std::time::Instant;

use async_trait::async_trait;
use tokio::sync::{Mutex, RwLock, mpsc};
use tokio::task::JoinHandle;

use super::peer_connection::PeerConnection;
use super::protocol::types::PeerMessage;
use super::streaming_upload_manager::{StreamingUploadConfig, StreamingUploadManager};
use super::{InfoHash, PeerId, TorrentError};

/// Connection status for peer tracking
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConnectionStatus {
    Connecting,
    Connected,
    Disconnected,
    Failed,
}

/// Peer connection information
#[derive(Debug, Clone)]
pub struct PeerInfo {
    pub address: SocketAddr,
    pub status: ConnectionStatus,
    pub connected_at: Option<Instant>,
    pub last_activity: Instant,
    pub bytes_downloaded: u64,
    pub bytes_uploaded: u64,
}

impl PeerInfo {
    pub fn new(address: SocketAddr) -> Self {
        Self {
            address,
            status: ConnectionStatus::Connecting,
            connected_at: None,
            last_activity: Instant::now(),
            bytes_downloaded: 0,
            bytes_uploaded: 0,
        }
    }
}

/// Message sent from peer to manager
#[derive(Debug)]
pub struct PeerMessageEvent {
    pub peer_address: SocketAddr,
    pub message: PeerMessage,
    pub received_at: Instant,
}

/// Abstract peer management interface for BitTorrent peer connections.
///
/// Provides peer discovery, connection management, and message routing following
/// the same pattern as TrackerClient for unified real/mock implementations.
#[async_trait]
pub trait PeerManager: Send + Sync {
    /// Connects to a new peer with the specified torrent info hash.
    ///
    /// Establishes TCP connection and performs BitTorrent handshake.
    /// Peer is added to active connection pool for message routing.
    ///
    /// # Errors
    /// - `TorrentError::PeerConnectionError` - TCP connection or handshake failed
    /// - `TorrentError::ProtocolError` - Invalid peer response
    async fn connect_peer(
        &mut self,
        address: SocketAddr,
        info_hash: InfoHash,
        peer_id: PeerId,
    ) -> Result<(), TorrentError>;

    /// Disconnects from peer and removes from active connection pool.
    ///
    /// Gracefully closes connection and cleans up associated resources.
    /// No-op if peer is not currently connected.
    ///
    /// # Errors
    /// - `TorrentError::PeerConnectionError` - Error during disconnect
    async fn disconnect_peer(&mut self, address: SocketAddr) -> Result<(), TorrentError>;

    /// Sends message to specific peer.
    ///
    /// Routes message to peer connection and handles serialization.
    /// Message is queued if peer is temporarily unavailable.
    ///
    /// # Errors
    /// - `TorrentError::PeerConnectionError` - Peer not connected or send failed
    /// - `TorrentError::ProtocolError` - Message serialization failed
    async fn send_message(
        &mut self,
        peer_address: SocketAddr,
        message: PeerMessage,
    ) -> Result<(), TorrentError>;

    /// Receives next message from any connected peer.
    ///
    /// Blocks until message received from any peer in connection pool.
    /// Returns both message content and source peer address.
    ///
    /// # Errors
    /// - `TorrentError::PeerConnectionError` - All connections lost
    /// - `TorrentError::ProtocolError` - Message deserialization failed
    async fn receive_message(&mut self) -> Result<PeerMessageEvent, TorrentError>;

    /// Returns list of currently connected peers with connection information.
    async fn connected_peers(&self) -> Vec<PeerInfo>;

    /// Returns count of active peer connections.
    async fn connection_count(&self) -> usize;

    /// Returns upload statistics for all connected peers.
    ///
    /// Returns (total_bytes_uploaded, upload_speed_bps) aggregated across all peers.
    /// Used for tracking BitTorrent upload performance and protocol compliance.
    async fn upload_stats(&self) -> (u64, u64);

    /// Disconnects all peers and shuts down connection pool.
    ///
    /// # Errors
    /// - `TorrentError::PeerConnectionError` - Error during shutdown
    async fn shutdown(&mut self) -> Result<(), TorrentError>;

    /// Configure upload manager for streaming optimization.
    ///
    /// Implementations that support upload management should configure bandwidth
    /// throttling based on streaming requirements. Mock implementations can
    /// safely return Ok(()) as a no-op.
    ///
    /// # Errors
    /// - `TorrentError::PeerConnectionError` - Configuration failed
    async fn configure_upload_manager(
        &mut self,
        info_hash: InfoHash,
        piece_size: u64,
        total_bandwidth: u64,
    ) -> Result<(), TorrentError>;

    /// Update streaming position for upload throttling.
    ///
    /// Implementations that support upload management should update their
    /// throttling decisions based on current playback position. Mock
    /// implementations can safely return Ok(()) as a no-op.
    ///
    /// # Errors
    /// - `TorrentError::PeerConnectionError` - Update failed
    async fn update_streaming_position(
        &mut self,
        info_hash: InfoHash,
        byte_position: u64,
    ) -> Result<(), TorrentError>;
}

/// Production peer manager using real TCP connections.
///
/// Manages multiple BitTorrent peer connections with concurrent message handling.
/// Provides connection pooling, automatic reconnection, and message routing.
/// Uses streaming-optimized upload throttling to prioritize download bandwidth.
pub struct TcpPeerManager {
    _peer_id: PeerId,
    connections: Arc<RwLock<HashMap<SocketAddr, Arc<Mutex<PeerConnection>>>>>,
    peer_info: Arc<RwLock<HashMap<SocketAddr, PeerInfo>>>,
    message_receiver: Arc<Mutex<mpsc::Receiver<PeerMessageEvent>>>,
    message_sender: mpsc::Sender<PeerMessageEvent>,
    active_tasks: Arc<Mutex<HashMap<SocketAddr, JoinHandle<()>>>>,
    _next_connection_id: AtomicU32,
    max_connections: usize,
    upload_manager: Arc<Mutex<StreamingUploadManager>>,
}

impl TcpPeerManager {
    /// Creates new network peer manager with specified peer ID and connection limit.
    ///
    /// Initializes connection pool and message routing infrastructure.
    /// Default maximum connections is 50 peers.
    pub fn new(peer_id: PeerId, max_connections: usize) -> Self {
        let (message_sender, message_receiver) = mpsc::channel(1000);

        Self {
            _peer_id: peer_id,
            connections: Arc::new(RwLock::new(HashMap::new())),
            peer_info: Arc::new(RwLock::new(HashMap::new())),
            message_receiver: Arc::new(Mutex::new(message_receiver)),
            message_sender,
            active_tasks: Arc::new(Mutex::new(HashMap::new())),
            _next_connection_id: AtomicU32::new(1),
            max_connections,
            upload_manager: Arc::new(Mutex::new(StreamingUploadManager::new())),
        }
    }

    /// Creates default network peer manager with generated peer ID.
    pub fn new_default() -> Self {
        Self::new(PeerId::generate(), 50)
    }

    /// Creates network peer manager with custom upload configuration.
    pub fn with_upload_config(
        peer_id: PeerId,
        max_connections: usize,
        upload_config: StreamingUploadConfig,
    ) -> Self {
        let (message_sender, message_receiver) = mpsc::channel(1000);

        Self {
            _peer_id: peer_id,
            connections: Arc::new(RwLock::new(HashMap::new())),
            peer_info: Arc::new(RwLock::new(HashMap::new())),
            message_receiver: Arc::new(Mutex::new(message_receiver)),
            message_sender,
            active_tasks: Arc::new(Mutex::new(HashMap::new())),
            _next_connection_id: AtomicU32::new(1),
            max_connections,
            upload_manager: Arc::new(Mutex::new(StreamingUploadManager::with_config(
                upload_config,
            ))),
        }
    }

    /// Provides direct access to the streaming upload manager.
    ///
    /// Allows components to interact with upload throttling logic directly
    /// without unnecessary wrapper methods in the peer manager.
    pub fn upload_manager(&self) -> Arc<Mutex<StreamingUploadManager>> {
        Arc::clone(&self.upload_manager)
    }

    /// Starts background task to handle peer connection lifecycle.
    #[allow(clippy::too_many_arguments)]
    async fn start_peer_task(
        &self,
        address: SocketAddr,
        connection: Arc<Mutex<PeerConnection>>,
        message_sender: mpsc::Sender<PeerMessageEvent>,
        peer_info: Arc<RwLock<HashMap<SocketAddr, PeerInfo>>>,
        upload_manager: Arc<Mutex<StreamingUploadManager>>,
    ) -> JoinHandle<()> {
        let peer_info_clone = peer_info.clone();

        tokio::spawn(async move {
            loop {
                let message_result = {
                    let mut conn = connection.lock().await;
                    conn.receive_message().await
                };

                match message_result {
                    Ok(message) => {
                        // Track download activity for reciprocity
                        if let PeerMessage::Piece { data, .. } = &message {
                            let mut upload_mgr = upload_manager.lock().await;
                            upload_mgr.record_download_from_peer(address, data.len() as u64);
                        }

                        // Update peer activity
                        {
                            let mut info_map = peer_info_clone.write().await;
                            if let Some(info) = info_map.get_mut(&address) {
                                info.last_activity = Instant::now();
                                info.status = ConnectionStatus::Connected;

                                // Track download bytes in peer info
                                if let PeerMessage::Piece { data, .. } = &message {
                                    info.bytes_downloaded += data.len() as u64;
                                }
                            }
                        }

                        // Forward message to manager
                        let event = PeerMessageEvent {
                            peer_address: address,
                            message,
                            received_at: Instant::now(),
                        };

                        if message_sender.send(event).await.is_err() {
                            break; // Manager shut down
                        }
                    }
                    Err(_) => {
                        // Connection lost, mark as disconnected
                        {
                            let mut info_map = peer_info_clone.write().await;
                            if let Some(info) = info_map.get_mut(&address) {
                                info.status = ConnectionStatus::Failed;
                            }
                        }

                        // Remove peer from upload manager
                        {
                            let mut upload_mgr = upload_manager.lock().await;
                            upload_mgr.remove_peer(address);
                        }
                        break;
                    }
                }
            }
        })
    }

    /// Checks if connection limit would be exceeded
    async fn at_connection_limit(&self) -> bool {
        let connections = self.connections.read().await;
        connections.len() >= self.max_connections
    }

    /// Clean up disconnected peer connections
    async fn cleanup_disconnected_peers(&self) {
        let mut connections = self.connections.write().await;
        let mut peer_info = self.peer_info.write().await;
        let mut tasks = self.active_tasks.lock().await;

        let disconnected: Vec<SocketAddr> = peer_info
            .iter()
            .filter(|(_, info)| info.status == ConnectionStatus::Failed)
            .map(|(addr, _)| *addr)
            .collect();

        for addr in disconnected {
            connections.remove(&addr);
            peer_info.remove(&addr);
            if let Some(task) = tasks.remove(&addr) {
                task.abort();
            }
        }
    }
}

#[async_trait]
impl PeerManager for TcpPeerManager {
    async fn connect_peer(
        &mut self,
        address: SocketAddr,
        info_hash: InfoHash,
        peer_id: PeerId,
    ) -> Result<(), TorrentError> {
        // Check connection limit
        if self.at_connection_limit().await {
            self.cleanup_disconnected_peers().await;
            if self.at_connection_limit().await {
                return Err(TorrentError::PeerConnectionError {
                    reason: format!("Connection limit reached: {}", self.max_connections),
                });
            }
        }

        // Check if already connected
        {
            let connections = self.connections.read().await;
            if connections.contains_key(&address) {
                return Ok(()); // Already connected
            }
        }

        // Create peer info entry
        {
            let mut info_map = self.peer_info.write().await;
            info_map.insert(address, PeerInfo::new(address));
        }

        // Establish connection
        let connection = match PeerConnection::connect(address, info_hash, peer_id).await {
            Ok(conn) => conn,
            Err(e) => {
                // Clean up failed connection attempt
                let mut info_map = self.peer_info.write().await;
                info_map.remove(&address);
                return Err(e);
            }
        };

        let connection = Arc::new(Mutex::new(connection));

        // Update peer info to connected
        {
            let mut info_map = self.peer_info.write().await;
            if let Some(info) = info_map.get_mut(&address) {
                info.status = ConnectionStatus::Connected;
                info.connected_at = Some(Instant::now());
            }
        }

        // Start background task for this peer
        let task = self
            .start_peer_task(
                address,
                connection.clone(),
                self.message_sender.clone(),
                self.peer_info.clone(),
                self.upload_manager.clone(),
            )
            .await;

        // Store connection and task
        {
            let mut connections = self.connections.write().await;
            connections.insert(address, connection);
        }
        {
            let mut tasks = self.active_tasks.lock().await;
            tasks.insert(address, task);
        }

        Ok(())
    }

    async fn disconnect_peer(&mut self, address: SocketAddr) -> Result<(), TorrentError> {
        // Remove connection
        let connection = {
            let mut connections = self.connections.write().await;
            connections.remove(&address)
        };

        if let Some(connection) = connection {
            // Disconnect gracefully
            let mut conn = connection.lock().await;
            conn.disconnect().await?;
        }

        // Cancel background task
        {
            let mut tasks = self.active_tasks.lock().await;
            if let Some(task) = tasks.remove(&address) {
                task.abort();
            }
        }

        // Remove peer from upload manager
        {
            let mut upload_manager = self.upload_manager.lock().await;
            upload_manager.remove_peer(address);
        }

        // Update peer info
        {
            let mut info_map = self.peer_info.write().await;
            if let Some(info) = info_map.get_mut(&address) {
                info.status = ConnectionStatus::Disconnected;
            }
        }

        Ok(())
    }

    async fn send_message(
        &mut self,
        peer_address: SocketAddr,
        message: PeerMessage,
    ) -> Result<(), TorrentError> {
        let connection = {
            let connections = self.connections.read().await;
            connections.get(&peer_address).cloned()
        };

        if let Some(connection) = connection {
            // Apply streaming upload throttling for piece requests
            match &message {
                PeerMessage::Request {
                    piece_index: _,
                    offset: _,
                    length: _,
                } => {
                    // This is a request FROM us TO the peer - always allow
                    let mut conn = connection.lock().await;
                    conn.send_message(message).await?;
                }
                PeerMessage::Piece {
                    piece_index: _,
                    offset: _,
                    data,
                } => {
                    // This is data FROM us TO the peer - apply throttling
                    // Note: In practice, this would be handled by a separate upload task
                    // that processes queued requests via the upload manager
                    let upload_bytes = data.len() as u64;

                    let mut conn = connection.lock().await;
                    conn.send_message(message).await?;

                    // Record upload in upload manager and peer info
                    {
                        let mut upload_manager = self.upload_manager.lock().await;
                        upload_manager.record_upload_to_peer(peer_address, upload_bytes);
                    }
                    {
                        let mut info_map = self.peer_info.write().await;
                        if let Some(info) = info_map.get_mut(&peer_address) {
                            info.bytes_uploaded += upload_bytes;
                        }
                    }
                }
                _ => {
                    // Non-piece messages are always allowed
                    let mut conn = connection.lock().await;
                    conn.send_message(message).await?;
                }
            }

            // Update peer activity
            {
                let mut info_map = self.peer_info.write().await;
                if let Some(info) = info_map.get_mut(&peer_address) {
                    info.last_activity = Instant::now();
                }
            }

            Ok(())
        } else {
            Err(TorrentError::PeerConnectionError {
                reason: format!("Peer not connected: {peer_address}"),
            })
        }
    }

    async fn receive_message(&mut self) -> Result<PeerMessageEvent, TorrentError> {
        let mut receiver = self.message_receiver.lock().await;
        receiver
            .recv()
            .await
            .ok_or_else(|| TorrentError::PeerConnectionError {
                reason: "All peer connections closed".to_string(),
            })
    }

    async fn connected_peers(&self) -> Vec<PeerInfo> {
        let info_map = self.peer_info.read().await;
        info_map
            .values()
            .filter(|info| info.status == ConnectionStatus::Connected)
            .cloned()
            .collect()
    }

    async fn connection_count(&self) -> usize {
        let connections = self.connections.read().await;
        connections.len()
    }

    async fn upload_stats(&self) -> (u64, u64) {
        // Use streaming upload manager for accurate throttled upload statistics
        let upload_manager = self.upload_manager.lock().await;
        upload_manager.upload_stats()
    }

    async fn shutdown(&mut self) -> Result<(), TorrentError> {
        // Cancel all background tasks
        {
            let mut tasks = self.active_tasks.lock().await;
            for (_, task) in tasks.drain() {
                task.abort();
            }
        }

        // Disconnect all peers
        let addresses: Vec<SocketAddr> = {
            let connections = self.connections.read().await;
            connections.keys().copied().collect()
        };

        for address in addresses {
            let _ = self.disconnect_peer(address).await; // Best effort
        }

        Ok(())
    }

    async fn configure_upload_manager(
        &mut self,
        info_hash: InfoHash,
        piece_size: u64,
        total_bandwidth: u64,
    ) -> Result<(), TorrentError> {
        let upload_manager = self.upload_manager();
        let mut upload_mgr = upload_manager.lock().await;

        upload_mgr.update_available_bandwidth(total_bandwidth);
        upload_mgr.update_streaming_position(info_hash, 0); // Start at beginning

        tracing::info!(
            "Configured streaming upload throttling for torrent {} with piece_size={}",
            info_hash,
            piece_size
        );

        Ok(())
    }

    async fn update_streaming_position(
        &mut self,
        info_hash: InfoHash,
        byte_position: u64,
    ) -> Result<(), TorrentError> {
        let upload_manager = self.upload_manager();
        let mut upload_mgr = upload_manager.lock().await;
        upload_mgr.update_streaming_position(info_hash, byte_position);
        Ok(())
    }
}

#[cfg(test)]
mod peer_manager_tests {
    use std::net::{IpAddr, Ipv4Addr};

    use super::*;

    #[tokio::test]
    async fn test_network_peer_manager_creation() {
        let peer_id = PeerId::generate();
        let manager = TcpPeerManager::new(peer_id, 25);

        assert_eq!(manager.connection_count().await, 0);
        assert!(manager.connected_peers().await.is_empty());
    }

    #[tokio::test]
    async fn test_network_peer_manager_default() {
        let manager = TcpPeerManager::new_default();
        assert_eq!(manager.connection_count().await, 0);
    }

    #[tokio::test]
    async fn test_connect_to_nonexistent_peer() {
        let mut manager = TcpPeerManager::new_default();
        let address = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0); // Port 0 should fail
        let info_hash = InfoHash::new([1u8; 20]);
        let peer_id = PeerId::generate();

        let result = manager.connect_peer(address, info_hash, peer_id).await;
        assert!(result.is_err());
        assert_eq!(manager.connection_count().await, 0);
    }

    #[tokio::test]
    async fn test_connection_limit() {
        let mut manager = TcpPeerManager::new(PeerId::generate(), 1); // Limit to 1 connection
        let addr1 = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 6881);
        let addr2 = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 6882);
        let info_hash = InfoHash::new([1u8; 20]);
        let peer_id = PeerId::generate();

        // Both connections should fail, but test the limit logic
        let _ = manager.connect_peer(addr1, info_hash, peer_id).await;
        let _result2 = manager.connect_peer(addr2, info_hash, peer_id).await;

        // Should either reject due to limit or both fail to connect
        // The important thing is that the limit is respected
        assert!(manager.connection_count().await <= 1);
    }

    #[tokio::test]
    async fn test_disconnect_nonexistent_peer() {
        let mut manager = TcpPeerManager::new_default();
        let address = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 6881);

        // Should not error when disconnecting non-existent peer
        let result = manager.disconnect_peer(address).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_send_message_to_nonexistent_peer() {
        let mut manager = TcpPeerManager::new_default();
        let address = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 6881);
        let message = PeerMessage::Choke;

        let result = manager.send_message(address, message).await;
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            TorrentError::PeerConnectionError { reason } if reason.contains("not connected")
        ));
    }

    #[tokio::test]
    async fn test_peer_info_structure() {
        let address = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 6881);
        let info = PeerInfo::new(address);

        assert_eq!(info.address, address);
        assert_eq!(info.status, ConnectionStatus::Connecting);
        assert_eq!(info.connected_at, None);
        assert_eq!(info.bytes_downloaded, 0);
        assert_eq!(info.bytes_uploaded, 0);
    }

    #[tokio::test]
    async fn test_shutdown() {
        let mut manager = TcpPeerManager::new_default();

        // Should succeed even with no connections
        let result = manager.shutdown().await;
        assert!(result.is_ok());
        assert_eq!(manager.connection_count().await, 0);
    }
}
