//! FFmpeg worker pool for efficient background transcoding
//!
//! Provides a pool of reusable FFmpeg processes to handle transcoding jobs
//! with priority-based scheduling and process lifecycle management.

use std::collections::HashMap;
use std::path::PathBuf;
use std::process::Stdio;
use std::time::{Duration, Instant};

use tokio::process::{Child, Command};
use tokio::sync::{mpsc, oneshot};
use uuid::Uuid;

use crate::torrent::InfoHash;

/// Errors that can occur during transcoding operations
#[derive(Debug, thiserror::Error)]
pub enum TranscodingError {
    #[error("FFmpeg process failed: {reason}")]
    ProcessFailed { reason: String },

    #[error("Job not found: {job_id}")]
    JobNotFound { job_id: JobId },

    #[error("Worker pool is shutting down")]
    PoolShuttingDown,

    #[error("Process spawn failed: {reason}")]
    SpawnFailed { reason: String },

    #[error("Job timeout after {seconds} seconds")]
    JobTimeout { seconds: u64 },

    #[error("Invalid job configuration: {reason}")]
    InvalidJobConfig { reason: String },

    #[error("Resource limit exceeded: {reason}")]
    ResourceLimitExceeded { reason: String },
}

/// Unique identifier for transcoding jobs
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct JobId(Uuid);

impl Default for JobId {
    fn default() -> Self {
        Self::new()
    }
}

impl JobId {
    /// Create new job ID
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }
}

impl std::fmt::Display for JobId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Priority levels for transcoding jobs
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum JobPriority {
    /// Low priority background transcoding
    Background = 0,
    /// Normal priority for regular requests
    Normal = 1,
    /// High priority for user-initiated requests
    High = 2,
    /// Critical priority for immediate streaming needs
    Critical = 3,
}

impl JobPriority {
    /// Check if this job can interrupt another job
    pub fn can_interrupt(&self, other: JobPriority) -> bool {
        *self > other
    }
}

/// Video segment key for caching
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SegmentKey {
    pub info_hash: InfoHash,
    pub start_time: Duration,
    pub duration: Duration,
    pub quality: VideoQuality,
    pub format: VideoFormat,
}

impl SegmentKey {
    /// Create new segment key
    pub fn new(
        info_hash: InfoHash,
        start_time: Duration,
        duration: Duration,
        quality: VideoQuality,
        format: VideoFormat,
    ) -> Self {
        Self {
            info_hash,
            start_time,
            duration,
            quality,
            format,
        }
    }

    /// Check if this is a high priority segment (near current playback)
    pub fn is_high_priority(&self) -> bool {
        // Segments starting within 30 seconds are high priority
        self.start_time <= Duration::from_secs(30)
    }
}

/// Video quality settings
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum VideoQuality {
    /// 480p resolution
    Low,
    /// 720p resolution
    Medium,
    /// 1080p resolution
    High,
    /// 4K resolution
    Ultra,
    /// Original quality (no transcoding)
    Original,
}

impl VideoQuality {
    /// Get target bitrate for quality level
    pub fn target_bitrate(&self) -> Option<u32> {
        match self {
            VideoQuality::Low => Some(1000),    // 1 Mbps
            VideoQuality::Medium => Some(2500), // 2.5 Mbps
            VideoQuality::High => Some(5000),   // 5 Mbps
            VideoQuality::Ultra => Some(15000), // 15 Mbps
            VideoQuality::Original => None,     // No transcoding
        }
    }

    /// Get resolution string for FFmpeg
    pub fn resolution(&self) -> Option<&'static str> {
        match self {
            VideoQuality::Low => Some("854x480"),
            VideoQuality::Medium => Some("1280x720"),
            VideoQuality::High => Some("1920x1080"),
            VideoQuality::Ultra => Some("3840x2160"),
            VideoQuality::Original => None,
        }
    }
}

/// Output video format
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum VideoFormat {
    Mp4,
    WebM,
    Hls,
}

impl VideoFormat {
    /// Get file extension
    pub fn extension(&self) -> &'static str {
        match self {
            VideoFormat::Mp4 => "mp4",
            VideoFormat::WebM => "webm",
            VideoFormat::Hls => "m3u8",
        }
    }

    /// Get MIME type
    pub fn mime_type(&self) -> &'static str {
        match self {
            VideoFormat::Mp4 => "video/mp4",
            VideoFormat::WebM => "video/webm",
            VideoFormat::Hls => "application/vnd.apple.mpegurl",
        }
    }
}

