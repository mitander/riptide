//! Integration tests for the torrent engine.
//!
//! These tests verify the complete torrent download workflow using the public
//! TorrentEngineHandle API, including tracker communication, peer management,
//! and piece coordination.

use std::time::Duration;

use riptide_core::config::RiptideConfig;
use riptide_core::engine::{MockPeers, MockTracker, TorrentEngineHandle, spawn_torrent_engine};
use riptide_core::torrent::parsing::types::TorrentMetadata;
use riptide_core::torrent::{InfoHash, TorrentError};
use tokio::time::timeout;

/// Test fixture for engine integration tests using the public API.
struct EngineTestFixture {
    handle: TorrentEngineHandle,
}

impl EngineTestFixture {
    /// Creates a new test fixture with configured mock dependencies.
    fn new() -> Self {
        let config = RiptideConfig::default();
        let peers = MockPeers::new();
        let tracker = MockTracker::new();

        let handle = spawn_torrent_engine(config, peers, tracker);

        Self { handle }
    }

    /// Creates a fixture with failing mock dependencies for error testing.
    fn new_with_failures() -> Self {
        let config = RiptideConfig::default();
        let peers = MockPeers::new_with_connection_failure();
        let tracker = MockTracker::new_with_announce_failure();

        let handle = spawn_torrent_engine(config, peers, tracker);

        Self { handle }
    }

    /// Creates test torrent metadata with specified parameters.
    fn create_test_metadata(
        &self,
        info_hash: InfoHash,
        name: &str,
        piece_count: u32,
    ) -> TorrentMetadata {
        TorrentMetadata {
            info_hash,
            name: name.to_string(),
            piece_length: 32768,
            piece_hashes: vec![[0u8; 20]; piece_count as usize],
            total_length: (piece_count as u64) * 32768,
            announce_urls: vec![
                "udp://tracker.example.com:1337/announce".to_string(),
                "udp://backup.tracker.com:1337/announce".to_string(),
            ],
            files: vec![],
        }
    }
}

#[tokio::test]
async fn test_complete_torrent_lifecycle() {
    let fixture = EngineTestFixture::new();
    let info_hash = InfoHash::new([1u8; 20]);
    let metadata = fixture.create_test_metadata(info_hash, "test_movie.mp4", 100);

    // Add torrent metadata
    let result = fixture.handle.add_torrent_metadata(metadata.clone()).await;
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), info_hash);

    // Verify torrent is tracked
    let session_result = fixture.handle.session_details(info_hash).await;
    assert!(session_result.is_ok());
    let session = session_result.unwrap();
    assert_eq!(session.filename, "test_movie.mp4");
    assert_eq!(session.piece_count, 100);
    assert!(!session.is_downloading);
    assert_eq!(session.progress, 0.0);

    // Start download
    let start_result = fixture.handle.start_download(info_hash).await;
    assert!(start_result.is_ok());

    // Allow some time for download to start
    tokio::time::sleep(Duration::from_millis(10)).await;

    // Verify download state
    let session_result = fixture.handle.session_details(info_hash).await;
    assert!(session_result.is_ok());
    let session = session_result.unwrap();
    assert!(session.is_downloading);

    // Simulate piece completion
    let pieces_to_complete = vec![0, 1, 2, 3, 4]; // 5%
    let mark_result = fixture
        .handle
        .mark_pieces_completed(info_hash, pieces_to_complete)
        .await;
    assert!(mark_result.is_ok());

    // Allow time for progress update
    tokio::time::sleep(Duration::from_millis(10)).await;

    // Verify progress update
    let session_result = fixture.handle.session_details(info_hash).await;
    assert!(session_result.is_ok());
    let session = session_result.unwrap();
    assert_eq!(session.progress, 0.05); // 5 out of 100 pieces

    // Complete all pieces
    let all_pieces: Vec<u32> = (0..100).collect();
    let complete_result = fixture
        .handle
        .mark_pieces_completed(info_hash, all_pieces)
        .await;
    assert!(complete_result.is_ok());

    // Allow time for completion
    tokio::time::sleep(Duration::from_millis(10)).await;

    // Verify completion
    let session_result = fixture.handle.session_details(info_hash).await;
    assert!(session_result.is_ok());
    let session = session_result.unwrap();
    assert_eq!(session.progress, 1.0);
    assert_eq!(session.completed_pieces.iter().filter(|&&x| x).count(), 100);

    // Stop download
    let stop_result = fixture.handle.stop_download(info_hash).await;
    assert!(stop_result.is_ok());

    // Allow time for stop
    tokio::time::sleep(Duration::from_millis(10)).await;

    let session_result = fixture.handle.session_details(info_hash).await;
    assert!(session_result.is_ok());
    let session = session_result.unwrap();
    assert!(!session.is_downloading);
}

