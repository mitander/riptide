//! Core types and enumerations for BitTorrent tracker communication

use std::net::SocketAddr;

use async_trait::async_trait;

use super::super::{InfoHash, TorrentError};

// Type aliases for complex types
pub(super) type PeerList = Result<Vec<SocketAddr>, TorrentError>;

/// Tracker announce request.
///
/// Contains client statistics and torrent information sent to tracker
/// during announce operations to report progress and request peer list.
#[derive(Debug, Clone)]
pub struct AnnounceRequest {
    pub info_hash: InfoHash,
    pub peer_id: [u8; 20],
    pub port: u16,
    pub uploaded: u64,
    pub downloaded: u64,
    pub left: u64,
    pub event: AnnounceEvent,
}

/// BitTorrent announce events.
///
/// Indicates client state changes that should be reported to tracker
/// for proper swarm management and statistics tracking.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnnounceEvent {
    Started,
    Stopped,
    Completed,
}

/// Tracker announce response.
///
/// Contains peer list and swarm statistics returned by tracker
/// in response to announce requests.
#[derive(Debug, Clone)]
pub struct AnnounceResponse {
    pub interval: u32,
    pub min_interval: Option<u32>,
    pub tracker_id: Option<String>,
    pub complete: u32,
    pub incomplete: u32,
    pub peers: Vec<SocketAddr>,
}

/// Tracker scrape request.
///
/// Requests statistics for multiple torrents without announcing
/// client presence or affecting swarm participation.
#[derive(Debug, Clone)]
pub struct ScrapeRequest {
    pub info_hashes: Vec<InfoHash>,
}

/// Individual torrent statistics from scrape.
///
/// Contains seeder/leecher counts and download statistics
/// for a single torrent from scrape response.
#[derive(Debug, Clone)]
pub struct ScrapeStats {
    pub complete: u32,
    pub downloaded: u32,
    pub incomplete: u32,
}

/// Tracker scrape response.
///
/// Contains statistics for all requested torrents indexed by info hash.
/// Used for monitoring swarm health without participating.
#[derive(Debug, Clone)]
pub struct ScrapeResponse {
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
    /// - `TorrentError::TrackerConnectionFailed` - Network or protocol error
    /// - `TorrentError::ProtocolError` - Invalid tracker response format
    async fn announce(&self, request: AnnounceRequest) -> Result<AnnounceResponse, TorrentError>;

    /// Retrieves torrent statistics from tracker without announcing.
    ///
    /// Queries tracker for seeder/leecher counts and download statistics
    /// without updating client's announced state.
    ///
    /// # Errors
    /// - `TorrentError::TrackerConnectionFailed` - Network or protocol error
    /// - `TorrentError::ProtocolError` - Invalid scrape response format
    async fn scrape(&self, request: ScrapeRequest) -> Result<ScrapeResponse, TorrentError>;

    /// Returns tracker URL for debugging and logging purposes.
    fn tracker_url(&self) -> &str;
}