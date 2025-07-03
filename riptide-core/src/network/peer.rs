//! Peer connection abstraction for production and simulation

use std::net::SocketAddr;
use std::time::Duration;

use async_trait::async_trait;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

use crate::torrent::protocol::handshake::HandshakeCodec;
use crate::torrent::protocol::types::{PeerHandshake, PeerId};
use crate::torrent::{InfoHash, TorrentError};

/// Represents a connected peer
#[derive(Debug, Clone)]
pub struct PeerConnection {
    pub address: SocketAddr,
    pub peer_id: Option<PeerId>,
    pub is_choked: bool,
    pub is_interested: bool,
    pub bytes_downloaded: u64,
    pub bytes_uploaded: u64,
}

/// Peer layer abstraction for BitTorrent connections
///
/// Enables both real TCP connections and simulation environments
/// to use the same peer management logic.
#[async_trait]
pub trait PeerLayer: Send + Sync {
    /// Connects to a peer and performs BitTorrent handshake
    ///
    /// # Errors
    /// - `TorrentError::PeerConnectionError` - Connection failed
    async fn connect_peer(
        &mut self,
        address: SocketAddr,
        _info_hash: InfoHash,
        _our_peer_id: PeerId,
    ) -> Result<(), TorrentError>;

    /// Disconnects from a peer
    async fn disconnect_peer(&mut self, address: SocketAddr) -> Result<(), TorrentError>;

    /// Returns all connected peers
    async fn connected_peers(&self) -> Vec<PeerConnection>;

    /// Returns number of active connections
    async fn connection_count(&self) -> usize;

    /// Sets maximum number of concurrent connections
    fn set_max_connections(&mut self, max: usize);
}

/// Production peer layer using real TCP connections
pub struct ProductionPeerLayer {
    max_connections: usize,
    connections: Vec<PeerConnection>,
    timeout: Duration,
}

impl ProductionPeerLayer {
    pub fn new(max_connections: usize) -> Self {
        Self {
            max_connections,
            connections: Vec::new(),
            timeout: Duration::from_secs(10),
        }
    }

    /// Performs BitTorrent handshake according to BEP 3
    ///
    /// # Errors
    /// - `TorrentError::PeerConnectionError` - Handshake failed or protocol error
    async fn perform_handshake(
        &self,
        stream: &mut tokio::net::TcpStream,
        info_hash: InfoHash,
        our_peer_id: PeerId,
    ) -> Result<PeerId, TorrentError> {
        // Create our handshake message
        let our_handshake = PeerHandshake {
            protocol: "BitTorrent protocol".to_string(),
            reserved: [0u8; 8], // No extensions for now
            info_hash,
            peer_id: our_peer_id,
        };

        // Serialize and send our handshake
        let handshake_bytes = HandshakeCodec::serialize_handshake(&our_handshake);
        stream.write_all(&handshake_bytes).await.map_err(|e| {
            TorrentError::PeerConnectionError {
                reason: format!("Failed to send handshake: {e}"),
            }
        })?;

        // Read peer's handshake response
        let mut response_buffer = vec![0u8; 68]; // Standard handshake length
        stream.read_exact(&mut response_buffer).await.map_err(|e| {
            TorrentError::PeerConnectionError {
                reason: format!("Failed to read handshake response: {e}"),
            }
        })?;

        // Parse peer handshake
        let peer_handshake = HandshakeCodec::deserialize_handshake(&response_buffer)?;

        // Validate handshake
        if peer_handshake.protocol != "BitTorrent protocol" {
            return Err(TorrentError::PeerConnectionError {
                reason: format!("Invalid protocol: {}", peer_handshake.protocol),
            });
        }

        if peer_handshake.info_hash != info_hash {
            return Err(TorrentError::PeerConnectionError {
                reason: "Info hash mismatch in handshake".to_string(),
            });
        }

        tracing::debug!(
            "Handshake successful with peer ID: {:?}",
            peer_handshake.peer_id
        );

        Ok(peer_handshake.peer_id)
    }
}

