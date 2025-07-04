//! Integration tests for streaming and buffering functionality.

#[cfg(test)]
mod tests {
    use sha1::{Digest, Sha1};

    use crate::config::RiptideConfig;
    use crate::engine::spawn_torrent_engine;
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
    async fn test_end_to_end_seeking_functionality() {
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

        // Create test torrent metadata with enough pieces for meaningful seeking
        let piece_count = 20;
        let piece_size = 32_768u32; // 32KB pieces
        let total_size = piece_count as u64 * piece_size as u64;
        let piece_hashes = generate_test_piece_hashes(piece_count, piece_size);

        let metadata = crate::torrent::parsing::types::TorrentMetadata {
            info_hash: InfoHash::new([42u8; 20]),
            name: "streaming_test.mp4".to_string(),
            total_length: total_size,
            piece_length: piece_size,
            piece_hashes,
            files: vec![crate::torrent::parsing::types::TorrentFile {
                path: vec!["streaming_test.mp4".to_string()],
                length: total_size,
            }],
            announce_urls: vec!["http://tracker.example.com/announce".to_string()],
        };

        // Add torrent and start download
        let info_hash = handle.add_torrent_metadata(metadata).await.unwrap();
        handle.start_download(info_hash).await.unwrap();

        // Wait a moment for download to start
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

        // Test initial buffer status
        let initial_status = handle.get_buffer_status(info_hash).await.unwrap();
        assert_eq!(initial_status.current_position, 0);

        // Test seeking to middle of file
        let seek_position = 10 * piece_size as u64; // Seek to piece 10
        let buffer_size = 3 * piece_size as u64; // 3-piece buffer

        handle
            .seek_to_position(info_hash, seek_position, buffer_size)
            .await
            .unwrap();

        // Test buffer strategy update
        handle
            .update_buffer_strategy(info_hash, 1.5, 2_000_000)
            .await
            .unwrap(); // 1.5x speed, 2MB/s

        // Get updated buffer status
        let updated_status = handle.get_buffer_status(info_hash).await.unwrap();
        assert_eq!(updated_status.current_position, seek_position);

        // Test seeking to end of file
        let end_seek_position = 18 * piece_size as u64; // Near end
        handle
            .seek_to_position(info_hash, end_seek_position, buffer_size)
            .await
            .unwrap();

        let end_status = handle.get_buffer_status(info_hash).await.unwrap();
        assert_eq!(end_status.current_position, end_seek_position);

        // Test error handling for non-existent torrent
        let fake_hash = InfoHash::new([99u8; 20]);
        let seek_result = handle.seek_to_position(fake_hash, 0, buffer_size).await;
        assert!(matches!(
            seek_result,
            Err(TorrentError::TorrentNotFound { .. })
        ));

        let buffer_result = handle.get_buffer_status(fake_hash).await;
        assert!(matches!(
            buffer_result,
            Err(TorrentError::TorrentNotFound { .. })
        ));

        handle.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn test_adaptive_buffering_under_different_conditions() {
        let config = RiptideConfig::default();
        let mut peer_manager = crate::torrent::MockPeerManager::new();
        peer_manager.enable_piece_data_simulation();
        let mut tracker_manager = crate::torrent::MockTrackerManager::new();
        tracker_manager.set_mock_peers(vec!["127.0.0.1:8080".parse().unwrap()]);

        let handle = spawn_torrent_engine(config, peer_manager, tracker_manager);

        // Create test torrent
        let piece_count = 50;
        let piece_size = 16_384u32; // 16KB pieces
        let total_size = piece_count as u64 * piece_size as u64;
        let piece_hashes = generate_test_piece_hashes(piece_count, piece_size);

        let metadata = crate::torrent::parsing::types::TorrentMetadata {
            info_hash: InfoHash::new([123u8; 20]),
            name: "adaptive_test.mkv".to_string(),
            total_length: total_size,
            piece_length: piece_size,
            piece_hashes,
            files: vec![crate::torrent::parsing::types::TorrentFile {
                path: vec!["adaptive_test.mkv".to_string()],
                length: total_size,
            }],
            announce_urls: vec!["http://tracker.example.com/announce".to_string()],
        };

        let info_hash = handle.add_torrent_metadata(metadata).await.unwrap();
        handle.start_download(info_hash).await.unwrap();

        // Test different playback speeds and bandwidths
        let test_conditions = vec![
            (1.0, 1_000_000), // Normal playback, 1 MB/s
            (1.5, 2_000_000), // Faster playback, 2 MB/s
            (0.5, 500_000),   // Slower playback, 0.5 MB/s
            (2.0, 500_000),   // Fast playback, low bandwidth
        ];

        for (speed, bandwidth) in test_conditions {
            // Update buffer strategy
            handle
                .update_buffer_strategy(info_hash, speed, bandwidth)
                .await
                .unwrap();

            // Get buffer status
            let status = handle.get_buffer_status(info_hash).await.unwrap();

            // Verify buffer status is reasonable
            assert!(status.buffer_health >= 0.0);
            assert!(status.buffer_health <= 1.0);
            assert!(status.buffer_duration >= 0.0);

            // Seek to different position to test adaptive behavior
            let seek_pos = (speed * 10.0 * piece_size as f64) as u64;
            handle
                .seek_to_position(info_hash, seek_pos, 5 * piece_size as u64)
                .await
                .unwrap();
        }

        handle.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn test_multiple_concurrent_streams() {
        let config = RiptideConfig::default();
        let mut peer_manager = crate::torrent::MockPeerManager::new();
        peer_manager.enable_piece_data_simulation();
        let mut tracker_manager = crate::torrent::MockTrackerManager::new();
        tracker_manager.set_mock_peers(vec!["127.0.0.1:8080".parse().unwrap()]);

        let handle = spawn_torrent_engine(config, peer_manager, tracker_manager);

        // Create multiple test torrents
        let torrents = vec![
            ("stream1.mp4", [1u8; 20]),
            ("stream2.mkv", [2u8; 20]),
            ("stream3.avi", [3u8; 20]),
        ];

        let mut info_hashes = Vec::new();

        for (name, hash_bytes) in torrents {
            let piece_count = 15;
            let piece_size = 65_536u32; // 64KB pieces
            let total_size = piece_count as u64 * piece_size as u64;
            let piece_hashes = generate_test_piece_hashes(piece_count, piece_size);

            let metadata = crate::torrent::parsing::types::TorrentMetadata {
                info_hash: InfoHash::new(hash_bytes),
                name: name.to_string(),
                total_length: total_size,
                piece_length: piece_size,
                piece_hashes,
                files: vec![crate::torrent::parsing::types::TorrentFile {
                    path: vec![name.to_string()],
                    length: total_size,
                }],
                announce_urls: vec!["http://tracker.example.com/announce".to_string()],
            };

            let info_hash = handle.add_torrent_metadata(metadata).await.unwrap();
            handle.start_download(info_hash).await.unwrap();
            info_hashes.push(info_hash);
        }

        // Test concurrent operations on all torrents
        for (i, &info_hash) in info_hashes.iter().enumerate() {
            let seek_position = (i as u64 + 1) * 3 * 65_536; // Different seek positions
            let playback_speed = 1.0 + (i as f64 * 0.5); // Different speeds

            // Perform concurrent seeks and buffer updates
            let seek_future = handle.seek_to_position(info_hash, seek_position, 2 * 65_536);
            let buffer_future = handle.update_buffer_strategy(info_hash, playback_speed, 1_500_000);
            let status_future = handle.get_buffer_status(info_hash);

            // Wait for all operations to complete
            let (seek_result, buffer_result, status_result) =
                tokio::join!(seek_future, buffer_future, status_future);

            seek_result.unwrap();
            buffer_result.unwrap();
            let status = status_result.unwrap();

            assert_eq!(status.current_position, seek_position);
            assert!(status.buffer_health >= 0.0);
        }

        handle.shutdown().await.unwrap();
    }
}
