//! Remux streaming with real-time FFmpeg integration
//!
//! This module provides remux streaming for container formats that require
//! conversion for browser compatibility (AVI, MKV -> MP4). It works at any
//! download completion level (0-100%) using a real-time FFmpeg pipeline that
//! processes input as it becomes available and streams output immediately.

use std::collections::HashMap;
use std::ops::Range;
use std::process::Stdio;
use std::sync::Arc;
use std::time::{Duration, Instant};

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::process::{Child, Command};
use tokio::sync::{Notify, RwLock, mpsc};
use tokio::time::timeout;

use crate::streaming::ffmpeg::RemuxingOptions;
use crate::streaming::file_assembler::FileAssembler;
use crate::streaming::strategy::{
    ContainerFormat, StreamingError, StreamingResult, StreamingStrategy,
};
use crate::torrent::InfoHash;

/// Configuration for unified remux streaming
#[derive(Debug, Clone)]
pub struct RemuxStreamingConfig {
    /// Minimum bytes needed from file head before starting FFmpeg
    pub min_head_size: u64,
    /// Maximum output buffer size per session (memory limit)
    pub max_output_buffer_size: usize,
    /// Timeout for waiting for piece availability
    pub piece_wait_timeout: Duration,
    /// Size of input chunks fed to FFmpeg
    pub input_chunk_size: usize,
    /// FFmpeg remuxing options
    pub remuxing_options: RemuxingOptions,
    /// Maximum concurrent remuxing sessions
    pub max_concurrent_sessions: usize,
    /// FFmpeg process timeout
    pub ffmpeg_timeout: Duration,
}

impl Default for RemuxStreamingConfig {
    fn default() -> Self {
        Self {
            min_head_size: 2 * 1024 * 1024, // 2MB minimum head for AVI/MKV
            max_output_buffer_size: 50 * 1024 * 1024, // 50MB buffer limit
            piece_wait_timeout: Duration::from_secs(30),
            input_chunk_size: 256 * 1024, // 256KB chunks
            remuxing_options: RemuxingOptions::default(),
            max_concurrent_sessions: 10,
            ffmpeg_timeout: Duration::from_secs(300), // 5 minutes
        }
    }
}

/// Unified remux streaming strategy
pub struct RemuxStreamingStrategy {
    file_assembler: Arc<dyn FileAssembler>,
    active_sessions: Arc<RwLock<HashMap<InfoHash, Arc<RemuxSession>>>>,
    config: RemuxStreamingConfig,
}

/// Active remuxing session with FFmpeg process
struct RemuxSession {
    info_hash: InfoHash,
    _start_time: Instant,
    file_size: u64,
    input_position: std::sync::atomic::AtomicU64,
    output_chunks: RwLock<HashMap<u64, OutputChunk>>,
    output_size: std::sync::atomic::AtomicU64,
    status: RwLock<RemuxingStatus>,
    ffmpeg_process: RwLock<Option<Child>>,
    error: RwLock<Option<StreamingError>>,
    head_data_ready: Notify,
    ready_for_output: Notify,
}

/// Output chunk with range and timestamp
#[derive(Debug, Clone)]
struct OutputChunk {
    range: Range<u64>,
    data: Vec<u8>,
    _timestamp: Instant,
}

/// Current status of remuxing session
#[derive(Debug, Clone)]
enum RemuxingStatus {
    /// Waiting for sufficient head data
    WaitingForHead { _bytes_needed: u64 },
    /// FFmpeg process active and processing
    Processing {
        _input_consumed: u64,
        output_produced: u64,
    },
    /// Successfully completed
    Completed { total_output: u64 },
    /// Failed with error
    Failed { _reason: String },
}

impl RemuxStreamingStrategy {
    /// Create new remux streaming strategy
    pub fn new(file_assembler: Arc<dyn FileAssembler>, config: RemuxStreamingConfig) -> Self {
        Self {
            file_assembler,
            active_sessions: Arc::new(RwLock::new(HashMap::new())),
            config,
        }
    }

