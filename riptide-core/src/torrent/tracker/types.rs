//! Core types and enumerations for BitTorrent tracker communication

use std::net::SocketAddr;

use async_trait::async_trait;

use crate::torrent::{InfoHash, TorrentError};

// Type aliases for complex types
pub(super) type PeerList = Result<Vec<SocketAddr>, TorrentError>;

/// Tracker announce request.
///
/// Contains client statistics and torrent information sent to tracker
/// during announce operations to report progress and request peer list.
#[derive(Debug, Clone)]
pub struct AnnounceRequest {
    /// Unique identifier for the torrent being announced
    pub info_hash: InfoHash,
    /// Client's unique 20-byte identifier
    pub peer_id: [u8; 20],
    /// TCP port client is listening on for peer connections
    pub port: u16,
    /// Total bytes uploaded to other peers
    pub uploaded: u64,
    /// Total bytes downloaded from other peers
    pub downloaded: u64,
    /// Bytes remaining to download (0 for seeders)
    pub left: u64,
    /// Current client state for this torrent
    pub event: AnnounceEvent,
}

/// BitTorrent announce events.
///
/// Indicates client state changes that should be reported to tracker
/// for proper swarm management and statistics tracking.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnnounceEvent {
    /// Client started downloading this torrent
    Started,
    /// Client stopped downloading this torrent
    Stopped,
    /// Client completed downloading this torrent
    Completed,
}

/// Tracker announce response.
///
/// Contains peer list and swarm statistics returned by tracker
/// in response to announce requests.
#[derive(Debug, Clone)]
pub struct AnnounceResponse {
    /// Seconds until next announce request should be sent
    pub interval: u32,
    /// Minimum allowed interval between announces
    pub min_interval: Option<u32>,
    /// Tracker-specific identifier for subsequent requests
    pub tracker_id: Option<String>,
    /// Number of seeders in the swarm
    pub complete: u32,
    /// Number of leechers in the swarm
    pub incomplete: u32,
    /// List of peer addresses for connection attempts
    pub peers: Vec<SocketAddr>,
}

/// Tracker scrape request.
///
/// Requests statistics for multiple torrents without announcing
/// client presence or affecting swarm participation.
#[derive(Debug, Clone)]
pub struct ScrapeRequest {
    /// List of torrent info hashes to query statistics for
    pub info_hashes: Vec<InfoHash>,
}

/// Individual torrent statistics from scrape.
///
/// Contains seeder/leecher counts and download statistics
/// for a single torrent from scrape response.
#[derive(Debug, Clone)]
pub struct ScrapeStats {
    /// Number of seeders (peers with complete file)
    pub complete: u32,
    /// Total number of completed downloads
    pub downloaded: u32,
    /// Number of leechers (peers downloading)
    pub incomplete: u32,
}

/// Tracker scrape response.
///
/// Contains statistics for all requested torrents indexed by info hash.
/// Used for monitoring swarm health without participating.
#[derive(Debug, Clone)]
pub struct ScrapeResponse {
    /// Statistics for each torrent indexed by info hash
    pub files: std::collections::HashMap<InfoHash, ScrapeStats>,
}

/// Abstract tracker communication interface for BitTorrent trackers.
///
/// Provides announce and scrape operations following BEP 3 specification.
/// Implementations handle protocol-specific details (HTTP/UDP) while maintaining
/// consistent error handling and response parsing.
#[async_trait]
pub trait TrackerClient: Send + Sync {
    /// Announces client presence to tracker and retrieves peer list.
    ///
    /// Sends torrent statistics and receives updated peer information.
    /// Tracker may respond with different peer lists based on client state.
    ///
    /// # Errors
    ///
    /// - `TorrentError::TrackerConnectionFailed` - If network or protocol error
    /// - `TorrentError::ProtocolError` - If invalid tracker response format
    async fn announce(&self, request: AnnounceRequest) -> Result<AnnounceResponse, TorrentError>;

    /// Retrieves torrent statistics from tracker without announcing.
    ///
    /// Queries tracker for seeder/leecher counts and download statistics
    /// without updating client's announced state.
    ///
    /// # Errors
    ///
    /// - `TorrentError::TrackerConnectionFailed` - If network or protocol error
    /// - `TorrentError::ProtocolError` - If invalid scrape response format
    async fn scrape(&self, request: ScrapeRequest) -> Result<ScrapeResponse, TorrentError>;

    /// Returns tracker URL for debugging and logging purposes.
    fn tracker_url(&self) -> &str;
}
