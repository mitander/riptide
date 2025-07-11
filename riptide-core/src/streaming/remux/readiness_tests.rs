//! Comprehensive tests for remux session manager readiness logic
//!
//! These tests focus on debugging the readiness check that gets stuck at 90% progress
//! in the UI, specifically the "Building buffer: Preparing stream metadata" message.

use std::collections::HashMap;
use std::ops::Range;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use tokio::sync::RwLock;

use super::session_manager::RemuxSessionManager;
use super::types::{RemuxConfig, StreamReadiness};
use crate::storage::data_source::{
    CacheStats, CacheableDataSource, DataError, DataResult, DataSource, RangeAvailability,
};
use crate::torrent::InfoHash;

/// Mock data source for testing readiness scenarios
#[derive(Clone)]
struct MockDataSource {
    files: Arc<RwLock<HashMap<InfoHash, MockFile>>>,
}

#[derive(Clone)]
struct MockFile {
    size: u64,
    available_ranges: Vec<Range<u64>>,
    data: Vec<u8>,
}

impl MockDataSource {
    fn new() -> Self {
        Self {
            files: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    async fn add_file(&self, info_hash: InfoHash, size: u64) {
        let mut files = self.files.write().await;
        files.insert(
            info_hash,
            MockFile {
                size,
                available_ranges: Vec::new(),
                data: vec![0u8; size as usize],
            },
        );
    }

    async fn add_range(&self, info_hash: InfoHash, range: Range<u64>) {
        let mut files = self.files.write().await;
        if let Some(file) = files.get_mut(&info_hash) {
            file.available_ranges.push(range);
        }
    }

    async fn set_head_available(&self, info_hash: InfoHash, size: u64) {
        self.add_range(info_hash, 0..size).await;
    }

    async fn set_tail_available(&self, info_hash: InfoHash, file_size: u64, tail_size: u64) {
        let start = file_size.saturating_sub(tail_size);
        self.add_range(info_hash, start..file_size).await;
    }

    async fn set_full_file_available(&self, info_hash: InfoHash) {
        let files = self.files.read().await;
        if let Some(file) = files.get(&info_hash) {
            let file_size = file.size;
            drop(files);
            self.add_range(info_hash, 0..file_size).await;
        }
    }

    fn is_range_available(&self, file: &MockFile, range: &Range<u64>) -> bool {
        file.available_ranges
            .iter()
            .any(|available| available.start <= range.start && available.end >= range.end)
    }
}

#[async_trait]
impl DataSource for MockDataSource {
    async fn read_range(&self, info_hash: InfoHash, range: Range<u64>) -> DataResult<Vec<u8>> {
        let files = self.files.read().await;
        let file = files.get(&info_hash).ok_or(DataError::Storage {
            reason: "File not found".to_string(),
        })?;

        if !self.is_range_available(file, &range) {
            return Err(DataError::InsufficientData {
                start: range.start,
                end: range.end,
                missing_count: 1,
            });
        }

        let start = range.start as usize;
        let end = range.end.min(file.size) as usize;
        Ok(file.data[start..end].to_vec())
    }

    async fn file_size(&self, info_hash: InfoHash) -> DataResult<u64> {
        let files = self.files.read().await;
        let file = files.get(&info_hash).ok_or(DataError::Storage {
            reason: "File not found".to_string(),
        })?;
        Ok(file.size)
    }

    async fn check_range_availability(
        &self,
        info_hash: InfoHash,
        range: Range<u64>,
    ) -> DataResult<RangeAvailability> {
        let files = self.files.read().await;
        let file = files.get(&info_hash).ok_or(DataError::Storage {
            reason: "File not found".to_string(),
        })?;

        let available = self.is_range_available(file, &range);
        Ok(RangeAvailability {
            available,
            missing_pieces: if available { Vec::new() } else { vec![0] },
            cache_hit: false,
        })
    }

    fn source_type(&self) -> &'static str {
        "mock_data_source"
    }

    async fn can_handle(&self, info_hash: InfoHash) -> bool {
        let files = self.files.read().await;
        files.contains_key(&info_hash)
    }
}

#[async_trait]
impl CacheableDataSource for MockDataSource {
    async fn cache_stats(&self) -> CacheStats {
        CacheStats {
            hits: 0,
            misses: 0,
            evictions: 0,
            memory_usage: 0,
            entry_count: 0,
        }
    }