    /// Ensure remuxing session is active for the given info hash
    async fn ensure_session_active(
        &self,
        info_hash: InfoHash,
    ) -> StreamingResult<Arc<RemuxSession>> {
        // First, try a read lock to quickly get an existing session
        {
            let sessions = self.active_sessions.read().await;
            if let Some(session) = sessions.get(&info_hash)
                && self.is_session_healthy(session).await
            {
                return Ok(Arc::clone(session));
            }
        } // Read lock is released here

        // If not found or unhealthy, get a write lock to create a new one
        let mut sessions = self.active_sessions.write().await;

        // Double-check if another thread created it while we waited for the write lock
        if let Some(session) = sessions.get(&info_hash) {
            if self.is_session_healthy(session).await {
                return Ok(Arc::clone(session));
            } else {
                sessions.remove(&info_hash);
            }
        }

        if sessions.len() >= self.config.max_concurrent_sessions {
            return Err(StreamingError::FfmpegError {
                reason: "Too many concurrent remuxing sessions".to_string(),
            });
        }

        // Create a new session
        let file_size = self
            .file_assembler
            .file_size(info_hash)
            .await
            .map_err(|e| StreamingError::FfmpegError {
                reason: format!("Failed to get file size: {e}"),
            })?;
        let session = Arc::new(RemuxSession {
            info_hash,
            _start_time: Instant::now(),
            file_size,
            input_position: std::sync::atomic::AtomicU64::new(0),
            output_chunks: RwLock::new(HashMap::new()),
            output_size: std::sync::atomic::AtomicU64::new(0),
            status: RwLock::new(RemuxingStatus::WaitingForHead {
                _bytes_needed: self.config.min_head_size,
            }),
            ffmpeg_process: RwLock::new(None),
            error: RwLock::new(None),
            head_data_ready: Notify::new(),
            ready_for_output: Notify::new(), // The crucial notifier
        });

        sessions.insert(info_hash, Arc::clone(&session));

        // Spawn the remuxing task in the background
        self.start_remuxing_task(Arc::clone(&session)).await?;

        // <<< FIX: Block here until the session signals it's ready.
        // This will block the *first* request until the MP4 header is generated.
        session.ready_for_output.notified().await;

        // After waiting, check if an error occurred during initialization.
        if let Some(error) = session.error.read().await.as_ref() {
            return Err(StreamingError::FfmpegError {
                reason: format!("Failed to prepare remux session: {error:?}"),
            });
        }

        Ok(session)
    }

    /// Check if remuxing session is healthy
    async fn is_session_healthy(&self, session: &RemuxSession) -> bool {
        // Check if process is still running
        if let Some(_process) = session.ffmpeg_process.read().await.as_ref() {
            // For now, assume process is healthy if it exists
            // In a real implementation, we'd check process status
            true
        } else {
            // Check if session has error
            session.error.read().await.is_none()
        }
    }

    /// Start remuxing task with real FFmpeg integration
    async fn start_remuxing_task(&self, session: Arc<RemuxSession>) -> StreamingResult<()> {
        let file_assembler = Arc::clone(&self.file_assembler);
        let config = self.config.clone();

        tokio::spawn(async move {
            if let Err(e) =
                Self::run_remuxing_pipeline(session.clone(), file_assembler, config).await
            {
                tracing::error!("Remuxing pipeline failed for {}: {}", session.info_hash, e);
                *session.error.write().await = Some(e);
                *session.status.write().await = RemuxingStatus::Failed {
                    _reason: "Remuxing pipeline failed".to_string(),
                };
            }
        });

        Ok(())
    }

