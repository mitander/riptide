//! Integration tests for HTTP range handler integration with adaptive piece picker.

use riptide_core::config::RiptideConfig;
use riptide_core::engine::{MockPeerManager, MockTrackerManager, spawn_torrent_engine};
use riptide_core::torrent::InfoHash;
use riptide_core::torrent::parsing::types::{TorrentFile, TorrentMetadata};
use sha1::{Digest, Sha1};

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

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_range_request_updates_piece_picker_position() {
        let config = RiptideConfig::default();
        let mut peer_manager = MockPeerManager::new();
        peer_manager.enable_piece_data_simulation();
        let mut tracker_manager = MockTrackerManager::new();
        tracker_manager.configure_mock_peers(vec!["127.0.0.1:8080".parse().unwrap()]);

        let handle = spawn_torrent_engine(config, peer_manager, tracker_manager);

        // Create a larger test torrent for meaningful range testing
        let piece_count = 100;
        let piece_size = 32_768u32; // 32KB pieces
        let total_size = piece_count as u64 * piece_size as u64;
        let piece_hashes = generate_test_piece_hashes(piece_count, piece_size);

        let metadata = TorrentMetadata {
            info_hash: InfoHash::new([99u8; 20]),
            name: "adaptive_movie.mp4".to_string(),
            total_length: total_size,
            piece_length: piece_size,
            piece_hashes,
            files: vec![TorrentFile {
                path: vec!["adaptive_movie.mp4".to_string()],
                length: total_size,
            }],
            announce_urls: vec!["http://tracker.example.com/announce".to_string()],
        };

        let info_hash = handle.add_torrent_metadata(metadata).await.unwrap();
        handle.start_download(info_hash).await.unwrap();

        // Wait for download to initialize
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

        // Test initial buffer status (should be at position 0)
        let initial_status = handle.buffer_status(info_hash).await.unwrap();
        assert_eq!(initial_status.current_position, 0);

        // Simulate a range request for the middle of the file (simulating HTTP Range header processing)
        let seek_position = 50 * piece_size as u64; // Seek to piece 50
        let buffer_size = 5 * piece_size as u64; // Request 5-piece buffer

        // This simulates what the HTTP range handler would do
        handle
            .seek_to_position(info_hash, seek_position, buffer_size)
            .await
            .unwrap();

        // Verify the position was updated
        let updated_status = handle.buffer_status(info_hash).await.unwrap();
        assert_eq!(updated_status.current_position, seek_position);

        // Test buffer strategy updates (simulating bandwidth estimation from range size)
        let estimated_speed = 1.2; // Simulated playback speed
        let estimated_bandwidth = 3_000_000; // 3 MB/s estimated bandwidth

        handle
            .update_buffer_strategy(info_hash, estimated_speed, estimated_bandwidth)
            .await
            .unwrap();

        // Test seeking to different positions to verify piece picker adaptation
        let positions_to_test = vec![
            10 * piece_size as u64, // Beginning of file
            75 * piece_size as u64, // Near end
            25 * piece_size as u64, // Back to earlier position (seeking)
        ];

        for position in positions_to_test {
            handle
                .seek_to_position(info_hash, position, buffer_size)
                .await
                .unwrap();

            let status = handle.buffer_status(info_hash).await.unwrap();
            assert_eq!(status.current_position, position);

            // Verify buffer health metrics are reasonable
            assert!(status.buffer_health >= 0.0 && status.buffer_health <= 1.0);
            assert!(status.buffer_duration >= 0.0);
        }

        handle.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn test_adaptive_buffer_responds_to_playback_patterns() {
        let config = RiptideConfig::default();
        let mut peer_manager = MockPeerManager::new();
        peer_manager.enable_piece_data_simulation();
        let mut tracker_manager = MockTrackerManager::new();
        tracker_manager.configure_mock_peers(vec!["127.0.0.1:8080".parse().unwrap()]);

        let handle = spawn_torrent_engine(config, peer_manager, tracker_manager);

        // Create test torrent
        let piece_count = 200;
        let piece_size = 16_384u32; // 16KB pieces for finer granularity
        let total_size = piece_count as u64 * piece_size as u64;
        let piece_hashes = generate_test_piece_hashes(piece_count, piece_size);

        let metadata = TorrentMetadata {
            info_hash: InfoHash::new([123u8; 20]),
            name: "adaptive_streaming_test.mkv".to_string(),
            total_length: total_size,
            piece_length: piece_size,
            piece_hashes,
            files: vec![TorrentFile {
                path: vec!["adaptive_streaming_test.mkv".to_string()],
                length: total_size,
            }],
            announce_urls: vec!["http://tracker.example.com/announce".to_string()],
        };

        let info_hash = handle.add_torrent_metadata(metadata).await.unwrap();
        handle.start_download(info_hash).await.unwrap();

        // Test different streaming scenarios that would come from HTTP range requests

        // Scenario 1: Normal playback (sequential small ranges)
        for i in 0..10 {
            let position = i * 2 * piece_size as u64; // Move forward by 2 pieces each time
            handle
                .seek_to_position(info_hash, position, piece_size as u64)
                .await
                .unwrap();

            // Update with normal playback characteristics
            handle
                .update_buffer_strategy(info_hash, 1.0, 2_000_000)
                .await
                .unwrap();

            let status = handle.buffer_status(info_hash).await.unwrap();
            assert_eq!(status.current_position, position);
        }

        // Scenario 2: Fast seeking (large jumps)
        let seek_positions = vec![
            50 * piece_size as u64,  // Jump to middle
            150 * piece_size as u64, // Jump to near end
            25 * piece_size as u64,  // Jump back
        ];

        for position in seek_positions {
            handle
                .seek_to_position(info_hash, position, 8 * piece_size as u64) // Larger buffer for seeks
                .await
                .unwrap();

            // Update with seeking characteristics (higher speed, higher bandwidth)
            handle
                .update_buffer_strategy(info_hash, 2.0, 5_000_000)
                .await
                .unwrap();

            let status = handle.buffer_status(info_hash).await.unwrap();
            assert_eq!(status.current_position, position);
        }

        // Scenario 3: Slow/paused playback (very small ranges)
        let pause_position = 100 * piece_size as u64;
        handle
            .seek_to_position(info_hash, pause_position, piece_size as u64 / 2) // Small buffer
            .await
            .unwrap();

        // Update with slow playback characteristics
        handle
            .update_buffer_strategy(info_hash, 0.5, 1_000_000)
            .await
            .unwrap();

        let final_status = handle.buffer_status(info_hash).await.unwrap();
        assert_eq!(final_status.current_position, pause_position);

        // Verify buffer metrics are still reasonable
        assert!(final_status.buffer_health >= 0.0 && final_status.buffer_health <= 1.0);

        handle.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn test_concurrent_range_requests_different_torrents() {
        let config = RiptideConfig::default();
        let mut peer_manager = MockPeerManager::new();
        peer_manager.enable_piece_data_simulation();
        let mut tracker_manager = MockTrackerManager::new();
        tracker_manager.configure_mock_peers(vec!["127.0.0.1:8080".parse().unwrap()]);

        let handle = spawn_torrent_engine(config, peer_manager, tracker_manager);

        // Create multiple test torrents to simulate concurrent streaming
        let torrents = vec![
            ("movie1.mp4", [1u8; 20]),
            ("movie2.mkv", [2u8; 20]),
            ("series_episode.avi", [3u8; 20]),
        ];

        let mut info_hashes = Vec::new();

        for (name, hash_bytes) in torrents {
            let piece_count = 80;
            let piece_size = 65_536u32; // 64KB pieces
            let total_size = piece_count as u64 * piece_size as u64;
            let piece_hashes = generate_test_piece_hashes(piece_count, piece_size);

            let metadata = TorrentMetadata {
                info_hash: InfoHash::new(hash_bytes),
                name: name.to_string(),
                total_length: total_size,
                piece_length: piece_size,
                piece_hashes: piece_hashes.clone(),
                files: vec![TorrentFile {
                    path: vec![name.to_string()],
                    length: total_size,
                }],
                announce_urls: vec!["http://tracker.example.com/announce".to_string()],
            };

            let info_hash = handle.add_torrent_metadata(metadata).await.unwrap();
            handle.start_download(info_hash).await.unwrap();
            info_hashes.push(info_hash);
        }

        // Simulate concurrent HTTP range requests from multiple video players
        for (i, &info_hash) in info_hashes.iter().enumerate() {
            let base_position = (i as u64 + 1) * 10 * 65_536; // Different starting positions
            let playback_speed = 1.0 + (i as f64 * 0.3); // Different playback speeds
            let bandwidth = 1_500_000 + (i as u64 * 500_000); // Different bandwidths

            // Simulate range request processing
            let seek_future = handle.seek_to_position(info_hash, base_position, 3 * 65_536);
            let buffer_future = handle.update_buffer_strategy(info_hash, playback_speed, bandwidth);

            // Execute both operations concurrently
            let (seek_result, buffer_result) = tokio::join!(seek_future, buffer_future);

            seek_result.unwrap();
            buffer_result.unwrap();

            // Verify the updates were applied correctly
            let status = handle.buffer_status(info_hash).await.unwrap();
            assert_eq!(status.current_position, base_position);
        }

        // Test that each torrent maintains its own independent state
        for (i, &info_hash) in info_hashes.iter().enumerate() {
            let expected_position = (i as u64 + 1) * 10 * 65_536;
            let status = handle.buffer_status(info_hash).await.unwrap();
            assert_eq!(
                status.current_position, expected_position,
                "Torrent {i} should maintain independent position"
            );
        }

        handle.shutdown().await.unwrap();
    }
}
