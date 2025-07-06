//! Command definitions for the torrent engine actor model.

use tokio::sync::oneshot;

use crate::torrent::parsing::types::TorrentMetadata;
use crate::torrent::{InfoHash, TorrentError};

/// Commands that can be sent to the torrent engine actor.
///
/// Each command encapsulates an operation request along with a response channel
/// for the actor to send back results. This message-passing approach eliminates
/// the need for shared state locks and prevents deadlocks.
pub enum TorrentEngineCommand {
    /// Add a torrent from a magnet link.
    AddMagnet {
        magnet_link: String,
        responder: oneshot::Sender<Result<InfoHash, TorrentError>>,
    },
    /// Start downloading a torrent by its info hash.
    StartDownload {
        info_hash: InfoHash,
        responder: oneshot::Sender<Result<(), TorrentError>>,
    },
    /// Get the session information for a specific torrent.
    GetSession {
        info_hash: InfoHash,
        responder: oneshot::Sender<Result<TorrentSession, TorrentError>>,
    },
    /// Get all active torrent sessions.
    GetActiveSessions {
        responder: oneshot::Sender<Vec<TorrentSession>>,
    },
    /// Get download statistics for the engine.
    GetDownloadStats {
        responder: oneshot::Sender<EngineStats>,
    },
    /// Mark specific pieces as completed for a torrent.
    MarkPiecesCompleted {
        info_hash: InfoHash,
        piece_indices: Vec<u32>,
        responder: oneshot::Sender<Result<(), TorrentError>>,
    },
    /// Add torrent metadata directly to the engine.
    AddTorrentMetadata {
        metadata: TorrentMetadata,
        responder: oneshot::Sender<Result<InfoHash, TorrentError>>,
    },
    /// Shutdown the engine actor gracefully.
    Shutdown { responder: oneshot::Sender<()> },
    /// Internal notification when a piece is completed (simulation only).
    PieceCompleted {
        info_hash: InfoHash,
        piece_index: u32,
    },
    /// Update download statistics for a torrent.
    UpdateDownloadStats {
        info_hash: InfoHash,
        stats: DownloadStats,
    },
    /// Request prioritization of pieces around a seek position for streaming.
    SeekToPosition {
        info_hash: InfoHash,
        byte_position: u64,
        buffer_size: u64,
        responder: oneshot::Sender<Result<(), TorrentError>>,
    },
    /// Update buffer strategy for adaptive piece picking.
    UpdateBufferStrategy {
        info_hash: InfoHash,
        playback_speed: f64,
        available_bandwidth: u64,
        responder: oneshot::Sender<Result<(), TorrentError>>,
    },
    /// Get current buffer status for a torrent.
    GetBufferStatus {
        info_hash: InfoHash,
        responder: oneshot::Sender<Result<crate::torrent::BufferStatus, TorrentError>>,
    },
    /// Stop downloading a torrent by its info hash.
    StopDownload {
        info_hash: InfoHash,
        responder: oneshot::Sender<Result<(), TorrentError>>,
    },
    /// Configure upload manager for streaming optimization.
    ConfigureUploadManager {
        info_hash: InfoHash,
        piece_size: u64,
        total_bandwidth: u64,
        responder: oneshot::Sender<Result<(), TorrentError>>,
    },
    /// Update streaming position for upload throttling.
    UpdateStreamingPosition {
        info_hash: InfoHash,
        byte_position: u64,
        responder: oneshot::Sender<Result<(), TorrentError>>,
    },
}

/// Active download session for a single torrent.
///
/// Tracks download progress, piece completion status, and session metadata
/// for an individual torrent being downloaded by the engine.
#[derive(Debug, Clone)]
pub struct TorrentSession {
    /// Torrent info hash
    pub info_hash: InfoHash,
    /// Number of pieces in torrent
    pub piece_count: u32,
    /// Size of each piece in bytes
    pub piece_size: u32,
    /// Total size of torrent content in bytes
    pub total_size: u64,
    /// Original filename for MIME type detection
    pub filename: String,
    /// Pieces we have completed
    pub completed_pieces: Vec<bool>,
    /// Download progress (0.0 to 1.0)
    pub progress: f32,
    /// When download started (for simulation)
    pub started_at: std::time::Instant,
    /// Whether download is actively running
    pub is_downloading: bool,
    /// Tracker URLs for this torrent
    pub tracker_urls: Vec<String>,
    /// Current download speed in bytes per second
    pub download_speed_bps: u64,
    /// Current upload speed in bytes per second  
    pub upload_speed_bps: u64,
    /// Total bytes downloaded for this torrent
    pub bytes_downloaded: u64,
    /// Total bytes uploaded for this torrent
    pub bytes_uploaded: u64,
}