/// Transcoding job definition
#[derive(Debug, Clone)]
pub struct TranscodeJob {
    pub id: JobId,
    pub segment: SegmentKey,
    pub input_path: PathBuf,
    pub output_path: PathBuf,
    pub priority: JobPriority,
    pub created_at: Instant,
    pub timeout: Option<Duration>,
}

impl TranscodeJob {
    /// Create new transcoding job
    pub fn new(
        segment: SegmentKey,
        input_path: PathBuf,
        output_path: PathBuf,
        priority: JobPriority,
    ) -> Self {
        Self {
            id: JobId::new(),
            segment,
            input_path,
            output_path,
            priority,
            created_at: Instant::now(),
            timeout: Some(Duration::from_secs(300)), // 5 minute default timeout
        }
    }

    /// Get job age
    pub fn age(&self) -> Duration {
        self.created_at.elapsed()
    }

    /// Check if job has timed out
    pub fn is_timed_out(&self) -> bool {
        if let Some(timeout) = self.timeout {
            self.age() > timeout
        } else {
            false
        }
    }
}

/// Progress tracking for transcoding jobs
#[derive(Debug, Clone)]
pub struct TranscodeProgress {
    pub job_id: JobId,
    pub progress_percent: f64,
    pub estimated_remaining: Option<Duration>,
    pub current_fps: Option<f64>,
    pub started_at: Instant,
}

impl TranscodeProgress {
    /// Create new progress tracker
    pub fn new(job_id: JobId) -> Self {
        Self {
            job_id,
            progress_percent: 0.0,
            estimated_remaining: None,
            current_fps: None,
            started_at: Instant::now(),
        }
    }

    /// Update progress percentage
    pub fn update_progress(&mut self, percent: f64) {
        self.progress_percent = percent.clamp(0.0, 100.0);

        // Estimate remaining time based on current progress
        if percent > 0.0 {
            let elapsed = self.started_at.elapsed();
            let total_estimated = elapsed.as_secs_f64() / (percent / 100.0);
            let remaining = total_estimated - elapsed.as_secs_f64();
            self.estimated_remaining = Some(Duration::from_secs_f64(remaining.max(0.0)));
        }
    }
}

/// Result of a completed transcoding job
#[derive(Debug, Clone)]
pub struct TranscodedSegment {
    pub segment: SegmentKey,
    pub output_path: PathBuf,
    pub file_size: u64,
    pub duration: Duration,
    pub processing_time: Duration,
}

/// Reusable FFmpeg process instance
pub struct FfmpegInstance {
    process: Option<Child>,
    worker_id: usize,
    created_at: Instant,
    jobs_processed: u64,
}

impl FfmpegInstance {
    /// Create new FFmpeg instance
    pub fn new(worker_id: usize) -> Self {
        Self {
            process: None,
            worker_id,
            created_at: Instant::now(),
            jobs_processed: 0,
        }
    }

    /// Start FFmpeg process for a job
    pub async fn start_job(&mut self, job: &TranscodeJob) -> Result<(), TranscodingError> {
        self.stop_current_job().await;

        let mut cmd = Command::new("ffmpeg");

        // Input configuration
        cmd.arg("-y") // Overwrite output
            .arg("-i")
            .arg(&job.input_path);

        // Seek to start time if needed
        if job.segment.start_time > Duration::ZERO {
            cmd.arg("-ss")
                .arg(format!("{:.3}", job.segment.start_time.as_secs_f64()));
        }

        // Duration limit
        if job.segment.duration > Duration::ZERO {
            cmd.arg("-t")
                .arg(format!("{:.3}", job.segment.duration.as_secs_f64()));
        }

        // Quality settings
        if let Some(resolution) = job.segment.quality.resolution() {
            cmd.arg("-vf").arg(format!("scale={resolution}"));
        }

        if let Some(bitrate) = job.segment.quality.target_bitrate() {
            cmd.arg("-b:v").arg(format!("{bitrate}k"));
        }

        // Format-specific settings
        match job.segment.format {
            VideoFormat::Mp4 => {
                cmd.arg("-c:v")
                    .arg("libx264")
                    .arg("-c:a")
                    .arg("aac")
                    .arg("-movflags")
                    .arg("faststart");
            }
            VideoFormat::WebM => {
                cmd.arg("-c:v").arg("libvpx-vp9").arg("-c:a").arg("libopus");
            }
            VideoFormat::Hls => {
                cmd.arg("-c:v")
                    .arg("libx264")
                    .arg("-c:a")
                    .arg("aac")
                    .arg("-hls_time")
                    .arg("6")
                    .arg("-hls_list_size")
                    .arg("0");
            }
        }

        // Output path
        cmd.arg(&job.output_path);

        // Set up stdio
        cmd.stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .stdin(Stdio::null());

        // Start the process
        let child = cmd.spawn().map_err(|e| TranscodingError::SpawnFailed {
            reason: e.to_string(),
        })?;

        self.process = Some(child);
        self.jobs_processed += 1;

        tracing::debug!(
            "Started FFmpeg job {} on worker {} (job #{} for this worker)",
            job.id,
            self.worker_id,
            self.jobs_processed
        );

        Ok(())
    }