#[async_trait]
impl PeerLayer for ProductionPeerLayer {
    async fn connect_peer(
        &mut self,
        address: SocketAddr,
        info_hash: InfoHash,
        our_peer_id: PeerId,
    ) -> Result<(), TorrentError> {
        if self.connections.len() >= self.max_connections {
            return Err(TorrentError::PeerConnectionError {
                reason: "Maximum connections reached".to_string(),
            });
        }

        // Establish TCP connection with timeout
        let mut stream =
            match tokio::time::timeout(self.timeout, tokio::net::TcpStream::connect(address)).await
            {
                Ok(Ok(stream)) => stream,
                Ok(Err(e)) => {
                    return Err(TorrentError::PeerConnectionError {
                        reason: format!("Failed to connect to {address}: {e}"),
                    });
                }
                Err(_) => {
                    return Err(TorrentError::PeerConnectionError {
                        reason: format!("Connection to {address} timed out"),
                    });
                }
            };

        // Perform BitTorrent handshake
        let peer_id = self
            .perform_handshake(&mut stream, info_hash, our_peer_id)
            .await?;

        // Create peer connection with handshake results
        let peer = PeerConnection {
            address,
            peer_id: Some(peer_id),
            is_choked: true,      // Peers start choked
            is_interested: false, // We start not interested
            bytes_downloaded: 0,
            bytes_uploaded: 0,
        };

        self.connections.push(peer);
        tracing::info!("Successfully connected to peer {address} with handshake");
        Ok(())
    }

    async fn disconnect_peer(&mut self, address: SocketAddr) -> Result<(), TorrentError> {
        self.connections.retain(|p| p.address != address);
        Ok(())
    }

    async fn connected_peers(&self) -> Vec<PeerConnection> {
        self.connections.clone()
    }

    async fn connection_count(&self) -> usize {
        self.connections.len()
    }

    fn set_max_connections(&mut self, max: usize) {
        self.max_connections = max;
    }
}

/// Simulation peer layer for deterministic testing
pub struct SimulationPeerLayer {
    max_connections: usize,
    connections: Vec<PeerConnection>,
    success_rate: f32,
}

impl SimulationPeerLayer {
    pub fn new(max_connections: usize) -> Self {
        Self {
            max_connections,
            connections: Vec::new(),
            success_rate: 0.8, // 80% connection success rate by default
        }
    }

    /// Sets the connection success rate for simulation (0.0 to 1.0).
    pub fn set_success_rate(&mut self, rate: f32) {
        self.success_rate = rate.clamp(0.0, 1.0);
    }
}

#[async_trait]
impl PeerLayer for SimulationPeerLayer {
    async fn connect_peer(
        &mut self,
        address: SocketAddr,
        _info_hash: InfoHash,
        _our_peer_id: PeerId,
    ) -> Result<(), TorrentError> {
        if self.connections.len() >= self.max_connections {
            return Err(TorrentError::PeerConnectionError {
                reason: "Maximum connections reached".to_string(),
            });
        }

        // Simulate connection success/failure
        if rand::random::<f32>() > self.success_rate {
            return Err(TorrentError::PeerConnectionError {
                reason: format!("Simulated connection failure to {address}"),
            });
        }

        // Add simulated connection
        let peer = PeerConnection {
            address,
            peer_id: Some(PeerId::generate()),
            is_choked: false, // Start unchoked in simulation
            is_interested: true,
            bytes_downloaded: 0,
            bytes_uploaded: 0,
        };

        self.connections.push(peer);
        tracing::info!("Simulation: Connected to peer {}", address);
        Ok(())
    }

    async fn disconnect_peer(&mut self, address: SocketAddr) -> Result<(), TorrentError> {
        self.connections.retain(|p| p.address != address);
        Ok(())
    }

    async fn connected_peers(&self) -> Vec<PeerConnection> {
        self.connections.clone()
    }

    async fn connection_count(&self) -> usize {
        self.connections.len()
    }

    fn set_max_connections(&mut self, max: usize) {
        self.max_connections = max;
    }
}