#[tokio::test]
async fn test_magnet_link_workflow() {
    let fixture = EngineTestFixture::new();

    // Add magnet link with display name and trackers
    let magnet = "magnet:?xt=urn:btih:0123456789abcdef0123456789abcdef01234567&dn=Ubuntu%20ISO&tr=udp://tracker.ubuntu.com:1337&tr=udp://backup.ubuntu.com:1337";
    let result = fixture.handle.add_magnet(magnet).await;
    assert!(result.is_ok());

    let info_hash = result.unwrap();

    // Verify magnet session creation
    let session_result = fixture.handle.session_details(info_hash).await;
    assert!(session_result.is_ok());
    let session = session_result.unwrap();
    assert_eq!(session.filename, "Ubuntu ISO");
    assert_eq!(session.tracker_urls.len(), 2);
    assert!(
        session
            .tracker_urls
            .contains(&"udp://tracker.ubuntu.com:1337".to_string())
    );
    assert!(
        session
            .tracker_urls
            .contains(&"udp://backup.ubuntu.com:1337".to_string())
    );

    // Test duplicate magnet link
    let duplicate_result = fixture.handle.add_magnet(magnet).await;
    assert!(matches!(
        duplicate_result,
        Err(TorrentError::DuplicateTorrent { .. })
    ));
}

#[tokio::test]
async fn test_magnet_link_with_fallback_trackers() {
    let fixture = EngineTestFixture::new();

    // Add magnet link without trackers
    let magnet = "magnet:?xt=urn:btih:0123456789abcdef0123456789abcdef01234567&dn=No%20Trackers";
    let result = fixture.handle.add_magnet(magnet).await;
    assert!(result.is_ok());

    let info_hash = result.unwrap();
    let session_result = fixture.handle.session_details(info_hash).await;
    assert!(session_result.is_ok());
    let session = session_result.unwrap();

    // Should use fallback trackers
    assert!(!session.tracker_urls.is_empty());
    assert!(
        session
            .tracker_urls
            .iter()
            .any(|t| t.contains("tracker.openbittorrent.com"))
    );
    assert!(
        session
            .tracker_urls
            .iter()
            .any(|t| t.contains("tracker.publicbt.com"))
    );
}