impl TorrentSession {
    /// Creates a new torrent session.
    ///
    /// Initializes a session with the given parameters and sets up the
    /// completed pieces tracking vector.
    pub fn new(params: TorrentSessionParams) -> Self {
        Self {
            info_hash: params.info_hash,
            piece_count: params.piece_count,
            piece_size: params.piece_size,
            total_size: params.total_size,
            filename: params.filename,
            completed_pieces: vec![false; params.piece_count as usize],
            progress: 0.0,
            started_at: std::time::Instant::now(),
            is_downloading: false,
            tracker_urls: params.tracker_urls,
            download_speed_bps: 0,
            upload_speed_bps: 0,
            bytes_downloaded: 0,
            bytes_uploaded: 0,
        }
    }

    /// Marks a piece as completed and updates progress.
    pub fn complete_piece(&mut self, piece_index: u32) {
        if let Some(piece) = self.completed_pieces.get_mut(piece_index as usize) {
            *piece = true;
            self.update_progress();
        }
    }

    /// Returns true if all pieces have been completed.
    pub fn is_complete(&self) -> bool {
        self.completed_pieces.iter().all(|&completed| completed)
    }

    /// Updates the progress based on completed pieces.
    fn update_progress(&mut self) {
        let completed_count = self.completed_pieces.iter().filter(|&&x| x).count();
        self.progress = completed_count as f32 / self.piece_count as f32;
    }

    /// Updates speed statistics for this torrent.
    pub fn update_speed_stats(&mut self, stats: DownloadStats) {
        self.download_speed_bps = stats.download_speed_bps;
        self.upload_speed_bps = stats.upload_speed_bps;
        self.bytes_downloaded = stats.bytes_downloaded;
        self.bytes_uploaded = stats.bytes_uploaded;
    }

    /// Returns download speed in a human-readable format.
    pub fn download_speed_formatted(&self) -> String {
        format_bytes_per_second(self.download_speed_bps)
    }

    /// Returns upload speed in a human-readable format.
    pub fn upload_speed_formatted(&self) -> String {
        format_bytes_per_second(self.upload_speed_bps)
    }
}

/// Engine statistics for monitoring and debugging.
#[derive(Debug, Clone)]
pub struct EngineStats {
    /// Number of active torrents
    pub active_torrents: usize,
    /// Total number of connected peers
    pub total_peers: usize,
    /// Total bytes downloaded across all torrents
    pub bytes_downloaded: u64,
    /// Total bytes uploaded across all torrents
    pub bytes_uploaded: u64,
    /// Average download progress across all torrents
    pub average_progress: f32,
}

/// Parameters for creating a new torrent session.
#[derive(Debug, Clone)]
pub struct TorrentSessionParams {
    pub info_hash: InfoHash,
    pub piece_count: u32,
    pub piece_size: u32,
    pub total_size: u64,
    pub filename: String,
    pub tracker_urls: Vec<String>,
}

/// Download statistics update for a torrent.
#[derive(Debug, Clone)]
pub struct DownloadStats {
    pub download_speed_bps: u64,
    pub upload_speed_bps: u64,
    pub bytes_downloaded: u64,
    pub bytes_uploaded: u64,
}

impl Default for EngineStats {
    fn default() -> Self {
        Self {
            active_torrents: 0,
            total_peers: 0,
            bytes_downloaded: 0,
            bytes_uploaded: 0,
            average_progress: 0.0,
        }
    }
}

/// Formats bytes per second into a human-readable string.
fn format_bytes_per_second(bytes_per_second: u64) -> String {
    if bytes_per_second == 0 {
        return "0 B/s".to_string();
    }

    const UNITS: &[&str] = &["B/s", "KB/s", "MB/s", "GB/s"];
    let mut value = bytes_per_second as f64;
    let mut unit_index = 0;

    while value >= 1024.0 && unit_index < UNITS.len() - 1 {
        value /= 1024.0;
        unit_index += 1;
    }

    if value >= 10.0 {
        format!("{:.0} {}", value, UNITS[unit_index])
    } else {
        format!("{:.1} {}", value, UNITS[unit_index])
    }
}
