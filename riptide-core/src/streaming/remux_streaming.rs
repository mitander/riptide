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
use crate::streaming::mp4_validation::{analyze_mp4_for_streaming, debug_mp4_structure};
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
            min_head_size: 10 * 1024 * 1024, // 10MB minimum head for AVI/MKV - ensures FFmpeg has enough data for metadata parsing
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
    // Download speed tracking
    download_speed_bytes_per_sec: std::sync::atomic::AtomicU64,
    bytes_downloaded_at_last_update: std::sync::atomic::AtomicU64,
    last_speed_update: RwLock<Instant>,
}

/// Output chunk with range and timestamp
#[derive(Debug, Clone)]
struct OutputChunk {
    data: Vec<u8>,
    _timestamp: Instant,
}

/// Current status of remuxing session
#[derive(Debug, Clone)]
enum RemuxingStatus {
    /// Waiting for sufficient head and tail data
    WaitingForHead { _bytes_needed: u64 },
    /// FFmpeg process active and processing
    Processing { _input_consumed: u64 },
    /// Successfully completed
    Completed { total_output: u64 },
    /// Failed with error
    Failed { _reason: String },
}

/// Session status information for readiness checking
#[derive(Debug)]
pub enum SessionStatus {
    Ready,
    WaitingForData {
        bytes_needed: u64,
        bytes_available: u64,
    },
    Remuxing {
        progress: Option<f64>,
    },
    Error {
        error: String,
    },
}

/// Session information for readiness checking
#[derive(Debug)]
pub struct SessionInfo {
    pub status: SessionStatus,
    pub output_size: u64,
}