#[tokio::test]
async fn test_multiple_torrent_coordination() {
    let fixture = EngineTestFixture::new();

    // Add multiple torrents
    let torrents = [
        ("Movie 1", 50),
        ("Movie 2", 75),
        ("Movie 3", 100),
        ("Documentary", 25),
    ];

    let mut info_hashes = Vec::new();

    for (i, (name, piece_count)) in torrents.iter().enumerate() {
        let info_hash = InfoHash::new([i as u8; 20]);
        let metadata = fixture.create_test_metadata(info_hash, name, *piece_count);

        let result = fixture.handle.add_torrent_metadata(metadata).await;
        assert!(result.is_ok());
        info_hashes.push(info_hash);
    }

    // Verify all torrents are tracked
    let sessions_result = fixture.handle.active_sessions().await;
    assert!(sessions_result.is_ok());
    let sessions = sessions_result.unwrap();
    assert_eq!(sessions.len(), 4);

    // Start downloads for all torrents
    for info_hash in &info_hashes {
        let result = fixture.handle.start_download(*info_hash).await;
        assert!(result.is_ok());
    }

    // Allow time for downloads to start
    tokio::time::sleep(Duration::from_millis(50)).await;

    // Verify all are downloading
    for info_hash in &info_hashes {
        let session_result = fixture.handle.session_details(*info_hash).await;
        assert!(session_result.is_ok());
        let session = session_result.unwrap();
        assert!(session.is_downloading);
    }

    // Simulate different completion rates
    let completion_rates = [0.2, 0.5, 0.8, 1.0]; // 20%, 50%, 80%, 100%

    for (i, info_hash) in info_hashes.iter().enumerate() {
        let session_result = fixture.handle.session_details(*info_hash).await;
        assert!(session_result.is_ok());
        let session = session_result.unwrap();
        let pieces_to_complete = (session.piece_count as f32 * completion_rates[i]) as u32;
        let pieces: Vec<u32> = (0..pieces_to_complete).collect();

        let result = fixture
            .handle
            .mark_pieces_completed(*info_hash, pieces)
            .await;
        assert!(result.is_ok());
    }

    // Allow time for progress updates
    tokio::time::sleep(Duration::from_millis(50)).await;

    // Verify individual progress
    for (i, info_hash) in info_hashes.iter().enumerate() {
        let session_result = fixture.handle.session_details(*info_hash).await;
        assert!(session_result.is_ok());
        let session = session_result.unwrap();
        assert!((session.progress - completion_rates[i]).abs() < 0.01);
    }

    // Verify aggregate statistics
    let stats_result = fixture.handle.download_statistics().await;
    assert!(stats_result.is_ok());
    let stats = stats_result.unwrap();
    assert_eq!(stats.active_torrents, 4);
    let expected_avg = completion_rates.iter().sum::<f32>() / completion_rates.len() as f32;
    assert!((stats.average_progress - expected_avg).abs() < 0.01);
}

#[tokio::test]
async fn test_streaming_integration() {
    let fixture = EngineTestFixture::new();
    let info_hash = InfoHash::new([1u8; 20]);
    let metadata = fixture.create_test_metadata(info_hash, "streaming_movie.mp4", 1000);

    // Add and start torrent
    fixture.handle.add_torrent_metadata(metadata).await.unwrap();
    fixture.handle.start_download(info_hash).await.unwrap();

    // Allow time for download to start
    tokio::time::sleep(Duration::from_millis(10)).await;

    // Simulate streaming seek
    let seek_result = fixture
        .handle
        .seek_to_position(info_hash, 1_000_000, 10_000_000)
        .await;
    assert!(seek_result.is_ok());

    // Update streaming position
    let update_result = fixture
        .handle
        .update_streaming_position(info_hash, 2_000_000)
        .await;
    assert!(update_result.is_ok());

    // Test buffer strategy
    let buffer_result = fixture
        .handle
        .update_buffer_strategy(info_hash, 1.5, 5_000_000)
        .await;
    assert!(buffer_result.is_ok());

    // Check buffer status
    let status_result = fixture.handle.buffer_status(info_hash).await;
    assert!(status_result.is_ok());
    let status = status_result.unwrap();
    assert!(status.buffer_health >= 0.0);
}

#[tokio::test]
async fn test_error_handling_and_recovery() {
    let fixture = EngineTestFixture::new_with_failures();
    let info_hash = InfoHash::new([1u8; 20]);
    let metadata = fixture.create_test_metadata(info_hash, "error_test.mp4", 10);

    // Add torrent should succeed
    let result = fixture.handle.add_torrent_metadata(metadata).await;
    assert!(result.is_ok());

    // Start download should handle tracker failures gracefully
    let start_result = fixture.handle.start_download(info_hash).await;
    // Should still succeed despite tracker failures due to fallback mechanisms
    assert!(start_result.is_ok());

    // Test operations on non-existent torrents
    let non_existent = InfoHash::new([99u8; 20]);

    let start_result = fixture.handle.start_download(non_existent).await;
    assert!(matches!(
        start_result,
        Err(TorrentError::TorrentNotFound { .. })
    ));

    let stop_result = fixture.handle.stop_download(non_existent).await;
    assert!(matches!(
        stop_result,
        Err(TorrentError::TorrentNotFound { .. })
    ));

    let mark_result = fixture
        .handle
        .mark_pieces_completed(non_existent, vec![0])
        .await;
    assert!(matches!(
        mark_result,
        Err(TorrentError::TorrentNotFound { .. })
    ));
}

