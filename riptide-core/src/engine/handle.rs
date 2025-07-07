//! Handle for communicating with the torrent engine actor.

use tokio::sync::{mpsc, oneshot};

use super::commands::{EngineStats, TorrentEngineCommand, TorrentSession};
use crate::torrent::parsing::types::TorrentMetadata;
use crate::torrent::{InfoHash, TorrentError};

/// Handle for communicating with the torrent engine actor.
///
/// This handle provides an ergonomic async API for sending commands to the
/// torrent engine actor. It can be cloned and shared across threads safely.
#[derive(Clone)]
pub struct TorrentEngineHandle {
    sender: mpsc::Sender<TorrentEngineCommand>,
}

impl TorrentEngineHandle {
    /// Creates a new handle with the given command sender.
    pub fn new(sender: mpsc::Sender<TorrentEngineCommand>) -> Self {
        Self { sender }
    }

    /// Adds a torrent from a magnet link.
    ///
    /// Parses the magnet link and adds the torrent to the engine's active
    /// torrent list. The torrent can then be started for downloading.
    ///
    /// # Errors
    /// - `TorrentError::InvalidMagnetLink` - Malformed magnet link
    /// - `TorrentError::TrackerConnectionFailed` - Could not contact tracker
    /// - `TorrentError::DuplicateTorrent` - Torrent already exists
    pub async fn add_magnet(&self, magnet_link: &str) -> Result<InfoHash, TorrentError> {
        let (responder, rx) = oneshot::channel();
        let cmd = TorrentEngineCommand::AddMagnet {
            magnet_link: magnet_link.to_string(),
            responder,
        };

        self.sender
            .send(cmd)
            .await
            .map_err(|_| TorrentError::EngineShutdown)?;

        rx.await.map_err(|_| TorrentError::EngineShutdown)?
    }

    /// Starts downloading a torrent by its info hash.
    ///
    /// Initiates the download process by connecting to trackers, discovering
    /// peers, and beginning piece acquisition. The torrent must already be
    /// added to the engine via `add_magnet` or `add_torrent_metadata`.
    ///
    /// # Errors
    /// - `TorrentError::TorrentNotFound` - Info hash not in active torrents
    /// - `TorrentError::TrackerConnectionFailed` - Could not contact tracker
    /// - `TorrentError::PeerConnectionError` - Could not connect to peers
    pub async fn start_download(&self, info_hash: InfoHash) -> Result<(), TorrentError> {
        let (responder, rx) = oneshot::channel();
        let cmd = TorrentEngineCommand::StartDownload {
            info_hash,
            responder,
        };

        self.sender
            .send(cmd)
            .await
            .map_err(|_| TorrentError::EngineShutdown)?;

        rx.await.map_err(|_| TorrentError::EngineShutdown)?
    }

    /// Gets the session information for a specific torrent.
    ///
    /// Returns detailed information about the torrent's download progress,
    /// piece completion status, and session metadata.
    ///
    /// # Errors
    /// - `TorrentError::TorrentNotFound` - Info hash not in active torrents
    pub async fn session_details(
        &self,
        info_hash: InfoHash,
    ) -> Result<TorrentSession, TorrentError> {
        let (responder, rx) = oneshot::channel();
        let cmd = TorrentEngineCommand::GetSession {
            info_hash,
            responder,
        };

        self.sender
            .send(cmd)
            .await
            .map_err(|_| TorrentError::EngineShutdown)?;

        rx.await.map_err(|_| TorrentError::EngineShutdown)?
    }

    /// Lists all active torrent sessions.
    ///
    /// Returns a vector of all torrent sessions currently managed by the engine.
    /// Sessions are returned in no particular order.
    ///
    /// # Errors
    /// Returns error if engine is shutdown or communication fails.
    pub async fn active_sessions(&self) -> Result<Vec<TorrentSession>, TorrentError> {
        let (responder, rx) = oneshot::channel();
        let cmd = TorrentEngineCommand::GetActiveSessions { responder };

        self.sender
            .send(cmd)
            .await
            .map_err(|_| TorrentError::EngineShutdown)?;

        rx.await.map_err(|_| TorrentError::EngineShutdown)
    }

