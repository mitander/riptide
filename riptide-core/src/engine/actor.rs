//! Actor implementation for the torrent engine.

use tokio::sync::mpsc;

use super::commands::TorrentEngineCommand;
use super::core::TorrentEngine;
use super::handle::TorrentEngineHandle;
use crate::config::RiptideConfig;
use crate::torrent::{PeerManager, TrackerManagement};

/// Spawns the torrent engine actor and returns its handle.
///
/// Creates a new torrent engine with the provided configuration and managers,
/// then spawns it as an actor running in a separate task. The actor processes
/// commands sequentially, eliminating lock contention and race conditions.
///
/// # Examples
/// ```
/// use riptide_core::engine::spawn_torrent_engine;
/// use riptide_core::config::RiptideConfig;
/// use riptide_core::torrent::{TcpPeerManager, TrackerManager};
///
/// let config = RiptideConfig::default();
/// let peer_manager = TcpPeerManager::new_default();
/// let tracker_manager = TrackerManager::new(config.network.clone());
/// let handle = spawn_torrent_engine(config, peer_manager, tracker_manager);
/// ```
pub fn spawn_torrent_engine<P, T>(
    config: RiptideConfig,
    peer_manager: P,
    tracker_manager: T,
) -> TorrentEngineHandle
where
    P: PeerManager + Send + 'static,
    T: TrackerManagement + Send + 'static,
{
    let (sender, receiver) = mpsc::channel(100);
    let engine = TorrentEngine::new(config, peer_manager, tracker_manager);

    tokio::spawn(async move {
        run_actor_loop(engine, receiver).await;
    });

    TorrentEngineHandle::new(sender)
}

/// Runs the main actor message processing loop.
///
/// This is the core of the actor model implementation. It processes commands
/// one by one in order, ensuring consistent state management without locks.
/// The loop continues until the command channel is closed or a shutdown
/// command is received.
async fn run_actor_loop<P, T>(
    mut engine: TorrentEngine<P, T>,
    mut receiver: mpsc::Receiver<TorrentEngineCommand>,
) where
    P: PeerManager + Send + 'static,
    T: TrackerManagement + Send + 'static,
{
    tracing::info!("Torrent engine actor started");

    while let Some(command) = receiver.recv().await {
        match command {
            TorrentEngineCommand::AddMagnet {
                magnet_link,
                responder,
            } => {
                let result = engine.add_magnet(&magnet_link).await;
                let _ = responder.send(result);
            }

            TorrentEngineCommand::StartDownload {
                info_hash,
                responder,
            } => {
                let result = engine.start_download(info_hash).await;
                let _ = responder.send(result);
            }

            TorrentEngineCommand::GetSession {
                info_hash,
                responder,
            } => {
                let result = engine
                    .get_session(info_hash)
                    .cloned()
                    .ok_or_else(|| crate::torrent::TorrentError::TorrentNotFound { info_hash });
                let _ = responder.send(result);
            }

            TorrentEngineCommand::GetActiveSessions { responder } => {
                let sessions = engine.active_sessions().cloned().collect();
                let _ = responder.send(sessions);
            }

            TorrentEngineCommand::GetDownloadStats { responder } => {
                let stats = engine.get_download_stats().await;
                let _ = responder.send(stats);
            }

            TorrentEngineCommand::MarkPiecesCompleted {
                info_hash,
                piece_indices,
                responder,
            } => {
                let result = engine.mark_pieces_completed(info_hash, piece_indices);
                let _ = responder.send(result);
            }

            TorrentEngineCommand::AddTorrentMetadata {
                metadata,
                responder,
            } => {
                let result = engine.add_torrent_metadata(metadata);
                let _ = responder.send(result);
            }

            TorrentEngineCommand::Shutdown { responder } => {
                tracing::info!("Torrent engine actor shutting down");
                let _ = responder.send(());
                break;
            }
        }
    }

    tracing::info!("Torrent engine actor stopped");
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::RiptideConfig;
    use crate::torrent::{InfoHash, TorrentError};

    #[tokio::test]
    async fn test_actor_spawn_and_basic_operations() {
        let config = RiptideConfig::default();
        let peer_manager = crate::torrent::MockPeerManager::new();
        let tracker_manager = crate::torrent::MockTrackerManager::new();

        let handle = spawn_torrent_engine(config, peer_manager, tracker_manager);

        // Test basic functionality
        assert!(handle.is_running());

        // Test getting stats from empty engine
        let stats = handle.get_download_stats().await.unwrap();
        assert_eq!(stats.active_torrents, 0);

        // Test shutdown
        handle.shutdown().await.unwrap();

        // Give the actor time to shut down
        tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;

        // Further operations should fail
        let result = handle.get_download_stats().await;
        assert!(matches!(result, Err(TorrentError::EngineShutdown)));
    }

    #[tokio::test]
    async fn test_actor_add_magnet_invalid() {
        let config = RiptideConfig::default();
        let peer_manager = crate::torrent::MockPeerManager::new();
        let tracker_manager = crate::torrent::MockTrackerManager::new();

        let handle = spawn_torrent_engine(config, peer_manager, tracker_manager);

        // Test invalid magnet link
        let result = handle.add_magnet("invalid-magnet").await;
        assert!(result.is_err());

        handle.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn test_actor_get_nonexistent_session() {
        let config = RiptideConfig::default();
        let peer_manager = crate::torrent::MockPeerManager::new();
        let tracker_manager = crate::torrent::MockTrackerManager::new();

        let handle = spawn_torrent_engine(config, peer_manager, tracker_manager);

        // Test getting session for non-existent torrent
        let fake_hash = InfoHash::new([0u8; 20]);
        let result = handle.get_session(fake_hash).await;
        assert!(matches!(result, Err(TorrentError::TorrentNotFound { .. })));

        handle.shutdown().await.unwrap();
    }
}