#[tokio::test]
async fn test_concurrent_operations() {
    let fixture = EngineTestFixture::new();
    let info_hash = InfoHash::new([1u8; 20]);
    let metadata = fixture.create_test_metadata(info_hash, "concurrent_test.mp4", 50);

    fixture.handle.add_torrent_metadata(metadata).await.unwrap();

    // Start concurrent operations
    let start_future = fixture.handle.start_download(info_hash);
    let seek_future = fixture
        .handle
        .seek_to_position(info_hash, 100_000, 1_000_000);
    let update_future = fixture.handle.update_streaming_position(info_hash, 200_000);

    // All operations should complete successfully
    let (start_result, seek_result, update_result) =
        tokio::join!(start_future, seek_future, update_future);

    assert!(start_result.is_ok());
    assert!(seek_result.is_ok());
    assert!(update_result.is_ok());

    // Allow time for operations to complete
    tokio::time::sleep(Duration::from_millis(10)).await;

    // Verify final state
    let session_result = fixture.handle.session_details(info_hash).await;
    assert!(session_result.is_ok());
    let session = session_result.unwrap();
    assert!(session.is_downloading);
}

#[tokio::test]
async fn test_resource_cleanup() {
    let fixture = EngineTestFixture::new();
    let info_hash = InfoHash::new([1u8; 20]);
    let metadata = fixture.create_test_metadata(info_hash, "cleanup_test.mp4", 10);

    // Add and start torrent
    fixture.handle.add_torrent_metadata(metadata).await.unwrap();
    fixture.handle.start_download(info_hash).await.unwrap();

    // Allow time for download to start
    tokio::time::sleep(Duration::from_millis(10)).await;

    // Verify download is active
    let session_result = fixture.handle.session_details(info_hash).await;
    assert!(session_result.is_ok());
    assert!(session_result.unwrap().is_downloading);

    // Stop download
    let stop_result = fixture.handle.stop_download(info_hash).await;
    assert!(stop_result.is_ok());

    // Allow time for cleanup
    tokio::time::sleep(Duration::from_millis(10)).await;

    // Verify cleanup
    let session_result = fixture.handle.session_details(info_hash).await;
    assert!(session_result.is_ok());
    let session = session_result.unwrap();
    assert!(!session.is_downloading);
}

#[tokio::test]
async fn test_invalid_magnet_links() {
    let fixture = EngineTestFixture::new();

    let invalid_magnets = vec![
        "not-a-magnet-link",
        "magnet:?invalid=format",
        "magnet:?xt=urn:btih:invalid-hash",
        "magnet:?xt=urn:btih:tooshort",
        "",
    ];

    for invalid_magnet in invalid_magnets {
        let result = fixture.handle.add_magnet(invalid_magnet).await;
        assert!(
            result.is_err(),
            "Should fail for invalid magnet: {invalid_magnet}"
        );
    }
}

