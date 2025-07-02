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
    pub async fn get_session(&self, info_hash: InfoHash) -> Result<TorrentSession, TorrentError> {
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

    /// Gets all active torrent sessions.
    ///
    /// Returns a vector of all torrent sessions currently managed by the engine.
    /// Sessions are returned in no particular order.
    pub async fn get_active_sessions(&self) -> Result<Vec<TorrentSession>, TorrentError> {
        let (responder, rx) = oneshot::channel();
        let cmd = TorrentEngineCommand::GetActiveSessions { responder };

        self.sender
            .send(cmd)
            .await
            .map_err(|_| TorrentError::EngineShutdown)?;

        rx.await.map_err(|_| TorrentError::EngineShutdown)
    }

    /// Gets download statistics for the engine.
    ///
    /// Returns aggregated statistics including active torrent count, peer
    /// connections, and data transfer metrics.
    pub async fn get_download_stats(&self) -> Result<EngineStats, TorrentError> {
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
    pub async fn shutdown(&self) -> Result<(), TorrentError> {
        let (responder, rx) = oneshot::channel();
        let cmd = TorrentEngineCommand::Shutdown { responder };

        self.sender
            .send(cmd)
            .await
            .map_err(|_| TorrentError::EngineShutdown)?;

        rx.await.map_err(|_| TorrentError::EngineShutdown)
    }

    /// Checks if the engine actor is still running.
    ///
    /// Returns true if the sender channel is still open, indicating the
    /// engine actor is running and accepting commands.
    pub fn is_running(&self) -> bool {
        !self.sender.is_closed()
    }
}