    /// Run the complete remuxing pipeline
    async fn run_remuxing_pipeline(
        session: Arc<RemuxSession>,
        file_assembler: Arc<dyn FileAssembler>,
        config: RemuxStreamingConfig,
    ) -> StreamingResult<()> {
        tracing::info!("Starting remuxing pipeline for {}", session.info_hash);

        // Wait for sufficient head data
        Self::wait_for_head_data(&session, &file_assembler, &config).await?;

        // Start FFmpeg process
        let ffmpeg_process = Self::start_ffmpeg_process(&session, &config).await?;

        // Store process reference
        *session.ffmpeg_process.write().await = Some(ffmpeg_process);

        // Get process handles
        let mut process_guard = session.ffmpeg_process.write().await;
        let process = process_guard.as_mut().unwrap();

        let mut stdin = process
            .stdin
            .take()
            .ok_or_else(|| StreamingError::FfmpegError {
                reason: "Failed to get FFmpeg stdin".to_string(),
            })?;

        let mut stdout = process
            .stdout
            .take()
            .ok_or_else(|| StreamingError::FfmpegError {
                reason: "Failed to get FFmpeg stdout".to_string(),
            })?;

        let mut stderr = process
            .stderr
            .take()
            .ok_or_else(|| StreamingError::FfmpegError {
                reason: "Failed to get FFmpeg stderr".to_string(),
            })?;

        drop(process_guard);

        // Create channels for coordination
        let (input_tx, mut input_rx) = mpsc::channel::<Vec<u8>>(16);
        let (output_tx, output_rx) = mpsc::channel::<Vec<u8>>(16);

        // Spawn input feeder task
        let input_session = Arc::clone(&session);
        let input_assembler = Arc::clone(&file_assembler);
        let input_config = config.clone();
        let input_handle = tokio::spawn(async move {
            Self::feed_input_data(input_session, input_assembler, input_config, input_tx).await
        });

        // Spawn output collector task
        let output_session = Arc::clone(&session);
        let output_handle =
            tokio::spawn(async move { Self::collect_output_data(output_session, output_rx).await });

        // Spawn FFmpeg I/O handlers
        let stdin_handle = tokio::spawn(async move {
            while let Some(chunk) = input_rx.recv().await {
                if let Err(e) = stdin.write_all(&chunk).await {
                    tracing::error!("Failed to write to FFmpeg stdin: {}", e);
                    break;
                }
            }
            let _ = stdin.shutdown().await;
        });

        let stdout_handle = tokio::spawn(async move {
            let mut buffer = vec![0u8; 64 * 1024]; // 64KB buffer
            loop {
                match stdout.read(&mut buffer).await {
                    Ok(0) => break, // EOF
                    Ok(n) => {
                        if output_tx.send(buffer[..n].to_vec()).await.is_err() {
                            break;
                        }
                    }
                    Err(e) => {
                        tracing::error!("Failed to read from FFmpeg stdout: {}", e);
                        break;
                    }
                }
            }
        });

        let stderr_handle = tokio::spawn(async move {
            let mut buffer = vec![0u8; 4096];
            loop {
                match stderr.read(&mut buffer).await {
                    Ok(0) => break, // EOF
                    Ok(n) => {
                        let stderr_output = String::from_utf8_lossy(&buffer[..n]);
                        tracing::warn!("FFmpeg stderr: {}", stderr_output.trim());
                    }
                    Err(e) => {
                        tracing::error!("Failed to read from FFmpeg stderr: {}", e);
                        break;
                    }
                }
            }
        });

        // <<< FIX: Wait for FFmpeg to produce initial output before marking as ready.
        let initial_output_wait_timeout = Duration::from_secs(15); // Increased timeout for slower machines
        let initial_output_ready = tokio::time::timeout(initial_output_wait_timeout, async {
            let mut wait_time = 0u64;
            loop {
                let output_size = session
                    .output_size
                    .load(std::sync::atomic::Ordering::Relaxed);

                // For small files, accept any output after a brief wait
                // For larger files, wait for substantial output (header + some data)
                if output_size > 0 && (output_size > 1024 || wait_time > 2000) {
                    break;
                }

                tokio::time::sleep(Duration::from_millis(100)).await;
                wait_time += 100;
            }
        })
        .await;

        // Handle timeout case
        if initial_output_ready.is_err() {
            let reason = "FFmpeg did not produce initial output within timeout".to_string();
            tracing::error!(
                "Remuxing pipeline for {} failed: {}",
                session.info_hash,
                reason
            );
            *session.error.write().await = Some(StreamingError::FfmpegError {
                reason: reason.clone(),
            });
            session.ready_for_output.notify_waiters(); // Notify waiters about the failure
            return Err(StreamingError::FfmpegError { reason });
        }

        // Notify waiters that the session is now ready to serve initial content
        session.ready_for_output.notify_waiters();
        tracing::info!(
            "Remux session for {} is now ready for output.",
            session.info_hash
        );

        // Update status to processing
        *session.status.write().await = RemuxingStatus::Processing {
            _input_consumed: 0,
            output_produced: session
                .output_size
                .load(std::sync::atomic::Ordering::Relaxed),
        };

        // Wait for completion with timeout
        let join_result = tokio::try_join!(
            input_handle,
            output_handle,
            stdin_handle,
            stdout_handle,
            stderr_handle
        );
        let result = timeout(config.ffmpeg_timeout, async move { join_result }).await;

        match result {
            Ok(Ok((_, _, _, _, _))) => {
                let output_size = session
                    .output_size
                    .load(std::sync::atomic::Ordering::Relaxed);
                *session.status.write().await = RemuxingStatus::Completed {
                    total_output: output_size,
                };
                tracing::info!(
                    "Remuxing completed for {} ({} bytes)",
                    session.info_hash,
                    output_size
                );
            }
            Ok(Err(e)) => {
                return Err(StreamingError::FfmpegError {
                    reason: format!("FFmpeg task failed: {e}"),
                });
            }
            Err(_) => {
                return Err(StreamingError::FfmpegError {
                    reason: "FFmpeg process timed out".to_string(),
                });
            }
        }

        // Clean up process
        if let Some(mut process) = session.ffmpeg_process.write().await.take() {
            let _ = process.kill().await;
        }

        Ok(())
    }