    async fn clear_cache(&self, _info_hash: InfoHash) -> DataResult<()> {
        Ok(())
    }

    async fn clear_all_cache(&self) -> DataResult<()> {
        Ok(())
    }
}

fn create_test_config() -> RemuxConfig {
    RemuxConfig {
        max_concurrent_sessions: 1,
        cache_dir: PathBuf::from("/tmp/test_remux"),
        min_head_size: 3 * 1024 * 1024, // 3MB
        min_tail_size: 2 * 1024 * 1024, // 2MB
        remux_timeout: Duration::from_secs(30),
        ffmpeg_path: PathBuf::from("echo"), // Use echo for testing
        cleanup_after: Duration::from_secs(3600),
    }
}

fn create_small_file_config() -> RemuxConfig {
    RemuxConfig {
        max_concurrent_sessions: 1,
        cache_dir: PathBuf::from("/tmp/test_remux"),
        min_head_size: 1024, // 1KB
        min_tail_size: 1024, // 1KB
        remux_timeout: Duration::from_secs(30),
        ffmpeg_path: PathBuf::from("echo"), // Use echo for testing
        cleanup_after: Duration::from_secs(3600),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_readiness_with_head_only_data() {
        let config = create_test_config();
        let data_source = MockDataSource::new();
        let info_hash = InfoHash::new([1u8; 20]);
        let file_size = 100 * 1024 * 1024; // 100MB file

        // Add file with only head data (common in torrent downloads)
        data_source.add_file(info_hash, file_size).await;
        data_source
            .set_head_available(info_hash, 5 * 1024 * 1024)
            .await; // 5MB head

        let manager = RemuxSessionManager::new(
            config,
            Arc::new(data_source),
            Arc::new(crate::streaming::SimulationFfmpegProcessor::new()),
        );
        let _handle = manager.get_or_create_session(info_hash).await.unwrap();

        // With head-only strategy, should now progress to Processing with sufficient head data
        let readiness = manager.check_readiness(info_hash).await.unwrap();
        assert_eq!(readiness, StreamReadiness::Processing);

        // Status should show we're processing
        let status = manager.get_status(info_hash).await.unwrap();
        assert_eq!(status.readiness, StreamReadiness::Processing);
    }

    #[tokio::test]
    async fn test_readiness_progresses_with_head_and_tail() {
        let config = create_test_config();
        let data_source = MockDataSource::new();
        let info_hash = InfoHash::new([2u8; 20]);
        let file_size = 100 * 1024 * 1024; // 100MB file

        // Add file with both head and tail data
        data_source.add_file(info_hash, file_size).await;
        data_source
            .set_head_available(info_hash, 5 * 1024 * 1024)
            .await; // 5MB head
        data_source
            .set_tail_available(info_hash, file_size, 3 * 1024 * 1024)
            .await; // 3MB tail

        let manager = RemuxSessionManager::new(
            config,
            Arc::new(data_source),
            Arc::new(crate::streaming::SimulationFfmpegProcessor::new()),
        );
        let _handle = manager.get_or_create_session(info_hash).await.unwrap();

        // This should progress to Processing because we have both head and tail
        let readiness = manager.check_readiness(info_hash).await.unwrap();
        assert_eq!(readiness, StreamReadiness::Processing);
    }

    #[tokio::test]
    async fn test_readiness_with_small_file() {
        let config = create_small_file_config();
        let data_source = MockDataSource::new();
        let info_hash = InfoHash::new([3u8; 20]);
        let file_size = 5 * 1024; // 5KB file

        // Add small file with full content
        data_source.add_file(info_hash, file_size).await;
        data_source.set_full_file_available(info_hash).await;

        let manager = RemuxSessionManager::new(
            config,
            Arc::new(data_source),
            Arc::new(crate::streaming::SimulationFfmpegProcessor::new()),
        );
        let _handle = manager.get_or_create_session(info_hash).await.unwrap();

        // Small file should be ready immediately
        let readiness = manager.check_readiness(info_hash).await.unwrap();
        assert_eq!(readiness, StreamReadiness::Processing);
    }

    #[tokio::test]
    async fn test_readiness_with_file_smaller_than_head_requirement() {
        let config = create_test_config();
        let data_source = MockDataSource::new();
        let info_hash = InfoHash::new([4u8; 20]);
        let file_size = 1024 * 1024; // 1MB file (smaller than 3MB head requirement)

        // Add file with full content
        data_source.add_file(info_hash, file_size).await;
        data_source.set_full_file_available(info_hash).await;

        let manager = RemuxSessionManager::new(
            config,
            Arc::new(data_source),
            Arc::new(crate::streaming::SimulationFfmpegProcessor::new()),
        );
        let _handle = manager.get_or_create_session(info_hash).await.unwrap();

        // Should be ready because file is smaller than head requirement
        let readiness = manager.check_readiness(info_hash).await.unwrap();
        assert_eq!(readiness, StreamReadiness::Processing);
    }

    #[tokio::test]
    async fn test_readiness_progression_over_time() {
        let config = create_test_config();
        let data_source = MockDataSource::new();
        let info_hash = InfoHash::new([5u8; 20]);
        let file_size = 100 * 1024 * 1024; // 100MB file

        // Start with just the file registered
        data_source.add_file(info_hash, file_size).await;

        let manager = RemuxSessionManager::new(
            config,
            Arc::new(data_source.clone()),
            Arc::new(crate::streaming::SimulationFfmpegProcessor::new()),
        );
        let _handle = manager.get_or_create_session(info_hash).await.unwrap();

        // Initially should be waiting for data
        let readiness = manager.check_readiness(info_hash).await.unwrap();
        assert_eq!(readiness, StreamReadiness::WaitingForData);

        // Add head data
        data_source
            .set_head_available(info_hash, 5 * 1024 * 1024)
            .await;

        // With head-only strategy, should progress to Processing once head is available
        let readiness = manager.check_readiness(info_hash).await.unwrap();
        assert_eq!(readiness, StreamReadiness::Processing);

        // Add tail data
        data_source
            .set_tail_available(info_hash, file_size, 3 * 1024 * 1024)
            .await;

        // Now should be processing
        let readiness = manager.check_readiness(info_hash).await.unwrap();
        assert_eq!(readiness, StreamReadiness::Processing);
    }

    #[tokio::test]
    async fn test_readiness_with_partial_head_data() {
        let config = create_test_config();
        let data_source = MockDataSource::new();
        let info_hash = InfoHash::new([6u8; 20]);
        let file_size = 100 * 1024 * 1024; // 100MB file

        // Add file with insufficient head data
        data_source.add_file(info_hash, file_size).await;
        data_source.set_head_available(info_hash, 1024 * 1024).await; // Only 1MB head
        data_source
            .set_tail_available(info_hash, file_size, 3 * 1024 * 1024)
            .await; // 3MB tail

        let manager = RemuxSessionManager::new(
            config,
            Arc::new(data_source),
            Arc::new(crate::streaming::SimulationFfmpegProcessor::new()),
        );
        let _handle = manager.get_or_create_session(info_hash).await.unwrap();

        // Should be waiting because head data is insufficient
        let readiness = manager.check_readiness(info_hash).await.unwrap();
        assert_eq!(readiness, StreamReadiness::WaitingForData);
    }

    #[tokio::test]
    async fn test_readiness_with_partial_tail_data() {
        let config = create_test_config();
        let data_source = MockDataSource::new();
        let info_hash = InfoHash::new([7u8; 20]);
        let file_size = 100 * 1024 * 1024; // 100MB file

        // Add file with insufficient tail data
        data_source.add_file(info_hash, file_size).await;
        data_source
            .set_head_available(info_hash, 5 * 1024 * 1024)
            .await; // 5MB head
        data_source
            .set_tail_available(info_hash, file_size, 1024 * 1024)
            .await; // Only 1MB tail

        let manager = RemuxSessionManager::new(
            config,
            Arc::new(data_source),
            Arc::new(crate::streaming::SimulationFfmpegProcessor::new()),
        );
        let _handle = manager.get_or_create_session(info_hash).await.unwrap();

        // With head-only strategy, should progress with sufficient head data regardless of tail
        let readiness = manager.check_readiness(info_hash).await.unwrap();
        assert_eq!(readiness, StreamReadiness::Processing);
    }

    #[tokio::test]
    async fn test_readiness_status_consistency() {
        let config = create_test_config();
        let data_source = MockDataSource::new();
        let info_hash = InfoHash::new([8u8; 20]);
        let file_size = 100 * 1024 * 1024; // 100MB file

        // Add file with no data
        data_source.add_file(info_hash, file_size).await;

        let manager = RemuxSessionManager::new(
            config,
            Arc::new(data_source),
            Arc::new(crate::streaming::SimulationFfmpegProcessor::new()),
        );
        let _handle = manager.get_or_create_session(info_hash).await.unwrap();

        // Both readiness and status should be consistent
        let readiness = manager.check_readiness(info_hash).await.unwrap();
        let status = manager.get_status(info_hash).await.unwrap();

        assert_eq!(readiness, StreamReadiness::WaitingForData);
        assert_eq!(status.readiness, StreamReadiness::WaitingForData);
    }

    #[tokio::test]
    async fn test_readiness_error_handling() {
        let config = create_test_config();
        let data_source = MockDataSource::new();
        let info_hash = InfoHash::new([9u8; 20]);

        // Don't add file to data source
        let manager = RemuxSessionManager::new(
            config,
            Arc::new(data_source),
            Arc::new(crate::streaming::SimulationFfmpegProcessor::new()),
        );
        let _handle = manager.get_or_create_session(info_hash).await.unwrap();

        // Should fail because file doesn't exist
        let result = manager.check_readiness(info_hash).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_readiness_multiple_sessions() {
        let config = create_test_config();
        let data_source = MockDataSource::new();
        let info_hash1 = InfoHash::new([10u8; 20]);
        let info_hash2 = InfoHash::new([11u8; 20]);
        let file_size = 100 * 1024 * 1024; // 100MB file

        // Set up first file with head and tail
        data_source.add_file(info_hash1, file_size).await;
        data_source
            .set_head_available(info_hash1, 5 * 1024 * 1024)
            .await;
        data_source
            .set_tail_available(info_hash1, file_size, 3 * 1024 * 1024)
            .await;

        // Set up second file with only head (sufficient for head-only strategy)
        data_source.add_file(info_hash2, file_size).await;
        data_source
            .set_head_available(info_hash2, 5 * 1024 * 1024)
            .await;

        let manager = RemuxSessionManager::new(
            config,
            Arc::new(data_source),
            Arc::new(crate::streaming::SimulationFfmpegProcessor::new()),
        );
        let _handle1 = manager.get_or_create_session(info_hash1).await.unwrap();
        let _handle2 = manager.get_or_create_session(info_hash2).await.unwrap();

        // Both should be processing with head-only strategy
        let readiness1 = manager.check_readiness(info_hash1).await.unwrap();
        let readiness2 = manager.check_readiness(info_hash2).await.unwrap();

        assert_eq!(readiness1, StreamReadiness::Processing);
        assert_eq!(readiness2, StreamReadiness::Processing);
    }

    #[tokio::test]
    async fn test_readiness_debug_stuck_at_90_percent() {
        // This test verifies the fix for the issue where remux was stuck at 90%
        let config = create_test_config();
        let data_source = MockDataSource::new();
        let info_hash = InfoHash::new([12u8; 20]);
        let file_size = 100 * 1024 * 1024; // 100MB file

        // Simulate torrent download with 90% progress (common scenario)
        data_source.add_file(info_hash, file_size).await;
        // Download progress: first 90MB available (includes sufficient head data)
        data_source
            .add_range(info_hash, 0..(90 * 1024 * 1024))
            .await;

        let manager = RemuxSessionManager::new(
            config,
            Arc::new(data_source),
            Arc::new(crate::streaming::SimulationFfmpegProcessor::new()),
        );
        let _handle = manager.get_or_create_session(info_hash).await.unwrap();

        // Fixed: Now with head-only strategy, remux starts with sufficient head data
        // This prevents the UI from being stuck at "Preparing stream metadata"
        let readiness = manager.check_readiness(info_hash).await.unwrap();
        assert_eq!(readiness, StreamReadiness::Processing);

        // Status should show we're processing, not stuck waiting
        let status = manager.get_status(info_hash).await.unwrap();
        assert_eq!(status.readiness, StreamReadiness::Processing);

        // Progress should be at some value (0.9 based on the original issue)
        assert!(status.progress.is_some());
    }

    #[tokio::test]
    async fn test_readiness_edge_case_exact_head_tail_requirements() {
        let config = create_test_config();
        let data_source = MockDataSource::new();
        let info_hash = InfoHash::new([13u8; 20]);
        let file_size = 100 * 1024 * 1024; // 100MB file

        // Add file with exactly the minimum required head and tail
        data_source.add_file(info_hash, file_size).await;
        data_source
            .set_head_available(info_hash, 3 * 1024 * 1024)
            .await; // Exactly 3MB head
        data_source
            .set_tail_available(info_hash, file_size, 2 * 1024 * 1024)
            .await; // Exactly 2MB tail

        let manager = RemuxSessionManager::new(
            config,
            Arc::new(data_source),
            Arc::new(crate::streaming::SimulationFfmpegProcessor::new()),
        );
        let _handle = manager.get_or_create_session(info_hash).await.unwrap();

        // Should be ready with exact requirements
        let readiness = manager.check_readiness(info_hash).await.unwrap();
        assert_eq!(readiness, StreamReadiness::Processing);
    }

    #[tokio::test]
    async fn test_readiness_edge_case_one_byte_short() {
        let config = create_test_config();
        let data_source = MockDataSource::new();
        let info_hash = InfoHash::new([14u8; 20]);
        let file_size = 100 * 1024 * 1024; // 100MB file

        // Add file with one byte less than required
        data_source.add_file(info_hash, file_size).await;
        data_source
            .set_head_available(info_hash, 3 * 1024 * 1024 - 1)
            .await; // 1 byte short
        data_source
            .set_tail_available(info_hash, file_size, 2 * 1024 * 1024)
            .await; // Exactly 2MB tail

        let manager = RemuxSessionManager::new(
            config,
            Arc::new(data_source),
            Arc::new(crate::streaming::SimulationFfmpegProcessor::new()),
        );
        let _handle = manager.get_or_create_session(info_hash).await.unwrap();

        // Should be waiting because head is 1 byte short
        let readiness = manager.check_readiness(info_hash).await.unwrap();
        assert_eq!(readiness, StreamReadiness::WaitingForData);
    }

    #[tokio::test]
    async fn test_head_only_remux_strategy() {
        // Test the new head-only remux strategy for streaming optimization
        let config = create_test_config();
        let data_source = MockDataSource::new();
        let info_hash = InfoHash::new([15u8; 20]);
        let file_size = 100 * 1024 * 1024; // 100MB file (large enough for head-only strategy)

        // Add file with only head data (no tail)
        data_source.add_file(info_hash, file_size).await;
        data_source
            .set_head_available(info_hash, 5 * 1024 * 1024)
            .await; // 5MB head (more than 3MB minimum)

        let manager = RemuxSessionManager::new(
            config,
            Arc::new(data_source),
            Arc::new(crate::streaming::SimulationFfmpegProcessor::new()),
        );
        let _handle = manager.get_or_create_session(info_hash).await.unwrap();

        // Should start processing with head-only data for streaming optimization
        let readiness = manager.check_readiness(info_hash).await.unwrap();
        assert_eq!(readiness, StreamReadiness::Processing);
    }

    #[tokio::test]
    async fn test_small_file_requires_full_content() {
        // Test that small files still require full content
        let config = create_test_config();
        let data_source = MockDataSource::new();
        let info_hash = InfoHash::new([16u8; 20]);
        let file_size = 5 * 1024 * 1024; // 5MB file (small file)

        // Add file with only head data
        data_source.add_file(info_hash, file_size).await;
        data_source
            .set_head_available(info_hash, 3 * 1024 * 1024)
            .await; // 3MB head

        let manager = RemuxSessionManager::new(
            config,
            Arc::new(data_source),
            Arc::new(crate::streaming::SimulationFfmpegProcessor::new()),
        );
        let _handle = manager.get_or_create_session(info_hash).await.unwrap();

        // Should still be waiting because small files need full content
        let readiness = manager.check_readiness(info_hash).await.unwrap();
        assert_eq!(readiness, StreamReadiness::WaitingForData);
    }

    #[tokio::test]
    async fn test_large_file_head_only_vs_traditional() {
        // Test comparison between head-only and traditional head+tail approach
        let config = create_test_config();
        let data_source = MockDataSource::new();
        let info_hash1 = InfoHash::new([17u8; 20]);
        let info_hash2 = InfoHash::new([18u8; 20]);
        let file_size = 100 * 1024 * 1024; // 100MB file

        // File 1: Only head data (new strategy should work)
        data_source.add_file(info_hash1, file_size).await;
        data_source
            .set_head_available(info_hash1, 5 * 1024 * 1024)
            .await;

        // File 2: Head + tail data (traditional approach)
        data_source.add_file(info_hash2, file_size).await;
        data_source
            .set_head_available(info_hash2, 5 * 1024 * 1024)
            .await;
        data_source
            .set_tail_available(info_hash2, file_size, 3 * 1024 * 1024)
            .await;

        let manager = RemuxSessionManager::new(
            config,
            Arc::new(data_source),
            Arc::new(crate::streaming::SimulationFfmpegProcessor::new()),
        );
        let _handle1 = manager.get_or_create_session(info_hash1).await.unwrap();
        let _handle2 = manager.get_or_create_session(info_hash2).await.unwrap();

        // Both should be ready to process
        let readiness1 = manager.check_readiness(info_hash1).await.unwrap();
        let readiness2 = manager.check_readiness(info_hash2).await.unwrap();

        assert_eq!(readiness1, StreamReadiness::Processing);
        assert_eq!(readiness2, StreamReadiness::Processing);
    }

    #[tokio::test]
    async fn test_insufficient_head_data_still_waits() {
        // Test that edge cases with insufficient head data still wait appropriately
        let config = create_test_config();
        let data_source = MockDataSource::new();
        let info_hash = InfoHash::new([19u8; 20]);
        let file_size = 100 * 1024 * 1024; // 100MB file

        // Add file with insufficient head data (less than 3MB minimum)
        data_source.add_file(info_hash, file_size).await;
        data_source.set_head_available(info_hash, 1024 * 1024).await; // Only 1MB head (less than 3MB minimum)

        let manager = RemuxSessionManager::new(
            config,
            Arc::new(data_source),
            Arc::new(crate::streaming::SimulationFfmpegProcessor::new()),
        );
        let _handle = manager.get_or_create_session(info_hash).await.unwrap();

        // Should still wait because head data is insufficient
        let readiness = manager.check_readiness(info_hash).await.unwrap();
        assert_eq!(readiness, StreamReadiness::WaitingForData);
    }

    #[tokio::test]
    async fn test_no_data_available_waits() {
        // Test that scenarios with no data still wait
        let config = create_test_config();
        let data_source = MockDataSource::new();
        let info_hash = InfoHash::new([20u8; 20]);
        let file_size = 100 * 1024 * 1024; // 100MB file

        // Add file with no data available
        data_source.add_file(info_hash, file_size).await;
        // Don't add any ranges - no data available

        let manager = RemuxSessionManager::new(
            config,
            Arc::new(data_source),
            Arc::new(crate::streaming::SimulationFfmpegProcessor::new()),
        );
        let _handle = manager.get_or_create_session(info_hash).await.unwrap();

        // Should wait because no data is available
        let readiness = manager.check_readiness(info_hash).await.unwrap();
        assert_eq!(readiness, StreamReadiness::WaitingForData);
    }
}