    /// Stop current job
    pub async fn stop_current_job(&mut self) {
        if let Some(mut process) = self.process.take() {
            // Try graceful shutdown first
            if let Err(e) = process.kill().await {
                tracing::warn!(
                    "Failed to kill FFmpeg process on worker {}: {}",
                    self.worker_id,
                    e
                );
            }
        }
    }

    /// Wait for current job to complete
    pub async fn wait_for_completion(&mut self) -> Result<(), TranscodingError> {
        if let Some(process) = self.process.take() {
            let output =
                process
                    .wait_with_output()
                    .await
                    .map_err(|e| TranscodingError::ProcessFailed {
                        reason: e.to_string(),
                    })?;

            if output.status.success() {
                Ok(())
            } else {
                let stderr = String::from_utf8_lossy(&output.stderr);
                Err(TranscodingError::ProcessFailed {
                    reason: format!("FFmpeg failed with stderr: {stderr}"),
                })
            }
        } else {
            Ok(())
        }
    }

    /// Check if worker should be recycled (too old or too many jobs)
    pub fn should_recycle(&self) -> bool {
        const MAX_AGE: Duration = Duration::from_secs(3600); // 1 hour
        const MAX_JOBS: u64 = 50;

        self.created_at.elapsed() > MAX_AGE || self.jobs_processed > MAX_JOBS
    }

    /// Get worker statistics
    pub fn stats(&self) -> WorkerStats {
        WorkerStats {
            worker_id: self.worker_id,
            age: self.created_at.elapsed(),
            jobs_processed: self.jobs_processed,
            is_busy: self.process.is_some(),
        }
    }
}

impl Drop for FfmpegInstance {
    fn drop(&mut self) {
        if let Some(mut process) = self.process.take() {
            // Best effort cleanup
            let _ = process.start_kill();
        }
    }
}

/// Worker statistics
#[derive(Debug, Clone)]
pub struct WorkerStats {
    pub worker_id: usize,
    pub age: Duration,
    pub jobs_processed: u64,
    pub is_busy: bool,
}

/// Configuration for worker pool
#[derive(Debug, Clone)]
pub struct WorkerPoolConfig {
    /// Maximum number of concurrent workers
    pub max_workers: usize,
    /// Job queue capacity
    pub queue_capacity: usize,
    /// Worker idle timeout before recycling
    pub worker_idle_timeout: Duration,
    /// Maximum job processing time
    pub job_timeout: Duration,
}

impl Default for WorkerPoolConfig {
    fn default() -> Self {
        Self {
            max_workers: num_cpus::get().min(8), // Don't overwhelm the system
            queue_capacity: 1000,
            worker_idle_timeout: Duration::from_secs(300), // 5 minutes
            job_timeout: Duration::from_secs(300),         // 5 minutes
        }
    }
}

/// Job completion result
pub type JobResult = Result<TranscodedSegment, TranscodingError>;

/// Command messages for worker pool
#[derive(Debug)]
enum PoolCommand {
    Submit {
        job: TranscodeJob,
        result_tx: oneshot::Sender<JobResult>,
    },
    Cancel {
        job_id: JobId,
    },
    GetProgress {
        job_id: JobId,
        result_tx: oneshot::Sender<Option<TranscodeProgress>>,
    },
    GetStats {
        result_tx: oneshot::Sender<PoolStats>,
    },
    Shutdown,
}