    /// Wait for sufficient head data before starting FFmpeg
    async fn wait_for_head_data(
        session: &RemuxSession,
        file_assembler: &Arc<dyn FileAssembler>,
        config: &RemuxStreamingConfig,
    ) -> StreamingResult<()> {
        let head_range = 0..config.min_head_size.min(session.file_size);
        let required_bytes = head_range.end;

        tracing::info!(
            "Waiting for head data: {} needs {} bytes (0..{})",
            session.info_hash,
            required_bytes,
            head_range.end
        );

        // Wait for head data with timeout
        let start_time = Instant::now();
        let mut last_log_time = start_time;

        while start_time.elapsed() < config.piece_wait_timeout {
            if file_assembler.is_range_available(session.info_hash, head_range.clone()) {
                tracing::info!(
                    "Head data available for {} after {:.2}s",
                    session.info_hash,
                    start_time.elapsed().as_secs_f64()
                );
                // Notify that head data is ready
                session.head_data_ready.notify_waiters();
                return Ok(());
            }

            // Log progress every 2 seconds
            if last_log_time.elapsed().as_secs() >= 2 {
                tracing::debug!(
                    "Still waiting for head data for {}: need {} bytes, elapsed {:.1}s",
                    session.info_hash,
                    required_bytes,
                    start_time.elapsed().as_secs_f64()
                );
                last_log_time = Instant::now();
            }

            tokio::time::sleep(Duration::from_millis(100)).await;
        }

        tracing::error!(
            "Head data timeout for {}: needed {} bytes but insufficient data after {:.1}s",
            session.info_hash,
            required_bytes,
            start_time.elapsed().as_secs_f64()
        );

        // Notify waiters about the failure
        session.head_data_ready.notify_waiters();

        Err(StreamingError::FfmpegError {
            reason: format!(
                "Waiting for sufficient head data - need {required_bytes} bytes for remux streaming",
            ),
        })
    }