    /// Returns download statistics for the engine.
    ///
    /// Returns aggregated statistics including active torrent count, peer
    /// connections, and data transfer metrics.
    ///
    /// # Errors
    /// Returns error if engine is shutdown or communication fails.
    pub async fn download_statistics(&self) -> Result<EngineStats, TorrentError> {
        let (responder, rx) = oneshot::channel();
        let cmd = TorrentEngineCommand::GetDownloadStats { responder };

        self.sender
            .send(cmd)
            .await
            .map_err(|_| TorrentError::EngineShutdown)?;

        rx.await.map_err(|_| TorrentError::EngineShutdown)
    }

    /// Marks specific pieces as completed for a torrent.
    ///
    /// Updates the torrent's piece completion status and recalculates download
    /// progress. This is typically called by the piece downloader when pieces
    /// are successfully downloaded and verified.
    ///
    /// # Errors
    /// - `TorrentError::TorrentNotFound` - Info hash not in active torrents
    /// - `TorrentError::InvalidPieceIndex` - Piece index out of bounds
    pub async fn mark_pieces_completed(
        &self,
        info_hash: InfoHash,
        piece_indices: Vec<u32>,
    ) -> Result<(), TorrentError> {
        let (responder, rx) = oneshot::channel();
        let cmd = TorrentEngineCommand::MarkPiecesCompleted {
            info_hash,
            piece_indices,
            responder,
        };

        self.sender
            .send(cmd)
            .await
            .map_err(|_| TorrentError::EngineShutdown)?;

        rx.await.map_err(|_| TorrentError::EngineShutdown)?
    }

    /// Adds torrent metadata directly to the engine.
    ///
    /// Registers a torrent with the engine using pre-parsed metadata. This is
    /// useful for simulation scenarios where torrent metadata is generated
    /// programmatically rather than parsed from magnet links.
    ///
    /// # Errors
    /// - `TorrentError::DuplicateTorrent` - Torrent already exists
    /// - `TorrentError::InvalidMetadata` - Metadata is malformed
    pub async fn add_torrent_metadata(
        &self,
        metadata: TorrentMetadata,
    ) -> Result<InfoHash, TorrentError> {
        let (responder, rx) = oneshot::channel();
        let cmd = TorrentEngineCommand::AddTorrentMetadata {
            metadata,
            responder,
        };

        self.sender
            .send(cmd)
            .await
            .map_err(|_| TorrentError::EngineShutdown)?;

        rx.await.map_err(|_| TorrentError::EngineShutdown)?
    }

    /// Shuts down the engine actor gracefully.
    ///
    /// Sends a shutdown command to the engine actor and waits for confirmation.
    /// After this call, all subsequent operations will return `TorrentError::EngineShutdown`.
    ///
    /// # Errors
    /// Returns error if engine communication fails.
    pub async fn shutdown(&self) -> Result<(), TorrentError> {
        let (responder, rx) = oneshot::channel();
        let cmd = TorrentEngineCommand::Shutdown { responder };

        self.sender
            .send(cmd)
            .await
            .map_err(|_| TorrentError::EngineShutdown)?;

        rx.await.map_err(|_| TorrentError::EngineShutdown)
    }

    /// Requests prioritization of pieces around a seek position for streaming.
    ///
    /// Signals the torrent engine to prioritize downloading pieces around the
    /// specified byte position to enable smooth seeking in video playback.
    /// The buffer size determines how many bytes around the position to prioritize.
    ///
    /// # Errors
    /// - `TorrentError::TorrentNotFound` - Info hash not in active torrents
    pub async fn seek_to_position(
        &self,
        info_hash: InfoHash,
        byte_position: u64,
        buffer_size: u64,
    ) -> Result<(), TorrentError> {
        let (responder, rx) = oneshot::channel();
        let cmd = TorrentEngineCommand::SeekToPosition {
            info_hash,
            byte_position,
            buffer_size,
            responder,
        };

        self.sender
            .send(cmd)
            .await
            .map_err(|_| TorrentError::EngineShutdown)?;

        rx.await.map_err(|_| TorrentError::EngineShutdown)?
    }