#[tokio::test]
async fn test_piece_validation() {
    let fixture = EngineTestFixture::new();
    let info_hash = InfoHash::new([1u8; 20]);
    let metadata = fixture.create_test_metadata(info_hash, "validation_test.mp4", 5);

    fixture.handle.add_torrent_metadata(metadata).await.unwrap();

    // Test valid piece indices
    let valid_pieces = vec![0, 1, 2, 3, 4];
    let result = fixture
        .handle
        .mark_pieces_completed(info_hash, valid_pieces)
        .await;
    assert!(result.is_ok());

    // Test invalid piece indices
    let invalid_pieces = vec![5, 6, 100];
    let result = fixture
        .handle
        .mark_pieces_completed(info_hash, invalid_pieces)
        .await;
    assert!(matches!(
        result,
        Err(TorrentError::InvalidPieceIndex { .. })
    ));

    // Test mixed valid/invalid pieces
    let mixed_pieces = vec![0, 1, 99];
    let result = fixture
        .handle
        .mark_pieces_completed(info_hash, mixed_pieces)
        .await;
    assert!(matches!(
        result,
        Err(TorrentError::InvalidPieceIndex { .. })
    ));
}

#[tokio::test]
async fn test_download_statistics_accuracy() {
    let fixture = EngineTestFixture::new();

    // Create torrents with different sizes
    let torrents = vec![
        (InfoHash::new([1u8; 20]), "small.mp4", 10),
        (InfoHash::new([2u8; 20]), "medium.mp4", 50),
        (InfoHash::new([3u8; 20]), "large.mp4", 100),
    ];

    for (info_hash, name, piece_count) in &torrents {
        let metadata = fixture.create_test_metadata(*info_hash, name, *piece_count);
        fixture.handle.add_torrent_metadata(metadata).await.unwrap();
    }

    // Complete different amounts
    fixture
        .handle
        .mark_pieces_completed(torrents[0].0, vec![0, 1, 2, 3, 4])
        .await
        .unwrap(); // 50%
    fixture
        .handle
        .mark_pieces_completed(torrents[1].0, vec![0, 1, 2, 3, 4, 5, 6, 7, 8, 9])
        .await
        .unwrap(); // 20%
    fixture
        .handle
        .mark_pieces_completed(torrents[2].0, (0..25).collect())
        .await
        .unwrap(); // 25%

    // Allow time for statistics to update
    tokio::time::sleep(Duration::from_millis(10)).await;

    let stats_result = fixture.handle.download_statistics().await;
    assert!(stats_result.is_ok());
    let stats = stats_result.unwrap();
    assert_eq!(stats.active_torrents, 3);

    // Calculate expected values
    let expected_bytes = (5 + 10 + 25) * 32768; // completed pieces * piece size
    assert_eq!(stats.bytes_downloaded, expected_bytes);

    let expected_avg = (0.5 + 0.2 + 0.25) / 3.0;
    assert!((stats.average_progress - expected_avg).abs() < 0.01);
}

#[tokio::test]
async fn test_engine_shutdown_graceful() {
    let fixture = EngineTestFixture::new();
    let info_hash = InfoHash::new([1u8; 20]);
    let metadata = fixture.create_test_metadata(info_hash, "shutdown_test.mp4", 10);

    // Add and start torrent
    fixture.handle.add_torrent_metadata(metadata).await.unwrap();
    fixture.handle.start_download(info_hash).await.unwrap();

    // Allow time for download to start
    tokio::time::sleep(Duration::from_millis(10)).await;

    // Verify download is active
    let session_result = fixture.handle.session_details(info_hash).await;
    assert!(session_result.is_ok());
    assert!(session_result.unwrap().is_downloading);

    // Engine should shut down gracefully when handle is dropped
    drop(fixture.handle);

    // Allow time for shutdown
    tokio::time::sleep(Duration::from_millis(50)).await;

    // Test passes if no panic occurs during shutdown
}

#[tokio::test]
async fn test_command_timeout_handling() {
    let fixture = EngineTestFixture::new();
    let info_hash = InfoHash::new([1u8; 20]);

    // Test that commands don't hang indefinitely
    let result = timeout(
        Duration::from_secs(1),
        fixture.handle.session_details(info_hash),
    )
    .await;

    // Should complete within timeout (even if it's an error)
    assert!(result.is_ok());

    // The actual result should be a TorrentNotFound error
    let session_result = result.unwrap();
    assert!(matches!(
        session_result,
        Err(TorrentError::TorrentNotFound { .. })
    ));
}