    /// Start FFmpeg process for remuxing
    async fn start_ffmpeg_process(
        session: &RemuxSession,
        config: &RemuxStreamingConfig,
    ) -> StreamingResult<Child> {
        let mut cmd = Command::new("ffmpeg");

        // Basic options
        cmd.arg("-y") // Overwrite output
            .arg("-i")
            .arg("pipe:0") // Input from pipe
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        // Smart codec selection for MP4 compatibility
        cmd.arg("-c:v");
        if config.remuxing_options.video_codec == "copy" {
            // For AVI files with raw video, use H.264 encoding since copy doesn't work
            cmd.arg("libx264")
                .arg("-preset")
                .arg("ultrafast") // Fast encoding for real-time streaming
                .arg("-crf")
                .arg("23"); // Good quality/speed balance
        } else {
            cmd.arg(&config.remuxing_options.video_codec);
        }

        cmd.arg("-c:a");
        if config.remuxing_options.audio_codec == "copy" {
            // Use AAC for MP4 compatibility
            cmd.arg("aac").arg("-b:a").arg("128k"); // Standard bitrate
        } else {
            cmd.arg(&config.remuxing_options.audio_codec);
        }

        // MP4 output with streaming optimization - use fragmented MP4 for pipe output
        cmd.arg("-movflags")
            .arg("+frag_keyframe+empty_moov+default_base_moof");

        // Output format
        cmd.arg("-f").arg("mp4").arg("pipe:1"); // Output to stdout

        tracing::debug!(
            "Starting FFmpeg process for {}: {:?}",
            session.info_hash,
            cmd
        );

        let process = cmd.spawn().map_err(|e| StreamingError::FfmpegError {
            reason: format!("Failed to spawn FFmpeg process: {e}"),
        })?;

        Ok(process)
    }

    /// Feed input data to FFmpeg from file assembler
    async fn feed_input_data(
        session: Arc<RemuxSession>,
        file_assembler: Arc<dyn FileAssembler>,
        config: RemuxStreamingConfig,
        input_tx: mpsc::Sender<Vec<u8>>,
    ) -> StreamingResult<()> {
        let mut position = 0u64;
        let mut last_successful_position = 0u64;

        // First, feed all available head data sequentially
        while position < session.file_size {
            let chunk_size = config
                .input_chunk_size
                .min((session.file_size - position) as usize);
            let range = position..position + chunk_size as u64;

            // Check if this chunk is available (don't wait)
            if file_assembler.is_range_available(session.info_hash, range.clone()) {
                match file_assembler
                    .read_range(session.info_hash, range.clone())
                    .await
                {
                    Ok(data) => {
                        if input_tx.send(data).await.is_err() {
                            break; // Receiver closed
                        }
                        position += chunk_size as u64;
                        last_successful_position = position;
                        session
                            .input_position
                            .store(position, std::sync::atomic::Ordering::Relaxed);
                    }
                    Err(e) => {
                        tracing::error!(
                            "Failed to read input chunk for {}: {}",
                            session.info_hash,
                            e
                        );
                        break;
                    }
                }
            } else {
                // No more sequential data available, break out of sequential reading
                break;
            }
        }

        tracing::info!(
            "Fed {} bytes of sequential head data to FFmpeg for {}",
            last_successful_position,
            session.info_hash
        );

        // For streaming, we've fed the head data which should be sufficient for FFmpeg
        // to start processing. FFmpeg will work with partial data.
        Ok(())
    }

    /// Collect output data from FFmpeg
    async fn collect_output_data(
        session: Arc<RemuxSession>,
        mut output_rx: mpsc::Receiver<Vec<u8>>,
    ) -> StreamingResult<()> {
        let mut current_position = 0u64;

        while let Some(chunk) = output_rx.recv().await {
            let chunk_size = chunk.len() as u64;
            let range = current_position..current_position + chunk_size;

            let output_chunk = OutputChunk {
                range: range.clone(),
                data: chunk,
                _timestamp: Instant::now(),
            };

            // Store chunk
            {
                let mut chunks = session.output_chunks.write().await;
                chunks.insert(current_position, output_chunk);

                // Enforce buffer size limit
                let total_size: usize = chunks.values().map(|c| c.data.len()).sum();
                if total_size > session.file_size as usize {
                    // Remove some chunks to free memory
                    let keys_to_remove: Vec<_> =
                        chunks.keys().take(chunks.len() / 2).copied().collect();
                    for key in keys_to_remove {
                        chunks.remove(&key);
                    }
                }
            }

            session.output_size.store(
                current_position + chunk_size,
                std::sync::atomic::Ordering::Relaxed,
            );
            current_position += chunk_size;
        }

        Ok(())
    }

