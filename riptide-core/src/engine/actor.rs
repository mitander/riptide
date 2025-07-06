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
/// ```rust,no_run
/// # #[tokio::main]
/// # async fn main() {
/// use riptide_core::engine::spawn_torrent_engine;
/// use riptide_core::config::RiptideConfig;
/// use riptide_core::torrent::{TcpPeerManager, TrackerManager};
///
/// let config = RiptideConfig::default();
/// let peer_manager = TcpPeerManager::new_default();
/// let tracker_manager = TrackerManager::new(config.network.clone());
/// let handle = spawn_torrent_engine(config, peer_manager, tracker_manager);
/// # }
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
    let (piece_sender, piece_receiver) = mpsc::unbounded_channel();
    let engine = TorrentEngine::new(config, peer_manager, tracker_manager, piece_sender);

    tokio::spawn(async move {
        run_actor_loop(engine, receiver, piece_receiver).await;
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
    mut piece_receiver: mpsc::UnboundedReceiver<TorrentEngineCommand>,
) where
    P: PeerManager + Send + 'static,
    T: TrackerManagement + Send + 'static,
{
    tracing::debug!("Torrent engine actor started");

    loop {
        tokio::select! {
            Some(command) = receiver.recv() => {
                if !handle_command(&mut engine, command).await {
                    break;
                }
            }
            Some(command) = piece_receiver.recv() => {
                if !handle_command(&mut engine, command).await {
                    break;
                }
            }
            else => break,
        }
    }

    tracing::debug!("Torrent engine actor stopped");
}

/// Handles a single command for the torrent engine.
/// Returns true to continue processing, false to shutdown.
async fn handle_command<P, T>(
    engine: &mut TorrentEngine<P, T>,
    command: TorrentEngineCommand,
) -> bool
where
    P: PeerManager + Send + 'static,
    T: TrackerManagement + Send + 'static,
{
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
                .session(info_hash)
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
            tracing::debug!("Torrent engine actor shutting down");
            let _ = responder.send(());
            return false; // Signal to break out of the loop
        }

        TorrentEngineCommand::PieceCompleted {
            info_hash,
            piece_index,
        } => {
            tracing::debug!("Piece {} completed for torrent {}", piece_index, info_hash);
            let _ = engine.mark_pieces_completed(info_hash, vec![piece_index]);
        }

        TorrentEngineCommand::UpdateDownloadStats { info_hash, stats } => {
            let _ = engine.update_download_stats(info_hash, stats);
        }

        TorrentEngineCommand::SeekToPosition {
            info_hash,
            byte_position,
            buffer_size,
            responder,
        } => {
            let result = engine.seek_to_position(info_hash, byte_position, buffer_size);
            let _ = responder.send(result);
        }

        TorrentEngineCommand::UpdateBufferStrategy {
            info_hash,
            playback_speed,
            available_bandwidth,
            responder,
        } => {
            let result =
                engine.update_buffer_strategy(info_hash, playback_speed, available_bandwidth);
            let _ = responder.send(result);
        }

        TorrentEngineCommand::GetBufferStatus {
            info_hash,
            responder,
        } => {
            let result = engine.buffer_status(info_hash);
            let _ = responder.send(result);
        }

        TorrentEngineCommand::StopDownload {
            info_hash,
            responder,
        } => {
            let result = engine.stop_download(info_hash);
            let _ = responder.send(result);
        }

        TorrentEngineCommand::ConfigureUploadManager {
            info_hash,
            piece_size,
            total_bandwidth: _,
            responder,
        } => {
            engine
                .configure_upload_manager_for_streaming(info_hash, piece_size)
                .await;
            let _ = responder.send(Ok(()));
        }

        TorrentEngineCommand::UpdateStreamingPosition {
            info_hash,
            byte_position,
            responder,
        } => {
            let result = engine
                .update_streaming_position_coordinated(info_hash, byte_position)
                .await;
            let _ = responder.send(result);
        }
    }
    true // Continue processing
}

#[cfg(test)]
mod tests {
    use sha1::{Digest, Sha1};