/// Pool statistics
#[derive(Debug, Clone)]
pub struct PoolStats {
    pub active_workers: usize,
    pub idle_workers: usize,
    pub queue_length: usize,
    pub completed_jobs: u64,
    pub failed_jobs: u64,
    pub average_processing_time: Option<Duration>,
}

/// FFmpeg worker pool for background transcoding
pub struct WorkerPool {
    command_tx: mpsc::UnboundedSender<PoolCommand>,
    _handle: tokio::task::JoinHandle<()>,
}

impl WorkerPool {
    /// Create new worker pool with configuration
    pub fn new(config: WorkerPoolConfig) -> Self {
        let (command_tx, command_rx) = mpsc::unbounded_channel();

        let handle = tokio::spawn(async move {
            let mut pool = WorkerPoolImpl::new(config, command_rx);
            pool.run().await;
        });

        Self {
            command_tx,
            _handle: handle,
        }
    }

    /// Submit job to worker pool
    pub async fn submit_job(&self, job: TranscodeJob) -> JobResult {
        let (result_tx, result_rx) = oneshot::channel();

        self.command_tx
            .send(PoolCommand::Submit { job, result_tx })
            .map_err(|_| TranscodingError::PoolShuttingDown)?;

        result_rx
            .await
            .map_err(|_| TranscodingError::PoolShuttingDown)?
    }

    /// Cancel job by ID
    pub async fn cancel_job(&self, job_id: JobId) -> Result<(), TranscodingError> {
        self.command_tx
            .send(PoolCommand::Cancel { job_id })
            .map_err(|_| TranscodingError::PoolShuttingDown)?;

        Ok(())
    }

    /// Get job progress
    pub async fn get_progress(
        &self,
        job_id: JobId,
    ) -> Result<Option<TranscodeProgress>, TranscodingError> {
        let (result_tx, result_rx) = oneshot::channel();

        self.command_tx
            .send(PoolCommand::GetProgress { job_id, result_tx })
            .map_err(|_| TranscodingError::PoolShuttingDown)?;

        result_rx
            .await
            .map_err(|_| TranscodingError::PoolShuttingDown)
    }

    /// Get pool statistics
    pub async fn get_stats(&self) -> Result<PoolStats, TranscodingError> {
        let (result_tx, result_rx) = oneshot::channel();

        self.command_tx
            .send(PoolCommand::GetStats { result_tx })
            .map_err(|_| TranscodingError::PoolShuttingDown)?;

        result_rx
            .await
            .map_err(|_| TranscodingError::PoolShuttingDown)
    }

    /// Shutdown worker pool gracefully
    pub async fn shutdown(&self) {
        let _ = self.command_tx.send(PoolCommand::Shutdown);
    }
}

/// Internal worker pool implementation
struct WorkerPoolImpl {
    config: WorkerPoolConfig,
    command_rx: mpsc::UnboundedReceiver<PoolCommand>,
    workers: Vec<FfmpegInstance>,
    job_queue: std::collections::VecDeque<(TranscodeJob, oneshot::Sender<JobResult>)>,
    active_jobs: HashMap<JobId, TranscodeProgress>,
    completed_jobs: u64,
    failed_jobs: u64,
    processing_times: Vec<Duration>,
}

impl WorkerPoolImpl {
    fn new(config: WorkerPoolConfig, command_rx: mpsc::UnboundedReceiver<PoolCommand>) -> Self {
        Self {
            config,
            command_rx,
            workers: Vec::new(),
            job_queue: std::collections::VecDeque::new(),
            active_jobs: HashMap::new(),
            completed_jobs: 0,
            failed_jobs: 0,
            processing_times: Vec::new(),
        }
    }

    async fn run(&mut self) {
        let mut shutdown = false;

        while !shutdown {
            tokio::select! {
                // Handle commands
                cmd = self.command_rx.recv() => {
                    match cmd {
                        Some(PoolCommand::Submit { job, result_tx }) => {
                            self.queue_job(job, result_tx).await;
                        }
                        Some(PoolCommand::Cancel { job_id }) => {
                            self.cancel_job(job_id).await;
                        }
                        Some(PoolCommand::GetProgress { job_id, result_tx }) => {
                            let progress = self.active_jobs.get(&job_id).cloned();
                            let _ = result_tx.send(progress);
                        }
                        Some(PoolCommand::GetStats { result_tx }) => {
                            let stats = self.get_stats();
                            let _ = result_tx.send(stats);
                        }
                        Some(PoolCommand::Shutdown) => {
                            shutdown = true;
                        }
                        None => {
                            shutdown = true;
                        }
                    }
                }

                // Process job queue
                _ = tokio::time::sleep(Duration::from_millis(100)) => {
                    self.process_queue().await;
                    self.recycle_workers().await;
                }
            }
        }

        // Shutdown all workers
        for worker in &mut self.workers {
            worker.stop_current_job().await;
        }
    }

