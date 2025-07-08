//! Background transcoding pipeline for efficient video processing
//!
//! Provides a worker pool system for transcoding video segments with priority-based
//! scheduling and intelligent caching. Supports multiple output formats and quality levels.

pub mod segment_cache;
pub mod worker_pool;

use std::sync::Arc;
use std::time::Duration;

pub use segment_cache::{
    CachedSegment, SegmentCache, SegmentCacheConfig, SegmentCacheError, SegmentCacheStats,
};
pub use worker_pool::{
    FfmpegInstance, JobId, JobPriority, JobResult, PoolStats, SegmentKey, TranscodeJob,
    TranscodeProgress, TranscodedSegment, TranscodingError, VideoFormat, VideoQuality, WorkerPool,
    WorkerPoolConfig, WorkerStats,
};

use crate::torrent::InfoHash;

/// High-level transcoding service that coordinates worker pool and caching
pub struct TranscodingService {
    worker_pool: Arc<WorkerPool>,
    config: TranscodingConfig,
}

/// Configuration for transcoding service
#[derive(Debug, Clone)]
pub struct TranscodingConfig {
    /// Worker pool configuration
    pub worker_pool: WorkerPoolConfig,
    /// Default segment duration for transcoding
    pub default_segment_duration: Duration,
    /// Supported output formats
    pub output_formats: Vec<VideoFormat>,
    /// Default quality levels to generate
    pub quality_levels: Vec<VideoQuality>,
    /// Enable adaptive bitrate streaming
    pub enable_adaptive_streaming: bool,
}

impl Default for TranscodingConfig {
    fn default() -> Self {
        Self {
            worker_pool: WorkerPoolConfig::default(),
            default_segment_duration: Duration::from_secs(6), // 6-second segments for HLS
            output_formats: vec![VideoFormat::Mp4, VideoFormat::WebM],
            quality_levels: vec![VideoQuality::Low, VideoQuality::Medium, VideoQuality::High],
            enable_adaptive_streaming: true,
        }
    }
}

impl TranscodingService {
    /// Create new transcoding service with configuration
    pub fn new(config: TranscodingConfig) -> Self {
        let worker_pool = Arc::new(WorkerPool::new(config.worker_pool.clone()));

        Self {
            worker_pool,
            config,
        }
    }

    /// Create service with default configuration
    pub fn new_default() -> Self {
        Self::new(TranscodingConfig::default())
    }

    /// Submit transcoding job with priority
    ///
    /// # Errors
    /// - `TranscodingError::PoolShuttingDown` - Service is shutting down
    /// - `TranscodingError::InvalidJobConfig` - Invalid job parameters
    pub async fn transcode_segment(
        &self,
        segment: SegmentKey,
        input_path: std::path::PathBuf,
        output_path: std::path::PathBuf,
        priority: JobPriority,
    ) -> JobResult {
        let job = TranscodeJob::new(segment, input_path, output_path, priority);
        self.worker_pool.submit_job(job).await
    }

    /// Get transcoding progress for a job
    pub async fn progress(
        &self,
        job_id: JobId,
    ) -> Result<Option<TranscodeProgress>, TranscodingError> {
        self.worker_pool.progress(job_id).await
    }

    /// Cancel transcoding job
    ///
    /// # Errors
    /// Returns `TranscodingError::PoolShuttingDown` if the worker pool is shutting down.
    pub async fn cancel_job(&self, job_id: JobId) -> Result<(), TranscodingError> {
        self.worker_pool.cancel_job(job_id).await
    }

    /// Get service statistics
    ///
    /// # Errors
    /// Returns `TranscodingError::PoolShuttingDown` if the worker pool is shutting down.
    pub async fn statistics(&self) -> Result<TranscodingStats, TranscodingError> {
        let pool_stats = self.worker_pool.statistics().await?;

        Ok(TranscodingStats {
            pool_stats,
            enabled_formats: self.config.output_formats.clone(),
            quality_levels: self.config.quality_levels.clone(),
            adaptive_streaming_enabled: self.config.enable_adaptive_streaming,
        })
    }

    /// Shutdown transcoding service gracefully
    pub async fn shutdown(&self) {
        self.worker_pool.shutdown().await;
    }

    /// Generate segment key for transcoding
    pub fn create_segment_key(
        &self,
        info_hash: InfoHash,
        start_time: Duration,
        quality: VideoQuality,
        format: VideoFormat,
    ) -> SegmentKey {
        SegmentKey::new(
            info_hash,
            start_time,
            self.config.default_segment_duration,
            quality,
            format,
        )
    }

    /// Get supported quality levels
    pub fn supported_qualities(&self) -> &[VideoQuality] {
        &self.config.quality_levels
    }

    /// Get supported output formats
    pub fn supported_formats(&self) -> &[VideoFormat] {
        &self.config.output_formats
    }

    /// Check if adaptive streaming is enabled
    pub fn is_adaptive_streaming_enabled(&self) -> bool {
        self.config.enable_adaptive_streaming
    }
}

/// Combined transcoding statistics
#[derive(Debug, Clone)]
pub struct TranscodingStats {
    pub pool_stats: PoolStats,
    pub enabled_formats: Vec<VideoFormat>,
    pub quality_levels: Vec<VideoQuality>,
    pub adaptive_streaming_enabled: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_transcoding_config_defaults() {
        let config = TranscodingConfig::default();
        assert!(!config.output_formats.is_empty());
        assert!(!config.quality_levels.is_empty());
        assert!(config.enable_adaptive_streaming);
        assert!(config.default_segment_duration > Duration::ZERO);
    }

    #[tokio::test]
    async fn test_transcoding_service_creation() {
        let service = TranscodingService::new_default();
        let stats = service.statistics().await.unwrap();

        assert_eq!(stats.pool_stats.active_workers, 0);
        assert!(stats.adaptive_streaming_enabled);
        assert!(!stats.enabled_formats.is_empty());
        assert!(!stats.quality_levels.is_empty());
    }

    #[tokio::test]
    async fn test_segment_key_creation() {
        let service = TranscodingService::new_default();
        let info_hash = InfoHash::new([1u8; 20]);

        let segment_key = service.create_segment_key(
            info_hash,
            Duration::from_secs(30),
            VideoQuality::Medium,
            VideoFormat::Mp4,
        );

        assert_eq!(segment_key.info_hash, info_hash);
        assert_eq!(segment_key.start_time, Duration::from_secs(30));
        assert_eq!(segment_key.quality, VideoQuality::Medium);
        assert_eq!(segment_key.format, VideoFormat::Mp4);
        assert_eq!(segment_key.duration, Duration::from_secs(6));
    }

    #[tokio::test]
    async fn test_supported_features_query() {
        let service = TranscodingService::new_default();

        assert!(!service.supported_qualities().is_empty());
        assert!(!service.supported_formats().is_empty());
        assert!(service.is_adaptive_streaming_enabled());

        // Check specific defaults
        assert!(
            service
                .supported_qualities()
                .contains(&VideoQuality::Medium)
        );
        assert!(service.supported_formats().contains(&VideoFormat::Mp4));
    }
}