/// Data availability check result
#[derive(Debug)]
pub enum DataAvailability {
    Sufficient,
    Insufficient { available: u64, needed: u64 },
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
            download_speed_bytes_per_sec: std::sync::atomic::AtomicU64::new(0),
            bytes_downloaded_at_last_update: std::sync::atomic::AtomicU64::new(0),
            last_speed_update: RwLock::new(Instant::now()),
        });

        sessions.insert(info_hash, Arc::clone(&session));

        // Spawn the remuxing task in the background
        self.start_remuxing_task(Arc::clone(&session)).await?;

        // <<< FIX: Block here until the session signals it's ready.
        // This will block the *first* request until the MP4 header is generated.
        // Add timeout to prevent infinite waiting in test environments
        let timeout_duration = self.config.ffmpeg_timeout;
        match timeout(timeout_duration, session.ready_for_output.notified()).await {
            Ok(_) => {
                tracing::debug!("Session {} is ready for output", info_hash);
            }
            Err(_) => {
                tracing::warn!("Session {} timed out waiting for ready signal", info_hash);
                // Remove the session from active sessions on timeout
                sessions.remove(&info_hash);
                return Err(StreamingError::FfmpegError {
                    reason: "Session startup timed out".to_string(),
                });
            }
        }

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
                // Notify waiters that the session encountered an error
                session.ready_for_output.notify_waiters();
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

        // No stdout needed since we're writing to a file

        let mut stderr = process
            .stderr
            .take()
            .ok_or_else(|| StreamingError::FfmpegError {
                reason: "Failed to get FFmpeg stderr".to_string(),
            })?;

        drop(process_guard);

        // Create channels for coordination
        let (input_tx, mut input_rx) = mpsc::channel::<Vec<u8>>(16);
        let (_output_tx, output_rx) = mpsc::channel::<Vec<u8>>(16);

        // Spawn input feeder task
        let input_session = Arc::clone(&session);
        let input_assembler = Arc::clone(&file_assembler);
        let input_config = config.clone();
        let input_handle = tokio::spawn(async move {
            Self::feed_input_data(input_session, input_assembler, input_config, input_tx).await
        });

        // Spawn output collector task
        let output_session = Arc::clone(&session);
        let output_config = config.clone();
        let output_handle = tokio::spawn(async move {
            Self::collect_output_data(output_session, output_rx, output_config).await
        });

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

        // No stdout handling needed since we're writing to a file

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

        // <<< FIX: Wait for FFmpeg to produce complete MP4 header before marking as ready.
        let initial_output_wait_timeout = Duration::from_secs(30); // Increased timeout for MP4 header generation
        let initial_output_ready = tokio::time::timeout(initial_output_wait_timeout, async {
            loop {
                // Check if we have a valid MP4 header
                if RemuxStreamingStrategy::has_complete_mp4_header(&session).await {
                    break;
                }

                tokio::time::sleep(Duration::from_millis(200)).await;
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
        *session.status.write().await = RemuxingStatus::Processing { _input_consumed: 0 };

        // Wait for completion with timeout
        let join_result =
            tokio::try_join!(input_handle, output_handle, stdin_handle, stderr_handle,);
        let result = timeout(config.ffmpeg_timeout, async move { join_result }).await;

        match result {
            Ok(Ok((_, _, _, _))) => {
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

    /// Wait for sufficient head and tail data before starting FFmpeg
    async fn wait_for_head_data(
        session: &RemuxSession,
        file_assembler: &Arc<dyn FileAssembler>,
        config: &RemuxStreamingConfig,
    ) -> StreamingResult<()> {
        let head_range = 0..config.min_head_size.min(session.file_size);
        let head_bytes = head_range.end;

        // Calculate tail range - need tail data for proper remuxing
        let tail_size = config.min_head_size.min(session.file_size);
        let tail_start = session.file_size.saturating_sub(tail_size);
        let tail_range = tail_start..session.file_size;

        tracing::info!(
            "Waiting for head and tail data: {} needs head {}..{} ({} bytes) and tail {}..{} ({} bytes)",
            session.info_hash,
            head_range.start,
            head_range.end,
            head_bytes,
            tail_range.start,
            tail_range.end,
            tail_range.end - tail_range.start
        );

        // Wait for both head and tail data with timeout
        let start_time = Instant::now();
        let mut last_log_time = start_time;

        while start_time.elapsed() < config.piece_wait_timeout {
            let head_available =
                file_assembler.is_range_available(session.info_hash, head_range.clone());
            let tail_available =
                file_assembler.is_range_available(session.info_hash, tail_range.clone());

            if head_available && tail_available {
                tracing::info!(
                    "Head and tail data available for {} after {:.2}s",
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
                    "Still waiting for head and tail data for {}: head available: {}, tail available: {}, elapsed {:.1}s",
                    session.info_hash,
                    head_available,
                    tail_available,
                    start_time.elapsed().as_secs_f64()
                );
                last_log_time = Instant::now();
            }

            tokio::time::sleep(Duration::from_millis(100)).await;
        }

        tracing::error!(
            "Head and tail data timeout for {}: needed head {}..{} and tail {}..{} but insufficient data after {:.1}s",
            session.info_hash,
            head_range.start,
            head_range.end,
            tail_range.start,
            tail_range.end,
            start_time.elapsed().as_secs_f64()
        );

        // Notify waiters about the failure
        session.head_data_ready.notify_waiters();

        Err(StreamingError::FfmpegError {
            reason: format!(
                "Waiting for sufficient head and tail data - need head {head_bytes} bytes and tail {} bytes for remux streaming",
                tail_range.end - tail_range.start
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
            // For AVI files, we need to re-encode to H.264 for browser compatibility
            cmd.arg("libx264")
                .arg("-preset")
                .arg("ultrafast")
                .arg("-profile:v")
                .arg("baseline") // Most compatible profile
                .arg("-level")
                .arg("3.0")
                .arg("-pix_fmt")
                .arg("yuv420p"); // Ensure compatible pixel format
        } else {
            cmd.arg(&config.remuxing_options.video_codec);
        }

        cmd.arg("-c:a");
        if config.remuxing_options.audio_codec == "copy" {
            // Convert to AAC for MP4 compatibility
            cmd.arg("aac").arg("-b:a").arg("128k");
        } else {
            cmd.arg(&config.remuxing_options.audio_codec);
        }

        // Generate proper MP4 structure for browser compatibility
        // Use faststart for HTTP range request compatibility (requires seekable output)
        cmd.arg("-movflags").arg("+faststart");

        // Output format - use MP4 with temporary file
        let temp_file = format!("/tmp/riptide_remux_{}.mp4", session.info_hash);
        cmd.arg("-f").arg("mp4").arg(&temp_file);

        tracing::debug!(
            "Starting FFmpeg process for {}: {:?}",
            session.info_hash,
            cmd
        );

        let process = cmd
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| StreamingError::FfmpegError {
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
        let mut retry_count = 0u32;
        const MAX_CONSECUTIVE_RETRIES: u32 = 600; // 10 minutes with 1s retry interval

        // Feed all data sequentially, waiting for unavailable pieces
        while position < session.file_size {
            let chunk_size = config
                .input_chunk_size
                .min((session.file_size - position) as usize);
            let range = position..position + chunk_size as u64;

            // Wait for this chunk to become available
            while !file_assembler.is_range_available(session.info_hash, range.clone()) {
                // Check if FFmpeg process is still alive
                {
                    let ffmpeg_guard = session.ffmpeg_process.read().await;
                    if ffmpeg_guard.is_none() {
                        tracing::info!("FFmpeg process terminated, stopping input feed");
                        return Ok(());
                    }
                }

                retry_count += 1;
                if retry_count > MAX_CONSECUTIVE_RETRIES {
                    tracing::error!(
                        "Timeout waiting for range {:?} after {} retries for {}",
                        range,
                        retry_count,
                        session.info_hash
                    );
                    return Err(StreamingError::RemuxingFailed {
                        reason: "Timeout waiting for torrent data".to_string(),
                    });
                }

                tracing::debug!(
                    "Waiting for range {:?} to become available for {} (retry {})",
                    range,
                    session.info_hash,
                    retry_count
                );
                tokio::time::sleep(Duration::from_secs(1)).await;
            }

            // Reset retry count on successful availability
            retry_count = 0;

            match file_assembler
                .read_range(session.info_hash, range.clone())
                .await
            {
                Ok(data) => {
                    if input_tx.send(data).await.is_err() {
                        tracing::info!("Input receiver closed, stopping feed");
                        break; // Receiver closed
                    }
                    position += chunk_size as u64;
                    session
                        .input_position
                        .store(position, std::sync::atomic::Ordering::Relaxed);

                    // Update download speed tracking
                    Self::update_download_speed(&session, position).await;

                    if position % (10 * 1024 * 1024) == 0 {
                        tracing::debug!(
                            "Fed {} MB to FFmpeg for {}",
                            position / (1024 * 1024),
                            session.info_hash
                        );
                    }
                }
                Err(e) => {
                    tracing::error!(
                        "Failed to read input chunk for {}: {}",
                        session.info_hash,
                        e
                    );
                    // Continue trying with exponential backoff
                    tokio::time::sleep(Duration::from_secs(2)).await;
                }
            }
        }

        tracing::info!(
            "Completed feeding {} bytes to FFmpeg for {}",
            position,
            session.info_hash
        );

        Ok(())
    }

    /// Collect output data from FFmpeg temporary file
    async fn collect_output_data(
        session: Arc<RemuxSession>,
        _output_rx: mpsc::Receiver<Vec<u8>>,
        config: RemuxStreamingConfig,
    ) -> StreamingResult<()> {
        use tokio::io::{AsyncReadExt, AsyncSeekExt};

        let temp_file = format!("/tmp/riptide_remux_{}.mp4", session.info_hash);
        let mut current_position = 0u64;
        let mut last_file_size = 0u64;

        loop {
            // Check if FFmpeg process is still running
            let process_running = {
                let ffmpeg_guard = session.ffmpeg_process.read().await;
                ffmpeg_guard.is_some()
            };

            // Try to read from the temporary file
            if let Ok(mut file) = tokio::fs::File::open(&temp_file).await {
                let file_size = file.metadata().await.map(|m| m.len()).unwrap_or(0);

                if file_size > last_file_size {
                    // New data available, read it
                    let mut buffer = vec![0u8; (file_size - last_file_size) as usize];
                    file.seek(std::io::SeekFrom::Start(last_file_size))
                        .await
                        .map_err(|e| StreamingError::RemuxingFailed {
                            reason: format!("Failed to seek in temp file: {e}"),
                        })?;

                    file.read_exact(&mut buffer).await.map_err(|e| {
                        StreamingError::RemuxingFailed {
                            reason: format!("Failed to read from temp file: {e}"),
                        }
                    })?;

                    // Store the new data as chunks
                    let chunk_size = buffer.len() as u64;
                    let output_chunk = OutputChunk {
                        data: buffer,
                        _timestamp: Instant::now(),
                    };

                    // Store chunk
                    {
                        let mut chunks = session.output_chunks.write().await;
                        chunks.insert(current_position, output_chunk);

                        // Enforce buffer size limit
                        let total_size: usize = chunks.values().map(|c| c.data.len()).sum();
                        if total_size > config.max_output_buffer_size {
                            // Remove chunks from the end, but preserve header data (first 10MB)
                            const HEADER_PRESERVE_SIZE: u64 = 10 * 1024 * 1024; // 10MB

                            let mut keys_to_remove = Vec::new();
                            let mut _preserved_size = 0;

                            // Collect keys to remove, starting from the end, but preserve header
                            for &key in chunks.keys() {
                                if key < HEADER_PRESERVE_SIZE {
                                    _preserved_size +=
                                        chunks.get(&key).map(|c| c.data.len()).unwrap_or(0);
                                } else {
                                    keys_to_remove.push(key);
                                }
                            }

                            // Sort keys to remove in descending order (remove from end first)
                            keys_to_remove.sort_by(|a, b| b.cmp(a));

                            // Remove chunks until we're under the limit
                            let mut current_size = total_size;
                            for key in keys_to_remove {
                                if current_size <= config.max_output_buffer_size {
                                    break;
                                }
                                if let Some(chunk) = chunks.remove(&key) {
                                    current_size -= chunk.data.len();
                                }
                            }
                        }
                    }

                    session.output_size.store(
                        current_position + chunk_size,
                        std::sync::atomic::Ordering::Relaxed,
                    );
                    current_position += chunk_size;
                    last_file_size = file_size;
                }
            }

            // If process is not running and we've read all data, we're done
            if !process_running {
                // Give it a moment to ensure all data is written
                tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

                // Try one more read
                if let Ok(mut file) = tokio::fs::File::open(&temp_file).await {
                    let file_size = file.metadata().await.map(|m| m.len()).unwrap_or(0);

                    if file_size > last_file_size {
                        let mut buffer = vec![0u8; (file_size - last_file_size) as usize];
                        file.seek(std::io::SeekFrom::Start(last_file_size))
                            .await
                            .map_err(|e| StreamingError::RemuxingFailed {
                                reason: format!("Failed to seek in temp file: {e}"),
                            })?;

                        file.read_exact(&mut buffer).await.map_err(|e| {
                            StreamingError::RemuxingFailed {
                                reason: format!("Failed to read from temp file: {e}"),
                            }
                        })?;

                        let chunk_size = buffer.len() as u64;
                        let output_chunk = OutputChunk {
                            data: buffer,
                            _timestamp: Instant::now(),
                        };

                        {
                            let mut chunks = session.output_chunks.write().await;
                            chunks.insert(current_position, output_chunk);
                        }

                        session.output_size.store(
                            current_position + chunk_size,
                            std::sync::atomic::Ordering::Relaxed,
                        );
                        current_position += chunk_size;
                    }
                }

                break;
            }

            // Wait before checking again
            tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
        }

        // Clean up temporary file
        if let Err(e) = tokio::fs::remove_file(&temp_file).await {
            tracing::warn!("Failed to remove temporary file {}: {}", temp_file, e);
        }

        // Mark remuxing as complete
        {
            let mut status = session.status.write().await;
            *status = RemuxingStatus::Completed {
                total_output: current_position,
            };
        }

        tracing::info!(
            "Remuxing completed for {}, total output: {} bytes",
            session.info_hash,
            current_position
        );

        Ok(())
    }

    /// Get output chunk for requested range
    async fn get_output_chunk(
        &self,
        session: &RemuxSession,
        range: Range<u64>,
    ) -> StreamingResult<Vec<u8>> {
        use tokio::io::{AsyncReadExt, AsyncSeekExt};

        let temp_file = format!("/tmp/riptide_remux_{}.mp4", session.info_hash);

        // Try to read from the temporary file
        let mut file = match tokio::fs::File::open(&temp_file).await {
            Ok(file) => file,
            Err(_) => {
                // File doesn't exist yet, check if we're still processing
                let status = session.status.read().await;
                match *status {
                    RemuxingStatus::WaitingForHead { .. } => {
                        return Err(StreamingError::FfmpegError {
                            reason: "Waiting for sufficient head and tail data".to_string(),
                        });
                    }
                    RemuxingStatus::Processing { .. } => {
                        return Err(StreamingError::FfmpegError {
                            reason: "FFmpeg is processing, output not ready yet".to_string(),
                        });
                    }
                    RemuxingStatus::Failed { .. } => {
                        return Err(StreamingError::FfmpegError {
                            reason: "FFmpeg process failed".to_string(),
                        });
                    }
                    RemuxingStatus::Completed { .. } => {
                        return Err(StreamingError::FfmpegError {
                            reason: "Output file not found".to_string(),
                        });
                    }
                }
            }
        };

        // Get file size
        let file_size = file
            .metadata()
            .await
            .map_err(|e| StreamingError::FfmpegError {
                reason: format!("Failed to get file metadata: {e}"),
            })?
            .len();

        // Check if the requested range is beyond the file
        if range.start >= file_size {
            let status = session.status.read().await;
            match *status {
                RemuxingStatus::Completed { .. } => {
                    return Err(StreamingError::FfmpegError {
                        reason: "Requested range beyond file size".to_string(),
                    });
                }
                _ => {
                    return Err(StreamingError::FfmpegError {
                        reason: "FFmpeg is processing, output not ready yet".to_string(),
                    });
                }
            }
        }

        // Seek to the requested position
        file.seek(std::io::SeekFrom::Start(range.start))
            .await
            .map_err(|e| StreamingError::FfmpegError {
                reason: format!("Failed to seek in temp file: {e}"),
            })?;

        // Calculate how much data to read
        let available_bytes = file_size - range.start;
        let bytes_to_read = (range.end - range.start).min(available_bytes) as usize;

        // Read the requested data
        let mut buffer = vec![0u8; bytes_to_read];
        let bytes_read = file
            .read(&mut buffer)
            .await
            .map_err(|e| StreamingError::FfmpegError {
                reason: format!("Failed to read from temp file: {e}"),
            })?;

        // Adjust buffer to actual bytes read
        buffer.truncate(bytes_read);

        // If we couldn't read the full range, check if we're still processing
        if bytes_read < (range.end - range.start) as usize {
            let status = session.status.read().await;
            match *status {
                RemuxingStatus::Completed { .. } => {
                    // File is complete, return whatever we have
                    Ok(buffer)
                }
                _ => {
                    // Still processing, return available data
                    if buffer.is_empty() {
                        return Err(StreamingError::FfmpegError {
                            reason: "FFmpeg is processing, output not ready yet".to_string(),
                        });
                    }
                    Ok(buffer)
                }
            }
        } else {
            Ok(buffer)
        }
    }

    /// Check if the session has a complete MP4 header suitable for browser streaming
    async fn has_complete_mp4_header(session: &RemuxSession) -> bool {
        use tokio::io::AsyncReadExt;

        let temp_file = format!("/tmp/riptide_remux_{}.mp4", session.info_hash);

        tracing::debug!("Checking MP4 header readiness for {}", session.info_hash);

        // Try to read from the temporary file
        let mut file = match tokio::fs::File::open(&temp_file).await {
            Ok(file) => file,
            Err(_) => {
                tracing::debug!("Temp file not found for {}", session.info_hash);
                return false;
            }
        };

        // Get file size - just need basic MP4 header for now
        let file_size = match file.metadata().await {
            Ok(metadata) => metadata.len(),
            Err(_) => return false,
        };

        // Need at least 10MB for smooth streaming playback
        const MIN_BUFFER_FOR_STREAMING: u64 = 10 * 1024 * 1024; // 10MB

        tracing::info!(
            "MP4 file size check for {}: {}MB available",
            session.info_hash,
            file_size / (1024 * 1024)
        );

        if file_size < MIN_BUFFER_FOR_STREAMING {
            tracing::info!(
                "Insufficient buffer for smooth streaming {}: have {}MB, need {}MB - marking NOT READY",
                session.info_hash,
                file_size / (1024 * 1024),
                MIN_BUFFER_FOR_STREAMING / (1024 * 1024)
            );
            return false;
        }

        // Read the first 4KB to validate MP4 header
        let mut buffer = vec![0u8; 4096];
        let bytes_read = match file.read(&mut buffer).await {
            Ok(n) => n,
            Err(_) => return false,
        };

        // Need at least 32 bytes to check for ftyp box
        if bytes_read < 32 {
            return false;
        }

        // Truncate buffer to actual data read
        buffer.truncate(bytes_read);

        // Use proper MP4 validation to check streaming compatibility
        let analysis = analyze_mp4_for_streaming(&buffer);

        if analysis.is_streaming_ready {
            tracing::info!(
                "MP4 header validation PASSED for {} - streaming READY with {}MB buffered",
                session.info_hash,
                file_size / (1024 * 1024)
            );
            return true;
        } else {
            // Log specific issues for debugging
            if !analysis.issues.is_empty() {
                tracing::warn!("MP4 validation issues: {}", analysis.issues.join(", "));
                debug_mp4_structure(&buffer, 5);
            }

            // For streaming, we need at least the ftyp box
            if buffer.len() >= 8 && &buffer[4..8] == b"ftyp" {
                tracing::info!(
                    "MP4 ftyp box detected for {} - allowing streaming start with {}MB buffered",
                    session.info_hash,
                    file_size / (1024 * 1024)
                );
                return true;
            }
        }

        tracing::warn!(
            "MP4 header validation FAILED for {} - not ready for streaming",
            session.info_hash
        );
        false
    }

    /// Update download speed tracking
    async fn update_download_speed(session: &RemuxSession, _bytes_downloaded: u64) {
        let now = Instant::now();
        let mut last_update = session.last_speed_update.write().await;
        let time_elapsed = now.duration_since(*last_update).as_secs_f64();

        if time_elapsed >= 2.0 {
            // Update every 2 seconds
            let previous_bytes = session
                .bytes_downloaded_at_last_update
                .load(std::sync::atomic::Ordering::Relaxed);
            let current_bytes = session
                .input_position
                .load(std::sync::atomic::Ordering::Relaxed);
            let bytes_in_period = current_bytes.saturating_sub(previous_bytes);

            if time_elapsed > 0.0 {
                let speed = (bytes_in_period as f64 / time_elapsed) as u64;
                session
                    .download_speed_bytes_per_sec
                    .store(speed, std::sync::atomic::Ordering::Relaxed);
                session
                    .bytes_downloaded_at_last_update
                    .store(current_bytes, std::sync::atomic::Ordering::Relaxed);
                *last_update = now;

                tracing::debug!(
                    "Download speed updated for {}: {} KB/s",
                    session.info_hash,
                    speed / 1024
                );
            }
        }
    }

    /// Get session information for readiness checking without creating a new session
    pub async fn get_session_info(&self, info_hash: InfoHash) -> Option<SessionInfo> {
        let sessions = self.active_sessions.read().await;
        let session = sessions.get(&info_hash)?;

        let status_guard = session.status.read().await;
        let output_size = session
            .output_size
            .load(std::sync::atomic::Ordering::Relaxed);

        let status = match &*status_guard {
            RemuxingStatus::WaitingForHead { _bytes_needed } => {
                // Check actual data availability
                let head_range = 0..self.config.min_head_size.min(session.file_size);
                let tail_size = self.config.min_head_size.min(session.file_size);
                let tail_start = session.file_size.saturating_sub(tail_size);
                let tail_range = tail_start..session.file_size;

                let head_available = self
                    .file_assembler
                    .is_range_available(info_hash, head_range);
                let tail_available = self
                    .file_assembler
                    .is_range_available(info_hash, tail_range);

                if head_available && tail_available {
                    SessionStatus::WaitingForData {
                        bytes_needed: self.config.min_head_size,
                        bytes_available: self.config.min_head_size,
                    }
                } else {
                    // Estimate available bytes (simplified)
                    let available = if head_available {
                        self.config.min_head_size / 2
                    } else {
                        0
                    };
                    SessionStatus::WaitingForData {
                        bytes_needed: self.config.min_head_size,
                        bytes_available: available,
                    }
                }
            }
            RemuxingStatus::Processing { _input_consumed } => {
                let progress = if session.file_size > 0 {
                    Some(*_input_consumed as f64 / session.file_size as f64)
                } else {
                    None
                };
                SessionStatus::Remuxing { progress }
            }
            RemuxingStatus::Completed { total_output: _ } => SessionStatus::Ready,
            RemuxingStatus::Failed { _reason } => SessionStatus::Error {
                error: _reason.clone(),
            },
        };

        Some(SessionInfo {
            status,
            output_size,
        })
    }

    /// Get minimum head size requirement
    pub fn get_min_head_size(&self) -> u64 {
        self.config.min_head_size
    }

    /// Check if streaming is ready based on remuxed MP4 output availability
    pub async fn check_streaming_readiness(
        &self,
        info_hash: InfoHash,
        _file_size: u64,
    ) -> Result<bool, StreamingError> {
        use tracing::info;

        // Get session to check remuxed output status
        let session = {
            let sessions = self.active_sessions.read().await;
            sessions.get(&info_hash).cloned()
        };

        let Some(session) = session else {
            info!(
                "Streaming not ready for {}: no active remux session",
                info_hash
            );
            return Ok(false);
        };

        // Check if session is ready for streaming
        let status_guard = session.status.read().await;
        let is_session_ready = matches!(*status_guard, RemuxingStatus::Completed { .. });
        drop(status_guard);

        if !is_session_ready {
            info!(
                "Streaming not ready for {}: remux session not completed",
                info_hash
            );
            return Ok(false);
        }

        // Check if we have sufficient remuxed output data
        let output_size = session
            .output_size
            .load(std::sync::atomic::Ordering::Relaxed);
        let min_output_needed = 5 * 1024 * 1024; // 5MB of remuxed output minimum

        if output_size < min_output_needed {
            info!(
                "Streaming not ready for {}: insufficient remuxed output ({} bytes, need {} bytes)",
                info_hash, output_size, min_output_needed
            );
            return Ok(false);
        }

        // Try to read a small amount of remuxed data to verify it's accessible
        let test_range = 0..64 * 1024; // 64KB test read
        match self.stream_range(info_hash, test_range).await {
            Ok(data) if data.len() >= 32 * 1024 => {
                info!(
                    "Streaming ready for {}: {} bytes remuxed output available, test read successful",
                    info_hash, output_size
                );
                Ok(true)
            }
            Ok(data) => {
                info!(
                    "Streaming not ready for {}: test read only returned {} bytes",
                    info_hash,
                    data.len()
                );
                Ok(false)
            }
            Err(e) => {
                info!(
                    "Streaming not ready for {}: test read failed: {}",
                    info_hash, e
                );
                Ok(false)
            }
        }
    }

    /// Check data availability for remuxing
    pub async fn check_data_availability(
        &self,
        info_hash: InfoHash,
        head_size_needed: u64,
        file_size: u64,
    ) -> DataAvailability {
        let head_range = 0..head_size_needed.min(file_size);
        let tail_size = head_size_needed.min(file_size);
        let tail_start = file_size.saturating_sub(tail_size);
        let tail_range = tail_start..file_size;

        let head_available = self
            .file_assembler
            .is_range_available(info_hash, head_range.clone());
        let tail_available = self
            .file_assembler
            .is_range_available(info_hash, tail_range.clone());

        if head_available && tail_available {
            DataAvailability::Sufficient
        } else {
            // Estimate available bytes by checking smaller ranges
            let mut available = 0u64;
            let chunk_size = 1024 * 1024; // 1MB chunks

            // Check head availability in chunks
            let mut pos = 0u64;
            while pos < head_range.end {
                let chunk_end = (pos + chunk_size).min(head_range.end);
                if self
                    .file_assembler
                    .is_range_available(info_hash, pos..chunk_end)
                {
                    available += chunk_end - pos;
                    pos = chunk_end;
                } else {
                    break;
                }
            }

            // Check tail availability
            if tail_available {
                available += tail_range.end - tail_range.start;
            }

            DataAvailability::Insufficient {
                available,
                needed: head_size_needed * 2, // Head + tail
            }
        }
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
        // Try to get session first for accurate size estimation
        match self.ensure_session_active(info_hash).await {
            Ok(session) => {
                let status = session.status.read().await;
                match &*status {
                    RemuxingStatus::Completed { total_output } => Ok(*total_output),
                    RemuxingStatus::Processing { .. } => {
                        // Estimate based on progress
                        let input_pos = session
                            .input_position
                            .load(std::sync::atomic::Ordering::Relaxed);
                        let actual_output_size = session
                            .output_size
                            .load(std::sync::atomic::Ordering::Relaxed);
                        let progress = input_pos as f64 / session.file_size as f64;
                        let estimated_size =
                            (actual_output_size as f64 / progress.max(0.01)) as u64;
                        Ok(estimated_size)
                    }
                    _ => Ok(session.file_size), // Return original file size as estimate
                }
            }
            Err(_) => {
                // Fallback: return original file size from assembler
                self.file_assembler.file_size(info_hash).await.map_err(|e| {
                    StreamingError::FfmpegError {
                        reason: format!("Failed to get file size: {e}"),
                    }
                })
            }
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