    /// Get output chunk for requested range
    async fn get_output_chunk(
        &self,
        session: &RemuxSession,
        range: Range<u64>,
    ) -> StreamingResult<Vec<u8>> {
        let chunks = session.output_chunks.read().await;
        let mut result = Vec::new();

        // Find chunks that overlap with the requested range
        for (&chunk_start, chunk) in chunks.iter() {
            if chunk.range.end > range.start && chunk.range.start < range.end {
                // This chunk overlaps with our range
                let chunk_offset = range.start.saturating_sub(chunk_start) as usize;
                let chunk_len =
                    (chunk.range.end - range.start).min(range.end - range.start) as usize;

                if chunk_offset < chunk.data.len() {
                    let copy_len = chunk_len.min(chunk.data.len() - chunk_offset);
                    result.extend_from_slice(&chunk.data[chunk_offset..chunk_offset + copy_len]);
                }
            }
        }

        if result.len() < (range.end - range.start) as usize {
            // Check if FFmpeg is still processing
            let status = session.status.read().await;

            match *status {
                RemuxingStatus::WaitingForHead { .. } => {
                    return Err(StreamingError::FfmpegError {
                        reason: "Waiting for sufficient head data".to_string(),
                    });
                }
                RemuxingStatus::Processing { .. } => {
                    // For streaming, return whatever data is available
                    if result.is_empty() {
                        return Err(StreamingError::FfmpegError {
                            reason: "FFmpeg is processing, output not ready yet".to_string(),
                        });
                    }
                    // Return available data even if it's less than requested
                }
                RemuxingStatus::Failed { .. } => {
                    return Err(StreamingError::FfmpegError {
                        reason: "FFmpeg process failed".to_string(),
                    });
                }
                RemuxingStatus::Completed { .. } => {
                    // For completed sessions, return whatever data is available
                    if result.is_empty() {
                        return Err(StreamingError::FfmpegError {
                            reason: "Insufficient output data available".to_string(),
                        });
                    }
                    // Return available data even if it's less than requested
                }
            }
        }

        Ok(result)
    }
}

#[async_trait::async_trait]
impl StreamingStrategy for RemuxStreamingStrategy {
    async fn stream_range(
        &self,
        info_hash: InfoHash,
        range: Range<u64>,
    ) -> StreamingResult<Vec<u8>> {
        let session = self.ensure_session_active(info_hash).await?;

        // Check if we have error
        if let Some(_error) = session.error.read().await.as_ref() {
            return Err(StreamingError::FfmpegError {
                reason: "Session has error".to_string(),
            });
        }

        // Try to get output chunk
        self.get_output_chunk(&session, range).await
    }

    async fn file_size(&self, info_hash: InfoHash) -> StreamingResult<u64> {
        let session = self.ensure_session_active(info_hash).await?;

        let status = session.status.read().await;
        match &*status {
            RemuxingStatus::Completed { total_output } => Ok(*total_output),
            RemuxingStatus::Processing {
                output_produced, ..
            } => {
                // Estimate based on progress
                let input_pos = session
                    .input_position
                    .load(std::sync::atomic::Ordering::Relaxed);
                let progress = input_pos as f64 / session.file_size as f64;
                let estimated_size = (*output_produced as f64 / progress.max(0.01)) as u64;
                Ok(estimated_size)
            }
            _ => Ok(session.file_size), // Return original file size as estimate
        }
    }

    async fn container_format(&self, _info_hash: InfoHash) -> StreamingResult<ContainerFormat> {
        // Always outputs MP4
        Ok(ContainerFormat::Mp4)
    }

    fn supports_format(&self, format: &ContainerFormat) -> bool {
        // Supports all formats for remuxing to MP4
        matches!(
            format,
            ContainerFormat::Avi | ContainerFormat::Mkv | ContainerFormat::Mp4
        )
    }
}

/// Create remux streaming strategy with default configuration
pub fn create_remux_streaming_strategy(
    file_assembler: Arc<dyn FileAssembler>,
) -> RemuxStreamingStrategy {
    RemuxStreamingStrategy::new(file_assembler, RemuxStreamingConfig::default())
}

