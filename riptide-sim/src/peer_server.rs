//! BitTorrent peer server for development mode simulation
//!
//! Spawns actual TCP servers that respond to BitTorrent wire protocol requests
//! using piece data from the InMemoryPieceStore. This allows the production
//! BitTorrent client to connect and download normally.

use std::net::SocketAddr;
use std::sync::Arc;

use riptide_core::torrent::{InfoHash, PieceIndex, PieceStore};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

/// BitTorrent peer server that serves pieces from a PieceStore
pub struct BitTorrentPeerServer<P: PieceStore> {
    /// Info hash of the torrent being served
    info_hash: InfoHash,
    /// Storage backend containing piece data
    piece_store: Arc<P>,
    /// Network address to bind the server socket
    listen_address: SocketAddr,
}

impl<P: PieceStore + Send + Sync + 'static> BitTorrentPeerServer<P> {
    /// Creates a new peer server for the given torrent
    pub fn new(info_hash: InfoHash, piece_store: Arc<P>, listen_address: SocketAddr) -> Self {
        Self {
            info_hash,
            piece_store,
            listen_address,
        }
    }

    /// Starts the peer server and returns immediately
    ///
    /// The server runs in a background task and serves pieces to connecting peers
    ///
    /// # Errors
    ///
    /// Returns error if TCP listener cannot bind to the specified address
    pub async fn start(self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let listener = TcpListener::bind(self.listen_address).await?;
        tracing::info!(
            "Started BitTorrent peer server for {} on {}",
            self.info_hash,
            self.listen_address
        );

        tokio::spawn(async move {
            loop {
                match listener.accept().await {
                    Ok((stream, peer_addr)) => {
                        tracing::debug!("Accepted connection from {}", peer_addr);
                        let server = BitTorrentPeerServer {
                            info_hash: self.info_hash,
                            piece_store: self.piece_store.clone(),
                            listen_address: self.listen_address,
                        };

                        tokio::spawn(async move {
                            if let Err(e) = server.handle_connection(stream, peer_addr).await {
                                tracing::debug!("Connection from {} ended: {}", peer_addr, e);
                            }
                        });
                    }
                    Err(e) => {
                        tracing::error!("Failed to accept connection: {}", e);
                        break;
                    }
                }
            }
        });

        Ok(())
    }

    /// Handles a single peer connection with full BitTorrent wire protocol
    ///
    /// # Errors
    ///
    /// Returns error if I/O operations fail or protocol violations occur
    async fn handle_connection(
        &self,
        mut stream: TcpStream,
        peer_addr: SocketAddr,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        tracing::debug!("Handling connection from {}", peer_addr);

        // A simplified handshake for simulation. A production implementation would also negotiate protocol extensions (BEP_0010) and handle encryption.
        let mut handshake_buffer = [0u8; 68];
        stream.read_exact(&mut handshake_buffer).await?;

        if &handshake_buffer[1..20] != b"BitTorrent protocol" {
            tracing::warn!("Invalid handshake from {}", peer_addr);
            return Err("Invalid handshake".into());
        }

        // Per BEP_0003, the info_hash is located at bytes 28-48 of the handshake.
        let received_info_hash = InfoHash::new(<[u8; 20]>::try_from(&handshake_buffer[28..48])?);
        if received_info_hash != self.info_hash {
            tracing::warn!(
                "Wrong info_hash from {}: got {}, expected {}",
                peer_addr,
                received_info_hash,
                self.info_hash
            );
            return Err("Wrong info_hash".into());
        }

        let mut response = vec![19u8];
        response.extend_from_slice(b"BitTorrent protocol");
        response.extend_from_slice(&[0u8; 8]);
        response.extend_from_slice(self.info_hash.as_bytes());
        response.extend_from_slice(&[0u8; 20]);
        stream.write_all(&response).await?;

        tracing::debug!("Handshake completed with {}", peer_addr);

        if let Ok(piece_count) = self.piece_store.piece_count(self.info_hash) {
            self.send_bitfield(&mut stream, piece_count).await?;
        }

        self.send_unchoke(&mut stream).await?;
        loop {
            match self.read_message(&mut stream).await {
                Ok(Some(message)) => match message {
                    PeerMessage::Request {
                        piece_index,
                        offset,
                        length,
                    } => {
                        self.handle_piece_request(&mut stream, piece_index, offset, length)
                            .await?;
                    }
                    PeerMessage::Interested => {
                        tracing::debug!("Peer {} is interested", peer_addr);
                    }
                    PeerMessage::KeepAlive => {
                        tracing::debug!("Keep-alive from {}", peer_addr);
                    }
                    _ => {
                        tracing::debug!("Received message from {}: {:?}", peer_addr, message);
                    }
                },
                Ok(None) => {
                    tracing::debug!("Connection closed by peer {}", peer_addr);
                    break;
                }
                Err(e) => {
                    tracing::debug!("Error reading message from {}: {}", peer_addr, e);
                    break;
                }
            }
        }

        Ok(())
    }

    /// Sends bitfield message indicating which pieces we have
    async fn send_bitfield(
        &self,
        stream: &mut TcpStream,
        piece_count: u32,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let bitfield_bytes = piece_count.div_ceil(8);
        let mut bitfield = vec![0xFFu8; bitfield_bytes as usize];

        // The BitTorrent protocol requires that any unused bits at the end of the bitfield are set to zero.
        let unused_bits = (8 - (piece_count % 8)) % 8;
        if unused_bits > 0 && !bitfield.is_empty() {
            let last_index = bitfield.len() - 1;
            bitfield[last_index] &= 0xFF << unused_bits;
        }

        let message_length = (1 + bitfield.len()) as u32;
        stream.write_all(&message_length.to_be_bytes()).await?;
        stream.write_all(&[5u8]).await?;
        stream.write_all(&bitfield).await?;

        tracing::debug!("Sent bitfield with {} pieces", piece_count);
        Ok(())
    }

    /// Sends unchoke message
    async fn send_unchoke(
        &self,
        stream: &mut TcpStream,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let message_length = 1u32;
        stream.write_all(&message_length.to_be_bytes()).await?;
        stream.write_all(&[1u8]).await?;
        tracing::debug!("Sent unchoke message");
        Ok(())
    }

    /// Reads a peer message from the stream
    async fn read_message(
        &self,
        stream: &mut TcpStream,
    ) -> Result<Option<PeerMessage>, Box<dyn std::error::Error + Send + Sync>> {
        let mut length_buffer = [0u8; 4];
        match stream.read_exact(&mut length_buffer).await {
            Ok(_) => {}
            Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => return Ok(None),
            Err(e) => return Err(e.into()),
        }

        let message_length = u32::from_be_bytes(length_buffer);

        if message_length == 0 {
            return Ok(Some(PeerMessage::KeepAlive));
        }

        let mut id_buffer = [0u8; 1];
        stream.read_exact(&mut id_buffer).await?;
        let message_id = id_buffer[0];

        match message_id {
            0 => Ok(Some(PeerMessage::Choke)),
            1 => Ok(Some(PeerMessage::Unchoke)),
            2 => Ok(Some(PeerMessage::Interested)),
            3 => Ok(Some(PeerMessage::NotInterested)),
            6 => {
                let mut request_buffer = [0u8; 12];
                stream.read_exact(&mut request_buffer).await?;

                let piece_index = PieceIndex::new(u32::from_be_bytes([
                    request_buffer[0],
                    request_buffer[1],
                    request_buffer[2],
                    request_buffer[3],
                ]));
                let offset = u32::from_be_bytes([
                    request_buffer[4],
                    request_buffer[5],
                    request_buffer[6],
                    request_buffer[7],
                ]);
                let length = u32::from_be_bytes([
                    request_buffer[8],
                    request_buffer[9],
                    request_buffer[10],
                    request_buffer[11],
                ]);

                Ok(Some(PeerMessage::Request {
                    piece_index,
                    offset,
                    length,
                }))
            }
            _ => {
                let payload_length = message_length - 1;
                let mut payload = vec![0u8; payload_length as usize];
                stream.read_exact(&mut payload).await?;
                Ok(Some(PeerMessage::Unknown))
            }
        }
    }

    /// Handles a piece request by sending the requested piece data
    async fn handle_piece_request(
        &self,
        stream: &mut TcpStream,
        piece_index: PieceIndex,
        offset: u32,
        length: u32,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        tracing::debug!(
            "Handling piece request: piece={}, offset={}, length={}",
            piece_index,
            offset,
            length
        );

        match self
            .piece_store
            .piece_data(self.info_hash, piece_index)
            .await
        {
            Ok(piece_data) => {
                let start = offset as usize;
                let end = std::cmp::min(start + length as usize, piece_data.len());

                if start >= piece_data.len() {
                    tracing::warn!(
                        "Invalid piece request: offset {} >= piece length {}",
                        start,
                        piece_data.len()
                    );
                    return Ok(());
                }

                let block_data = &piece_data[start..end];

                let message_length = (9 + block_data.len()) as u32;
                stream.write_all(&message_length.to_be_bytes()).await?;
                stream.write_all(&[7u8]).await?;
                stream
                    .write_all(&piece_index.as_u32().to_be_bytes())
                    .await?;
                stream.write_all(&offset.to_be_bytes()).await?;
                stream.write_all(block_data).await?;

                tracing::debug!(
                    "Sent piece block: piece={}, offset={}, length={}",
                    piece_index,
                    offset,
                    length
                );
            }
            Err(e) => {
                tracing::warn!("Failed to get piece data for piece {}: {}", piece_index, e);
            }
        }

        Ok(())
    }
}