    /// Updates buffer strategy for adaptive piece picking.
    ///
    /// Adjusts the buffer size and prioritization strategy based on current
    /// playback speed and available bandwidth. This helps optimize buffering
    /// for different viewing conditions.
    ///
    /// # Errors
    /// - `TorrentError::TorrentNotFound` - Info hash not in active torrents
    pub async fn update_buffer_strategy(
        &self,
        info_hash: InfoHash,
        playback_speed: f64,
        available_bandwidth: u64,
    ) -> Result<(), TorrentError> {
        let (responder, rx) = oneshot::channel();
        let cmd = TorrentEngineCommand::UpdateBufferStrategy {
            info_hash,
            playback_speed,
            available_bandwidth,
            responder,
        };

        self.sender
            .send(cmd)
            .await
            .map_err(|_| TorrentError::EngineShutdown)?;

        rx.await.map_err(|_| TorrentError::EngineShutdown)?
    }

    /// Gets current buffer status for a torrent.
    ///
    /// Returns detailed information about the buffering state, including
    /// how much content is buffered ahead and behind the current position.
    ///
    /// # Errors
    /// - `TorrentError::TorrentNotFound` - Info hash not in active torrents
    pub async fn buffer_status(
        &self,
        info_hash: InfoHash,
    ) -> Result<crate::torrent::BufferStatus, TorrentError> {
        let (responder, rx) = oneshot::channel();
        let cmd = TorrentEngineCommand::GetBufferStatus {
            info_hash,
            responder,
        };

        self.sender
            .send(cmd)
            .await
            .map_err(|_| TorrentError::EngineShutdown)?;

        rx.await.map_err(|_| TorrentError::EngineShutdown)?
    }

    /// Stops downloading a torrent by its info hash.
    ///
    /// Stops the download process for the specified torrent, closes peer
    /// connections, and releases resources. The torrent remains in the
    /// engine's list but is marked as not downloading.
    ///
    /// # Errors
    /// - `TorrentError::TorrentNotFound` - Info hash not in active torrents
    pub async fn stop_download(&self, info_hash: InfoHash) -> Result<(), TorrentError> {
        let (responder, rx) = oneshot::channel();
        let cmd = TorrentEngineCommand::StopDownload {
            info_hash,
            responder,
        };

        self.sender
            .send(cmd)
            .await
            .map_err(|_| TorrentError::EngineShutdown)?;

        rx.await.map_err(|_| TorrentError::EngineShutdown)?
    }

    /// Configures upload manager for streaming optimization.
    ///
    /// Sets up upload throttling and bandwidth management parameters optimized
    /// for streaming media content. This should be called when starting torrents
    /// that will be used for streaming.
    ///
    /// # Errors
    /// - `TorrentError::TorrentNotFound` - Info hash not in active torrents
    pub async fn configure_upload_manager(
        &self,
        info_hash: InfoHash,
        piece_size: u64,
        total_bandwidth: u64,
    ) -> Result<(), TorrentError> {
        let (responder, rx) = oneshot::channel();
        let cmd = TorrentEngineCommand::ConfigureUploadManager {
            info_hash,
            piece_size,
            total_bandwidth,
            responder,
        };

        self.sender
            .send(cmd)
            .await
            .map_err(|_| TorrentError::EngineShutdown)?;

        rx.await.map_err(|_| TorrentError::EngineShutdown)?
    }

    /// Updates streaming position for upload throttling.
    ///
    /// Informs the upload manager about the current streaming position so it
    /// can adjust upload priorities to favor pieces needed for streaming.
    ///
    /// # Errors
    /// - `TorrentError::TorrentNotFound` - Info hash not in active torrents
    pub async fn update_streaming_position(
        &self,
        info_hash: InfoHash,
        byte_position: u64,
    ) -> Result<(), TorrentError> {
        let (responder, rx) = oneshot::channel();
        let cmd = TorrentEngineCommand::UpdateStreamingPosition {
            info_hash,
            byte_position,
            responder,
        };

        self.sender
            .send(cmd)
            .await
            .map_err(|_| TorrentError::EngineShutdown)?;

        rx.await.map_err(|_| TorrentError::EngineShutdown)?
    }

    /// Checks if the engine actor is still running.
    ///
    /// Returns true if the sender channel is still open, indicating the
    /// engine actor is running and accepting commands.
    pub fn is_running(&self) -> bool {
        !self.sender.is_closed()
    }
}
