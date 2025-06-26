//! Mock tracker types and error definitions

use std::net::SocketAddr;
use std::time::SystemTime;

use riptide_core::torrent::InfoHash;

/// Mock torrent data for simulation.
#[derive(Debug, Clone)]
pub struct MockTorrentData {
    pub info_hash: InfoHash,
    pub name: String,
    pub size: u64,
    pub seeders: u32,
    pub leechers: u32,
    pub last_announce: SystemTime,
}

/// Response from tracker announce request.
#[derive(Debug, Clone)]
pub struct AnnounceResponse {
    pub interval: u32,                // Time between announces in seconds
    pub seeders: u32,                 // Number of seeders
    pub leechers: u32,                // Number of leechers
    pub peers: Vec<SocketAddr>,       // List of peer addresses
    pub complete: u32,                // Total completed downloads
    pub incomplete: u32,              // Total incomplete downloads
    pub downloaded: u32,              // Total downloads from this tracker
    pub torrent_name: Option<String>, // Human-readable torrent name
    pub torrent_size: Option<u64>,    // Total torrent size in bytes
}

/// Errors that can occur during tracker simulation.
#[derive(Debug, thiserror::Error)]
pub enum TrackerError {
    #[error("Connection to tracker failed")]
    ConnectionFailed,

    #[error("Torrent not found")]
    TorrentNotFound,

    #[error("Magneto provider not enabled")]
    MagnetoNotEnabled,

    #[error("Search operation failed")]
    SearchFailed(#[from] magneto::ClientError),

    #[error("Invalid tracker response: {message}")]
    InvalidResponse { message: String },
}