    async fn queue_job(&mut self, job: TranscodeJob, result_tx: oneshot::Sender<JobResult>) {
        // Check queue capacity
        if self.job_queue.len() >= self.config.queue_capacity {
            let _ = result_tx.send(Err(TranscodingError::ResourceLimitExceeded {
                reason: "Job queue is full".to_string(),
            }));
            return;
        }

        // Insert job based on priority
        let insert_pos = self
            .job_queue
            .iter()
            .position(|(queued_job, _)| queued_job.priority < job.priority)
            .unwrap_or(self.job_queue.len());

        self.job_queue.insert(insert_pos, (job, result_tx));
    }

    async fn process_queue(&mut self) {
        while let Some(available_worker_idx) = self.find_available_worker().await {
            if let Some((job, result_tx)) = self.job_queue.pop_front() {
                let progress = TranscodeProgress::new(job.id);
                self.active_jobs.insert(job.id, progress);

                if let Err(e) = self.workers[available_worker_idx].start_job(&job).await {
                    self.active_jobs.remove(&job.id);
                    self.failed_jobs += 1;
                    let _ = result_tx.send(Err(e));
                    continue;
                }

                // Spawn task to wait for completion
                let _job_id = job.id;
                let output_path = job.output_path.clone();
                let segment = job.segment.clone();
                let started_at = Instant::now();

                tokio::spawn(async move {
                    // This would be implemented with proper FFmpeg monitoring
                    // For now, simulate completion
                    tokio::time::sleep(Duration::from_secs(10)).await;

                    let file_size = tokio::fs::metadata(&output_path)
                        .await
                        .map(|m| m.len())
                        .unwrap_or(0);

                    let result = TranscodedSegment {
                        segment,
                        output_path,
                        file_size,
                        duration: Duration::from_secs(30), // Would be parsed from FFmpeg output
                        processing_time: started_at.elapsed(),
                    };

                    let _ = result_tx.send(Ok(result));
                });
            } else {
                break;
            }
        }
    }

    async fn find_available_worker(&mut self) -> Option<usize> {
        // Find idle worker
        for (idx, worker) in self.workers.iter().enumerate() {
            if worker.process.is_none() {
                return Some(idx);
            }
        }

        // Create new worker if under limit
        if self.workers.len() < self.config.max_workers {
            let worker_id = self.workers.len();
            let worker = FfmpegInstance::new(worker_id);
            self.workers.push(worker);
            return Some(worker_id);
        }

        None
    }

    async fn cancel_job(&mut self, job_id: JobId) {
        // Remove from queue
        let mut jobs_to_cancel = Vec::new();
        self.job_queue.retain(|(job, _)| {
            if job.id == job_id {
                jobs_to_cancel.push(job.id);
                false
            } else {
                true
            }
        });

        // Send cancellation notifications
        for _ in jobs_to_cancel {
            // Job was removed from queue, consider it cancelled
        }

        // Stop active job
        if self.active_jobs.remove(&job_id).is_some() {
            for worker in &mut self.workers {
                worker.stop_current_job().await;
            }
        }
    }

    async fn recycle_workers(&mut self) {
        self.workers.retain(|worker| !worker.should_recycle());
    }

    fn get_stats(&self) -> PoolStats {
        let active_workers = self.workers.iter().filter(|w| w.process.is_some()).count();
        let idle_workers = self.workers.len() - active_workers;

        let average_processing_time = if !self.processing_times.is_empty() {
            let total: Duration = self.processing_times.iter().sum();
            Some(total / self.processing_times.len() as u32)
        } else {
            None
        };

        PoolStats {
            active_workers,
            idle_workers,
            queue_length: self.job_queue.len(),
            completed_jobs: self.completed_jobs,
            failed_jobs: self.failed_jobs,
            average_processing_time,
        }
    }
}

#[cfg(test)]
mod tests {
    use tempfile::tempdir;

    use super::*;