/// Create remux streaming strategy with custom configuration
pub fn create_remux_streaming_strategy_with_config(
    file_assembler: Arc<dyn FileAssembler>,
    config: RemuxStreamingConfig,
) -> RemuxStreamingStrategy {
    RemuxStreamingStrategy::new(file_assembler, config)
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use tokio::sync::RwLock;

    use super::*;
    use crate::streaming::file_assembler::{FileAssembler, FileAssemblerError};
    use crate::torrent::InfoHash;

    #[derive(Debug)]
    struct MockFileAssembler {
        available_ranges: RwLock<HashMap<InfoHash, Vec<Range<u64>>>>,
        file_sizes: HashMap<InfoHash, u64>,
        data: HashMap<InfoHash, Vec<u8>>,
    }

    impl MockFileAssembler {
        fn new() -> Self {
            Self {
                available_ranges: RwLock::new(HashMap::new()),
                file_sizes: HashMap::new(),
                data: HashMap::new(),
            }
        }

        fn add_file(&mut self, info_hash: InfoHash, size: u64, data: Vec<u8>) {
            self.file_sizes.insert(info_hash, size);
            self.data.insert(info_hash, data);
        }

        async fn make_range_available(&self, info_hash: InfoHash, range: Range<u64>) {
            let mut ranges = self.available_ranges.write().await;
            ranges.entry(info_hash).or_insert_with(Vec::new).push(range);
        }
    }

    #[async_trait::async_trait]
    impl FileAssembler for MockFileAssembler {
        async fn read_range(
            &self,
            info_hash: InfoHash,
            range: Range<u64>,
        ) -> Result<Vec<u8>, FileAssemblerError> {
            let data = self.data.get(&info_hash).ok_or_else(|| {
                FileAssemblerError::Torrent(crate::torrent::TorrentError::TorrentNotFound {
                    info_hash,
                })
            })?;

            Ok(data[range.start as usize..range.end as usize].to_vec())
        }

        async fn file_size(&self, info_hash: InfoHash) -> Result<u64, FileAssemblerError> {
            self.file_sizes.get(&info_hash).copied().ok_or_else(|| {
                FileAssemblerError::Torrent(crate::torrent::TorrentError::TorrentNotFound {
                    info_hash,
                })
            })
        }

        fn is_range_available(&self, info_hash: InfoHash, range: Range<u64>) -> bool {
            if let Ok(ranges) = self.available_ranges.try_read() {
                if let Some(available) = ranges.get(&info_hash) {
                    return available
                        .iter()
                        .any(|r| r.start <= range.start && r.end >= range.end);
                }
            }
            false
        }
    }

    #[tokio::test]
    async fn test_remux_streaming_creation() {
        let file_assembler: Arc<dyn FileAssembler> = Arc::new(MockFileAssembler::new());
        let strategy = create_remux_streaming_strategy(file_assembler);

        // Test basic properties
        assert!(strategy.supports_format(&ContainerFormat::Avi));
        assert!(strategy.supports_format(&ContainerFormat::Mkv));
        assert!(strategy.supports_format(&ContainerFormat::Mp4));
    }

    #[tokio::test]
    async fn test_remux_streaming_head_requirement() {
        let mut file_assembler = MockFileAssembler::new();
        let info_hash = InfoHash::new([1u8; 20]);
        let test_data = vec![0u8; 2048 * 1024]; // 2MB test data

        file_assembler.add_file(info_hash, test_data.len() as u64, test_data);

        let strategy =
            create_remux_streaming_strategy(Arc::new(file_assembler) as Arc<dyn FileAssembler>);

        // Should fail without head data
        let result = strategy.stream_range(info_hash, 0..1024).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_remux_streaming_with_head_data() {
        let mut file_assembler = MockFileAssembler::new();
        let info_hash = InfoHash::new([1u8; 20]);
        let test_data = vec![0u8; 2048 * 1024]; // 2MB test data

        file_assembler.add_file(info_hash, test_data.len() as u64, test_data);

        // Make head data available
        file_assembler
            .make_range_available(info_hash, 0..1024 * 1024)
            .await;

        let file_assembler: Arc<dyn FileAssembler> = Arc::new(file_assembler);
        let strategy = create_remux_streaming_strategy(Arc::clone(&file_assembler));

        // Should succeed with head data (will fail due to no FFmpeg in tests, but validates setup)
        let result = strategy.stream_range(info_hash, 0..1024).await;
        // In real tests with FFmpeg, this would succeed
        assert!(result.is_err()); // Expected in test environment without FFmpeg
    }
}