/// Simple peer message enum for the peer server
#[derive(Debug)]
enum PeerMessage {
    KeepAlive,
    Choke,
    Unchoke,
    Interested,
    NotInterested,
    Request {
        piece_index: PieceIndex,
        offset: u32,
        length: u32,
    },
    Unknown,
}

/// Spawns multiple peer servers for a torrent on different ports
///
/// # Errors
///
/// - `Box<dyn std::error::Error + Send + Sync>` - If failed to bind socket or start server
pub async fn spawn_peer_servers_for_torrent<P: PieceStore + Send + Sync + 'static>(
    info_hash: InfoHash,
    piece_store: Arc<P>,
    peer_count: usize,
) -> Result<Vec<SocketAddr>, Box<dyn std::error::Error + Send + Sync>> {
    let mut peer_addresses = Vec::new();
    let base_port = 8881;

    for i in 0..peer_count {
        let port = base_port + i as u16;
        let address = SocketAddr::new([127, 0, 0, 1].into(), port);

        let server = BitTorrentPeerServer::new(info_hash, piece_store.clone(), address);
        server.start().await?;

        peer_addresses.push(address);
        tracing::info!(
            "Spawned peer server {} for torrent {} on {}",
            i + 1,
            info_hash,
            address
        );
    }

    Ok(peer_addresses)
}