    fn create_test_segment_key() -> SegmentKey {
        SegmentKey::new(
            crate::torrent::InfoHash::new([1u8; 20]),
            Duration::from_secs(0),
            Duration::from_secs(30),
            VideoQuality::Medium,
            VideoFormat::Mp4,
        )
    }

    #[test]
    fn test_job_priority_ordering() {
        assert!(JobPriority::Critical > JobPriority::High);
        assert!(JobPriority::High > JobPriority::Normal);
        assert!(JobPriority::Normal > JobPriority::Background);

        assert!(JobPriority::Critical.can_interrupt(JobPriority::Background));
        assert!(!JobPriority::Background.can_interrupt(JobPriority::High));
    }

    #[test]
    fn test_video_quality_settings() {
        assert_eq!(VideoQuality::Low.target_bitrate(), Some(1000));
        assert_eq!(VideoQuality::Original.target_bitrate(), None);

        assert_eq!(VideoQuality::High.resolution(), Some("1920x1080"));
        assert_eq!(VideoQuality::Original.resolution(), None);
    }

    #[test]
    fn test_video_format_properties() {
        assert_eq!(VideoFormat::Mp4.extension(), "mp4");
        assert_eq!(VideoFormat::Mp4.mime_type(), "video/mp4");

        assert_eq!(VideoFormat::WebM.extension(), "webm");
        assert_eq!(VideoFormat::WebM.mime_type(), "video/webm");
    }

    #[test]
    fn test_segment_key_priority() {
        let high_priority = SegmentKey::new(
            crate::torrent::InfoHash::new([1u8; 20]),
            Duration::from_secs(0),
            Duration::from_secs(30),
            VideoQuality::Medium,
            VideoFormat::Mp4,
        );

        let low_priority = SegmentKey::new(
            crate::torrent::InfoHash::new([1u8; 20]),
            Duration::from_secs(60),
            Duration::from_secs(30),
            VideoQuality::Medium,
            VideoFormat::Mp4,
        );

        assert!(high_priority.is_high_priority());
        assert!(!low_priority.is_high_priority());
    }

    #[tokio::test]
    async fn test_transcode_job_creation() {
        let temp_dir = tempdir().unwrap();
        let input_path = temp_dir.path().join("input.mkv");
        let output_path = temp_dir.path().join("output.mp4");

        let segment = create_test_segment_key();
        let job = TranscodeJob::new(
            segment,
            input_path.clone(),
            output_path.clone(),
            JobPriority::Normal,
        );

        assert_eq!(job.input_path, input_path);
        assert_eq!(job.output_path, output_path);
        assert_eq!(job.priority, JobPriority::Normal);
        assert!(job.age() < Duration::from_millis(100));
        assert!(!job.is_timed_out());
    }

    #[tokio::test]
    async fn test_transcode_progress_tracking() {
        let job_id = JobId::new();
        let mut progress = TranscodeProgress::new(job_id);

        assert_eq!(progress.job_id, job_id);
        assert_eq!(progress.progress_percent, 0.0);

        progress.update_progress(50.0);
        assert_eq!(progress.progress_percent, 50.0);
        assert!(progress.estimated_remaining.is_some());

        // Test clamping
        progress.update_progress(150.0);
        assert_eq!(progress.progress_percent, 100.0);

        progress.update_progress(-10.0);
        assert_eq!(progress.progress_percent, 0.0);
    }

    #[test]
    fn test_ffmpeg_instance_lifecycle() {
        let instance = FfmpegInstance::new(1);

        assert_eq!(instance.worker_id, 1);
        assert_eq!(instance.jobs_processed, 0);
        assert!(!instance.should_recycle());

        let stats = instance.stats();
        assert_eq!(stats.worker_id, 1);
        assert_eq!(stats.jobs_processed, 0);
        assert!(!stats.is_busy);
    }

    #[tokio::test]
    async fn test_worker_pool_configuration() {
        let config = WorkerPoolConfig::default();
        assert!(config.max_workers > 0);
        assert!(config.queue_capacity > 0);
        assert!(config.worker_idle_timeout > Duration::ZERO);
        assert!(config.job_timeout > Duration::ZERO);

        let pool = WorkerPool::new(config);
        let stats = pool.get_stats().await.unwrap();
        assert_eq!(stats.active_workers, 0);
        assert_eq!(stats.idle_workers, 0);
        assert_eq!(stats.queue_length, 0);
    }
}