    use super::*;
    use crate::config::RiptideConfig;
    use crate::torrent::{InfoHash, TorrentError};

    /// Generates proper SHA1 hashes for test torrent metadata.
    fn generate_test_piece_hashes(piece_count: usize, piece_size: u32) -> Vec<[u8; 20]> {
        (0..piece_count)
            .map(|i| {
                let mut hasher = Sha1::new();
                hasher.update(vec![i as u8; piece_size as usize]);
                let result = hasher.finalize();
                let mut hash = [0u8; 20];
                hash.copy_from_slice(&result);
                hash
            })
            .collect()
    }

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
        println!("Result after shutdown: {:?}", result);
        // The channel should be closed, so we should get an error
        assert!(result.is_err());
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

    #[tokio::test]
    async fn test_real_download_basic_flow() {
        let config = RiptideConfig::default();
        let mut peer_manager = crate::torrent::MockPeerManager::new();
        peer_manager.enable_piece_data_simulation();
        let mut tracker_manager = crate::torrent::MockTrackerManager::new();

        // Set up mock peers for the tracker manager
        let mock_peers = vec![
            "127.0.0.1:8080".parse().unwrap(),
            "127.0.0.1:8081".parse().unwrap(),
        ];
        tracker_manager.set_mock_peers(mock_peers);

        let handle = spawn_torrent_engine(config, peer_manager, tracker_manager);

        // Create test torrent metadata
        let piece_count = 3;
        let piece_size = 32_768u32;
        let total_size = piece_count as u64 * piece_size as u64;
        let piece_hashes = generate_test_piece_hashes(piece_count, piece_size);

        let metadata = crate::torrent::parsing::types::TorrentMetadata {
            info_hash: crate::torrent::InfoHash::new([1u8; 20]),
            name: "test.bin".to_string(),
            total_length: total_size,
            piece_length: piece_size,
            piece_hashes,
            files: vec![crate::torrent::parsing::types::TorrentFile {
                path: vec!["test.bin".to_string()],
                length: total_size,
            }],
            announce_urls: vec!["http://tracker.example.com/announce".to_string()],
        };

        // Add torrent and start download
        let info_hash = handle.add_torrent_metadata(metadata).await.unwrap();
        let session = handle.get_session(info_hash).await.unwrap();
        assert!(!session.is_downloading);

        handle.start_download(info_hash).await.unwrap();
        let session = handle.get_session(info_hash).await.unwrap();
        assert!(session.is_downloading);

        // Verify engine stats
        let stats = handle.get_download_stats().await.unwrap();
        assert_eq!(stats.active_torrents, 1);

        handle.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn test_real_download_error_handling() {
        let config = RiptideConfig::default();

        // Create tracker manager that fails announces
        let peer_manager = crate::torrent::MockPeerManager::new();
        let tracker_manager = crate::torrent::MockTrackerManager::new_with_announce_failure();

        let handle = spawn_torrent_engine(config, peer_manager, tracker_manager);

        // Create test torrent metadata
        let piece_count = 3;
        let piece_size = 32_768u32;
        let total_size = piece_count as u64 * piece_size as u64;
        let piece_hashes = generate_test_piece_hashes(piece_count, piece_size);

        let metadata = crate::torrent::parsing::types::TorrentMetadata {
            info_hash: crate::torrent::InfoHash::new([2u8; 20]),
            name: "error_test.bin".to_string(),
            total_length: total_size,
            piece_length: piece_size,
            piece_hashes,
            files: vec![crate::torrent::parsing::types::TorrentFile {
                path: vec!["error_test.bin".to_string()],
                length: total_size,
            }],
            announce_urls: vec!["http://tracker.example.com/announce".to_string()],
        };

        // Add torrent and start download - should fail due to tracker errors
        let info_hash = handle.add_torrent_metadata(metadata).await.unwrap();
        handle.start_download(info_hash).await.unwrap();

        // Verify download started but will fail
        let session = handle.get_session(info_hash).await.unwrap();
        assert!(session.is_downloading);

        // Brief wait for download attempt
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;

        // Verify engine stats
        let stats = handle.get_download_stats().await.unwrap();
        assert_eq!(stats.active_torrents, 1);

        handle.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn test_multiple_torrent_management() {
        let config = RiptideConfig::default();
        let mut peer_manager = crate::torrent::MockPeerManager::new();
        peer_manager.enable_piece_data_simulation();
        let mut tracker_manager = crate::torrent::MockTrackerManager::new();
        tracker_manager.set_mock_peers(vec!["127.0.0.1:8080".parse().unwrap()]);

        let handle = spawn_torrent_engine(config, peer_manager, tracker_manager);

        // Add multiple torrents
        let info_hash1 = handle
            .add_torrent_metadata(crate::torrent::parsing::types::TorrentMetadata {
                info_hash: crate::torrent::InfoHash::new([1u8; 20]),
                name: "torrent1.bin".to_string(),
                total_length: 1024,
                piece_length: 512,
                piece_hashes: vec![[1u8; 20], [2u8; 20]],
                files: vec![crate::torrent::parsing::types::TorrentFile {
                    path: vec!["torrent1.bin".to_string()],
                    length: 1024,
                }],
                announce_urls: vec!["http://tracker.example.com/announce".to_string()],
            })
            .await
            .unwrap();

        let info_hash2 = handle
            .add_torrent_metadata(crate::torrent::parsing::types::TorrentMetadata {
                info_hash: crate::torrent::InfoHash::new([2u8; 20]),
                name: "torrent2.bin".to_string(),
                total_length: 2048,
                piece_length: 512,
                piece_hashes: vec![[3u8; 20], [4u8; 20], [5u8; 20], [6u8; 20]],
                files: vec![crate::torrent::parsing::types::TorrentFile {
                    path: vec!["torrent2.bin".to_string()],
                    length: 2048,
                }],
                announce_urls: vec!["http://tracker.example.com/announce".to_string()],
            })
            .await
            .unwrap();

        // Verify both torrents exist
        let sessions = handle.get_active_sessions().await.unwrap();
        assert_eq!(sessions.len(), 2);

        // Start downloads
        handle.start_download(info_hash1).await.unwrap();
        handle.start_download(info_hash2).await.unwrap();

        // Verify stats
        let stats = handle.get_download_stats().await.unwrap();
        assert_eq!(stats.active_torrents, 2);

        handle.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn test_magnet_link_display_name_parsing() {
        let config = RiptideConfig::default();
        let peer_manager = crate::torrent::MockPeerManager::new();
        let tracker_manager = crate::torrent::MockTrackerManager::new();
        let handle = spawn_torrent_engine(config, peer_manager, tracker_manager);

        // Test magnet link with display name
        let magnet_with_name = "magnet:?xt=urn:btih:0123456789abcdef0123456789abcdef01234567&dn=Wallace+And+Gromit+Vengeance+Most+Fowl+2024+720p+WEB-DL+x264+BONE&tr=http://tracker.example.com/announce";

        let info_hash = handle.add_magnet(magnet_with_name).await.unwrap();
        let session = handle.get_session(info_hash).await.unwrap();

        // Verify the display name was parsed and used as filename
        assert_eq!(
            session.filename,
            "Wallace And Gromit Vengeance Most Fowl 2024 720p WEB-DL x264 BONE"
        );

        // Test magnet link without display name
        let magnet_without_name = "magnet:?xt=urn:btih:fedcba9876543210fedcba9876543210fedcba98&tr=http://tracker.example.com/announce";

        let info_hash2 = handle.add_magnet(magnet_without_name).await.unwrap();
        let session2 = handle.get_session(info_hash2).await.unwrap();

        // Verify fallback name includes more of the hash for readability
        assert!(session2.filename.starts_with("Torrent_fedcba9876543210"));

        println!("✓ Magnet with display name: '{}'", session.filename);
        println!("✓ Magnet fallback name: '{}'", session2.filename);

        handle.shutdown().await.unwrap();
    }
}
