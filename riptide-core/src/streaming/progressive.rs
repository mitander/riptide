//! Progressive streaming support for feeding data to FFmpeg as it becomes available
//!
//! This module provides functionality to start FFmpeg remuxing/transcoding operations
//! before the entire input file is available, enabling true progressive streaming from
//! partial torrent data.
//!
//! ## Architecture
//!
//! The progressive streaming system consists of several components:
//! - `ProgressiveFeeder`: Legacy implementation (kept for compatibility)
//! - `coordinator`: Event-driven coordinator for robust streaming management
//! - `feeder`: Simplified feeder that works with the coordinator

pub mod coordinator;
pub mod diagnostics;
pub mod feeder;

use std::io;
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::Arc;
use std::time::{Duration, Instant};

use tokio::process::{Child, Command};
use tokio::sync::{RwLock, oneshot};
use tokio::time::interval;
use tracing::{debug, error, info, warn};

use crate::storage::{DataError, DataSource};
use crate::streaming::{RemuxingOptions, StreamingError, StreamingResult};
use crate::torrent::InfoHash;

/// Minimum amount of data required to start FFmpeg processing
const MIN_HEADER_SIZE: u64 = 20 * 1024 * 1024; // 20MB for AVI files with headers (increased for better compatibility)
const CHUNK_SIZE: u64 = 1024 * 1024; // 1MB chunks for progressive feeding
const FEED_INTERVAL: Duration = Duration::from_millis(100);
const STALL_TIMEOUT: Duration = Duration::from_secs(1800); // 30 minutes - much longer for progressive streaming

/// Errors specific to progressive streaming
#[derive(thiserror::Error, Debug)]
pub enum ProgressiveError {
    /// FFmpeg process failed with an error
    #[error("FFmpeg process failed: {reason}")]
    ProcessFailed {
        /// Reason for the failure
        reason: String,
    },

    /// Error from the data source
    #[error("Data source error: {0}")]
    DataSource(#[from] DataError),

    /// IO error during file operations
    #[error("IO error: {0}")]
    Io(#[from] io::Error),

    /// Timeout waiting for data to become available
    #[error("Timeout waiting for data")]
    Timeout,

    /// FFmpeg process terminated unexpectedly
    #[error("FFmpeg terminated unexpectedly")]
    UnexpectedTermination,
}

/// State of a progressive streaming session
#[derive(Debug, Clone)]
pub enum ProgressiveState {
    /// Waiting for minimum data to start
    WaitingForData {
        /// Number of bytes needed before streaming can start
        bytes_needed: u64,
    },

    /// Actively feeding data to FFmpeg
    Feeding {
        /// Total bytes fed to FFmpeg so far
        bytes_fed: u64,
        /// Timestamp of last successful progress
        last_progress: Instant,
    },

    /// Completed successfully
    Completed {
        /// Total number of bytes processed
        total_bytes: u64,
    },

    /// Failed with error
    Failed {
        /// Error description
        error: String,
    },
}

/// Manages progressive data feeding to FFmpeg
pub struct ProgressiveFeeder {
    info_hash: InfoHash,
    data_source: Arc<dyn DataSource>,
    input_path: PathBuf,
    output_path: PathBuf,
    state: Arc<RwLock<ProgressiveState>>,
    file_size: u64,
    shutdown_tx: Option<oneshot::Sender<()>>,
}

impl ProgressiveFeeder {
    /// Create a new progressive feeder
    pub fn new(
        info_hash: InfoHash,
        data_source: Arc<dyn DataSource>,
        input_path: PathBuf,
        output_path: PathBuf,
        file_size: u64,
    ) -> Self {
        Self {
            info_hash,
            data_source,
            input_path,
            output_path,
            state: Arc::new(RwLock::new(ProgressiveState::WaitingForData {
                bytes_needed: MIN_HEADER_SIZE.min(file_size),
            })),
            file_size,
            shutdown_tx: None,
        }
    }

    /// Start progressive streaming with FFmpeg
    ///
    /// # Errors
    ///
    /// - `StreamingError::RemuxingFailed` - If FFmpeg cannot be started or initial data cannot be read
    pub async fn start(&mut self, options: &RemuxingOptions) -> StreamingResult<()> {
        // First, let's diagnose the issue
        if let Ok(diagnosis) = diagnostics::diagnose_progressive_streaming_issue(
            self.info_hash,
            Arc::clone(&self.data_source),
        )
        .await
        {
            info!("Progressive streaming diagnosis:\n{}", diagnosis);
        }
        // Check if we have minimum data to start
        let initial_size = MIN_HEADER_SIZE.min(self.file_size);

        debug!(
            "Starting progressive feeder for {}: file_size={}, initial_size={}",
            self.info_hash, self.file_size, initial_size
        );

        match self
            .data_source
            .check_range_availability(self.info_hash, 0..initial_size)
            .await
        {
            Ok(availability) if availability.available => {
                info!(
                    "Starting progressive streaming for {} with {} bytes available",
                    self.info_hash, initial_size
                );
            }
            Ok(availability) => {
                debug!(
                    "Initial data not available for {}: missing_pieces={:?}",
                    self.info_hash, availability.missing_pieces
                );
            }
            _ => {
                debug!(
                    "Waiting for initial {} bytes before starting FFmpeg for {}",
                    initial_size, self.info_hash
                );

                // Wait for initial data with timeout
                let wait_start = Instant::now();
                loop {
                    if wait_start.elapsed() > Duration::from_secs(60) {
                        return Err(StreamingError::RemuxingFailed {
                            reason: "Timeout waiting for initial data".to_string(),
                        });
                    }

                    match self
                        .data_source
                        .check_range_availability(self.info_hash, 0..initial_size)
                        .await
                    {
                        Ok(availability) if availability.available => break,
                        _ => {
                            tokio::time::sleep(Duration::from_millis(500)).await;
                        }
                    }
                }
            }
        }

        debug!(
            "Initial data available for {}, starting FFmpeg process",
            self.info_hash
        );

        // Start FFmpeg process
        debug!("About to start FFmpeg process for {}", self.info_hash);
        let mut ffmpeg_process = self.start_ffmpeg_process(options).await?;
        debug!("FFmpeg process started successfully for {}", self.info_hash);

        // Verify FFmpeg process is still alive immediately after start
        match ffmpeg_process.try_wait() {
            Ok(Some(status)) => {
                error!("FFmpeg process exited immediately with status: {}", status);
                return Err(StreamingError::RemuxingFailed {
                    reason: format!("FFmpeg process exited immediately with status: {status}"),
                });
            }
            Ok(None) => {
                debug!("FFmpeg process confirmed running after start");
            }
            Err(e) => {
                warn!("Failed to check FFmpeg process status after start: {}", e);
            }
        }

        // Write initial data to stdin
        let initial_data = self
            .data_source
            .read_range(self.info_hash, 0..initial_size)
            .await
            .map_err(|e| StreamingError::RemuxingFailed {
                reason: format!("Failed to read initial data: {e}"),
            })?;

        if let Some(stdin) = ffmpeg_process.stdin.as_mut() {
            debug!(
                "Writing {} bytes of initial data to FFmpeg stdin for {}",
                initial_data.len(),
                self.info_hash
            );
            use tokio::io::AsyncWriteExt;

            let write_start = Instant::now();
            stdin
                .write_all(&initial_data)
                .await
                .map_err(|e| StreamingError::RemuxingFailed {
                    reason: format!("Failed to write initial data to FFmpeg: {e}"),
                })?;
            let write_duration = write_start.elapsed();

            debug!(
                "Successfully wrote {} bytes of initial data to FFmpeg stdin for {} in {:?}",
                initial_data.len(),
                self.info_hash,
                write_duration
            );

            // Verify stdin is still available after write
            if ffmpeg_process.stdin.is_none() {
                error!(
                    "FFmpeg stdin became unavailable immediately after initial write for {}",
                    self.info_hash
                );
                return Err(StreamingError::RemuxingFailed {
                    reason: "FFmpeg stdin became unavailable after initial write".to_string(),
                });
            }
        } else {
            error!(
                "FFmpeg stdin not available at startup for {}",
                self.info_hash
            );
            return Err(StreamingError::RemuxingFailed {
                reason: "FFmpeg stdin not available".to_string(),
            });
        }

        debug!(
            "FFmpeg process and initial data feeding completed for {}",
            self.info_hash
        );

        // Start background task to feed more data
        let (shutdown_tx, shutdown_rx) = oneshot::channel();
        self.shutdown_tx = Some(shutdown_tx);

        let feeder_handle = tokio::spawn(self.clone().feed_data_loop(
            ffmpeg_process,
            initial_size,
            shutdown_rx,
        ));

        debug!(
            "Progressive feeder background task started for {}, output: {}",
            self.info_hash,
            self.output_path.display()
        );

        // Return immediately to allow streaming to start, but store the handle for monitoring
        // The remuxer will monitor the feeder's completion status separately
        let info_hash_for_logging = self.info_hash;
        tokio::spawn(async move {
            match feeder_handle.await {
                Ok(Ok(())) => {
                    debug!(
                        "Progressive feeder completed successfully for {}",
                        info_hash_for_logging
                    );
                }
                Ok(Err(e)) => {
                    error!(
                        "Progressive feeder failed for {}: {}",
                        info_hash_for_logging, e
                    );
                }
                Err(e) => {
                    error!(
                        "Progressive feeder task panicked for {}: {}",
                        info_hash_for_logging, e
                    );
                }
            }
        });

        Ok(())
    }

    /// Start FFmpeg process with pipe input
    async fn start_ffmpeg_process(&self, options: &RemuxingOptions) -> StreamingResult<Child> {
        let mut cmd = Command::new("ffmpeg");

        // Basic FFmpeg options
        cmd.arg("-y"); // Overwrite output

        // Input options for progressive streaming
        cmd.arg("-analyzeduration")
            .arg("10000000") // 10 seconds max analysis (enough for AVI headers)
            .arg("-probesize")
            .arg("20971520") // 20MB probe size (matches our initial data size)
            .arg("-err_detect")
            .arg("ignore_err")
            .arg("-fflags")
            .arg("+igndts+ignidx+genpts+discardcorrupt")
            .arg("-avoid_negative_ts")
            .arg("make_zero")
            .arg("-thread_queue_size")
            .arg("1024") // Increase input thread queue for progressive data
            .arg("-i")
            .arg("pipe:0"); // Read from stdin

        // Output options - Video codec
        if options.video_codec == "copy" {
            cmd.arg("-c:v").arg("copy");
        } else {
            // For transcoding, use fast settings
            cmd.arg("-c:v")
                .arg(&options.video_codec)
                .arg("-preset")
                .arg("ultrafast")
                .arg("-crf")
                .arg("28")
                .arg("-profile:v")
                .arg("baseline")
                .arg("-level")
                .arg("3.0")
                .arg("-pix_fmt")
                .arg("yuv420p")
                .arg("-tune")
                .arg("zerolatency")
                .arg("-x264opts")
                .arg("keyint=30:min-keyint=30");
        }

        // Audio codec
        cmd.arg("-c:a").arg(&options.audio_codec);

        // Streaming optimizations - use fragmented MP4 for progressive streaming
        cmd.arg("-movflags")
            .arg("frag_keyframe+empty_moov+default_base_moof");

        // Muxing queue size for output
        cmd.arg("-max_muxing_queue_size").arg("4096");

        // Output format
        cmd.arg("-f").arg("mp4").arg(&self.output_path);

        // Configure process
        cmd.stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);

        info!(
            "Starting FFmpeg for progressive streaming [{}]: {:?}",
            self.info_hash,
            cmd.as_std()
        );

        // Add process resource monitoring
        info!("FFmpeg command for {}: {:?}", self.info_hash, cmd.as_std());

        let mut child = cmd.spawn().map_err(|e| {
            error!(
                "Failed to spawn FFmpeg process for {}: {}",
                self.info_hash, e
            );
            StreamingError::RemuxingFailed {
                reason: format!("Failed to start FFmpeg: {e}"),
            }
        })?;

        // Verify process started successfully
        info!(
            "FFmpeg process spawned successfully for {} (PID: {:?})",
            self.info_hash,
            child.id()
        );

        // Capture stderr for debugging with enhanced monitoring
        if let Some(stderr) = child.stderr.take() {
            let info_hash = self.info_hash;
            tokio::spawn(async move {
                use tokio::io::{AsyncBufReadExt, BufReader};
                let reader = BufReader::new(stderr);
                let mut lines = reader.lines();
                let mut error_lines = Vec::new();

                while let Ok(Some(line)) = lines.next_line().await {
                    // Log all FFmpeg output as info for debugging
                    info!("FFmpeg [{}]: {}", info_hash, line);

                    // Collect error lines for analysis
                    if line.contains("error")
                        || line.contains("Error")
                        || line.contains("ERROR")
                        || line.contains("invalid")
                        || line.contains("Invalid")
                        || line.contains("failed")
                        || line.contains("Failed")
                    {
                        error_lines.push(line.clone());
                    }

                    // Immediate warning for critical errors
                    if line.contains("Invalid data found")
                        || line.contains("pipe")
                        || line.contains("Connection")
                    {
                        warn!("FFmpeg critical error [{}]: {}", info_hash, line);
                    }
                }

                if !error_lines.is_empty() {
                    error!(
                        "FFmpeg errors collected for {}: {:?}",
                        info_hash, error_lines
                    );
                }

                info!("FFmpeg stderr monitoring ended for {}", info_hash);
            });
        }

        Ok(child)
    }

    /// Background loop to feed data progressively
    #[allow(clippy::too_many_lines)]
    async fn feed_data_loop(
        self,
        mut ffmpeg_process: Child,
        mut bytes_written: u64,
        mut shutdown_rx: oneshot::Receiver<()>,
    ) -> Result<(), ProgressiveError> {
        let mut feed_timer = interval(FEED_INTERVAL);
        let mut last_progress = Instant::now();
        let mut consecutive_errors = 0;

        // Update state
        {
            let mut state = self.state.write().await;
            *state = ProgressiveState::Feeding {
                bytes_fed: bytes_written,
                last_progress,
            };
        }

        loop {
            tokio::select! {
                _ = feed_timer.tick() => {
                    // Check if more data is available
                    if bytes_written >= self.file_size {
                        debug!("All data fed to FFmpeg for {} ({}/{} bytes), closing stdin and waiting for completion",
                               self.info_hash, bytes_written, self.file_size);
                        // Close stdin to signal EOF to FFmpeg, but only once
                        if ffmpeg_process.stdin.is_some() {
                            debug!("Closing FFmpeg stdin for {}", self.info_hash);
                            if let Some(stdin) = ffmpeg_process.stdin.take() {
                                drop(stdin);
                            }
                        }
                        // Continue waiting for FFmpeg to finish processing
                        continue;
                    }

                    // Calculate next chunk - use smaller chunks for better responsiveness
                    let chunk_end = (bytes_written + CHUNK_SIZE).min(self.file_size);

                    // Check if chunk is available
                    debug!(
                        "Progressive feeder checking chunk availability for {}: {}-{} ({} bytes)",
                        self.info_hash, bytes_written, chunk_end, chunk_end - bytes_written
                    );
                    match self.data_source
                        .check_range_availability(self.info_hash, bytes_written..chunk_end)
                        .await
                    {
                        Ok(availability) if availability.available => {
                            debug!(
                                "Chunk {}-{} available for {}, writing to FFmpeg",
                                bytes_written, chunk_end, self.info_hash
                            );
                            // Read and write chunk to pipe
                            let chunk_write_start = Instant::now();
                            debug!(
                                "Attempting to write chunk {}-{} ({} bytes) for {}",
                                bytes_written, chunk_end, chunk_end - bytes_written, self.info_hash
                            );

                            match self.write_chunk_to_pipe(&mut ffmpeg_process, bytes_written, chunk_end).await {
                                Ok(()) => {
                                    let write_duration = chunk_write_start.elapsed();
                                    bytes_written = chunk_end;
                                    last_progress = Instant::now();
                                    consecutive_errors = 0;

                                    // Update state
                                    let mut state = self.state.write().await;
                                    *state = ProgressiveState::Feeding {
                                        bytes_fed: bytes_written,
                                        last_progress,
                                    };

                                    debug!(
                                        "Wrote chunk {}-{} in {:?} for {} (total: {}/{} bytes, {:.1}%)",
                                        bytes_written - CHUNK_SIZE,
                                        bytes_written,
                                        write_duration,
                                        self.info_hash,
                                        bytes_written,
                                        self.file_size,
                                        (bytes_written as f64 / self.file_size as f64) * 100.0
                                    );
                                }
                                Err(e) => {
                                    let write_duration = chunk_write_start.elapsed();
                                    consecutive_errors += 1;
                                    error!(
                                        "Failed to write chunk {}-{} after {:?} for {} (attempt {}): {}",
                                        bytes_written, chunk_end, write_duration, self.info_hash, consecutive_errors, e
                                    );

                                    // Check if FFmpeg process is still alive
                                    match ffmpeg_process.try_wait() {
                                        Ok(Some(status)) => {
                                            error!(
                                                "FFmpeg process exited during chunk write with status: {} for {}",
                                                status, self.info_hash
                                            );
                                            return Err(ProgressiveError::ProcessFailed {
                                                reason: format!("FFmpeg exited with status: {status}"),
                                            });
                                        }
                                        Ok(None) => {
                                            // Process is still running - check stdin status
                                            let stdin_available = ffmpeg_process.stdin.is_some();
                                            debug!(
                                                "FFmpeg still running but write failed for {} (stdin available: {})",
                                                self.info_hash, stdin_available
                                            );

                                            if !stdin_available {
                                                error!(
                                                    "FFmpeg stdin was closed unexpectedly while process running for {}",
                                                    self.info_hash
                                                );
                                                return Err(ProgressiveError::ProcessFailed {
                                                    reason: "FFmpeg stdin closed unexpectedly while process running".to_string(),
                                                });
                                            }
                                        }
                                        Err(e) => {
                                            error!(
                                                "Failed to check FFmpeg process status for {}: {}",
                                                self.info_hash, e
                                            );
                                        }
                                    }

                                    if consecutive_errors > 3 {
                                        error!(
                                            "Too many consecutive errors ({}) for chunk writes, aborting for {}",
                                            consecutive_errors, self.info_hash
                                        );
                                        return Err(ProgressiveError::ProcessFailed {
                                            reason: format!("Too many consecutive pipe write errors: {e}"),
                                        });
                                    }

                                    // Add delay before retry to avoid busy loop
                                    tokio::time::sleep(Duration::from_millis(100)).await;
                                }
                            }
                        }
                        Ok(availability) => {
                            // Data not yet available - this is the critical part
                            debug!(
                                "Chunk {}-{} not available for {} (missing {} pieces), waiting for torrent download...",
                                bytes_written, chunk_end, self.info_hash, availability.missing_pieces.len()
                            );

                            // Log detailed progress information
                            let progress_pct = (bytes_written as f64 / self.file_size as f64) * 100.0;
                            let elapsed = last_progress.elapsed().as_secs();
                            debug!(
                                "Waiting for data for {}: progress {}/{} bytes ({:.1}%), missing pieces: {:?}, elapsed: {}s",
                                self.info_hash,
                                bytes_written, self.file_size, progress_pct,
                                availability.missing_pieces,
                                elapsed
                            );

                            // The remuxer monitoring task now handles streaming position updates
                            // based on progressive feeder progress to ensure sequential piece downloading

                            // Never timeout as long as we haven't reached the end of file
                            if last_progress.elapsed() > STALL_TIMEOUT {
                                debug!(
                                    "Progressive streaming waiting {} seconds for data at byte {}/{} ({:.1}%)",
                                    last_progress.elapsed().as_secs(),
                                    bytes_written, self.file_size,
                                    (bytes_written as f64 / self.file_size as f64) * 100.0
                                );

                                // Reset the progress timer to prevent spam
                                last_progress = Instant::now();
                            }
                        }
                        Err(e) => {
                            error!("Error checking data availability: {}", e);
                            return Err(ProgressiveError::DataSource(e));
                        }
                    }
                }

                _ = &mut shutdown_rx => {
                    info!("Progressive feeder shutdown requested for {}", self.info_hash);
                    // Close stdin to signal EOF to FFmpeg
                    if let Some(stdin) = ffmpeg_process.stdin.take() {
                        drop(stdin);
                    }
                    break;
                }

                status = ffmpeg_process.wait() => {
                    match status {
                        Ok(exit_status) if exit_status.success() => {
                            info!("FFmpeg completed successfully for {} with exit status: {} (processed {}/{} bytes)",
                                  self.info_hash, exit_status, bytes_written, self.file_size);
                            break;
                        }
                        Ok(exit_status) => {
                            // FFmpeg failed - provide context
                            error!(
                                "FFmpeg exited with status {} for {} (progress: {}/{} bytes, {:.1}%)",
                                exit_status, self.info_hash, bytes_written, self.file_size,
                                (bytes_written as f64 / self.file_size as f64) * 100.0
                            );

                            // Check if we have a valid partial output
                            if self.output_path.exists() {
                                if let Ok(metadata) = std::fs::metadata(&self.output_path) {
                                    if metadata.len() > 0 {
                                        info!(
                                            "Partial output available ({} bytes) despite FFmpeg exit with status {}",
                                            metadata.len(), exit_status
                                        );
                                        break;
                                    } else {
                                        warn!("Output file exists but is empty");
                                    }
                                } else {
                                    warn!("Cannot read output file metadata");
                                }
                            } else {
                                warn!("No output file generated by FFmpeg");
                            }

                            return Err(ProgressiveError::ProcessFailed {
                                reason: format!("FFmpeg exited with status: {exit_status}"),
                            });
                        }
                        Err(e) => {
                            error!("Failed to wait for FFmpeg process: {}", e);
                            return Err(ProgressiveError::ProcessFailed {
                                reason: format!("Failed to wait for FFmpeg: {e}"),
                            });
                        }
                    }
                }
            }
        }

        // Update final state
        let mut state = self.state.write().await;
        *state = ProgressiveState::Completed {
            total_bytes: bytes_written,
        };

        // Clean up input file
        if let Err(e) = std::fs::remove_file(&self.input_path) {
            warn!("Failed to remove input file: {}", e);
        }

        Ok(())
    }

    /// Write a chunk of data to FFmpeg stdin pipe
    async fn write_chunk_to_pipe(
        &self,
        ffmpeg_process: &mut Child,
        start: u64,
        end: u64,
    ) -> Result<(), ProgressiveError> {
        let chunk_size = end - start;

        debug!(
            "Attempting to write chunk {}-{} ({} bytes) for {}",
            start, end, chunk_size, self.info_hash
        );

        // Check if FFmpeg process is still alive before attempting to write
        match ffmpeg_process.try_wait() {
            Ok(Some(status)) => {
                error!(
                    "FFmpeg process exited before write with status: {} for {} (chunk {}-{})",
                    status, self.info_hash, start, end
                );
                return Err(ProgressiveError::ProcessFailed {
                    reason: format!("FFmpeg process exited with status: {status}"),
                });
            }
            Ok(None) => {
                debug!(
                    "FFmpeg process confirmed alive for {} (PID: {:?})",
                    self.info_hash,
                    ffmpeg_process.id()
                );
            }
            Err(e) => {
                debug!(
                    "Failed to check FFmpeg process status for {}: {}",
                    self.info_hash, e
                );
            }
        }

        let data = self
            .data_source
            .read_range(self.info_hash, start..end)
            .await?;

        debug!(
            "Read {} bytes from data source for chunk {}-{} for {}",
            data.len(),
            start,
            end,
            self.info_hash
        );

        if let Some(stdin) = ffmpeg_process.stdin.as_mut() {
            debug!(
                "Writing {} bytes to FFmpeg stdin for chunk {}-{} for {}",
                data.len(),
                start,
                end,
                self.info_hash
            );

            use tokio::io::AsyncWriteExt;

            // Add timeout to write operation to detect hangs
            let write_result = tokio::time::timeout(
                Duration::from_secs(30), // 30 second timeout for writes
                stdin.write_all(&data),
            )
            .await;

            match write_result {
                Ok(Ok(())) => {
                    debug!(
                        "Successfully wrote {} bytes for chunk {}-{} for {}",
                        data.len(),
                        start,
                        end,
                        self.info_hash
                    );
                }
                Ok(Err(e)) => {
                    error!(
                        "Write error for chunk {}-{} for {}: {}",
                        start, end, self.info_hash, e
                    );
                    return Err(ProgressiveError::ProcessFailed {
                        reason: format!("Failed to write to FFmpeg stdin: {e}"),
                    });
                }
                Err(_) => {
                    error!(
                        "Write timed out after 30s for chunk {}-{} for {}",
                        start, end, self.info_hash
                    );
                    return Err(ProgressiveError::ProcessFailed {
                        reason: "FFmpeg stdin write timed out after 30 seconds".to_string(),
                    });
                }
            }
        } else {
            error!(
                "FFmpeg stdin not available when writing chunk {}-{} for {}",
                start, end, self.info_hash
            );

            // Check if process exited after stdin became None
            match ffmpeg_process.try_wait() {
                Ok(Some(status)) => {
                    error!(
                        "FFmpeg process exited (status: {}) causing stdin unavailability for chunk {}-{} for {}",
                        status, start, end, self.info_hash
                    );
                    return Err(ProgressiveError::ProcessFailed {
                        reason: format!("FFmpeg process exited with status: {status}"),
                    });
                }
                Ok(None) => {
                    error!(
                        "FFmpeg stdin not available but process still running for {} (chunk {}-{})",
                        self.info_hash, start, end
                    );
                }
                Err(e) => {
                    error!(
                        "Failed to check FFmpeg status after stdin became unavailable for {}: {}",
                        self.info_hash, e
                    );
                }
            }
            return Err(ProgressiveError::ProcessFailed {
                reason: format!("FFmpeg stdin not available for chunk {start}-{end}"),
            });
        }

        Ok(())
    }

    /// Get current state
    pub async fn state(&self) -> ProgressiveState {
        self.state.read().await.clone()
    }

    /// Check if output is ready for streaming
    pub async fn is_output_ready(&self) -> bool {
        self.output_path.exists()
            && matches!(
                &*self.state.read().await,
                ProgressiveState::Feeding { .. } | ProgressiveState::Completed { .. }
            )
    }

    /// Shutdown the progressive feeder
    pub fn shutdown(&mut self) {
        if let Some(tx) = self.shutdown_tx.take() {
            let _ = tx.send(());
        }
    }
}

impl Clone for ProgressiveFeeder {
    fn clone(&self) -> Self {
        Self {
            info_hash: self.info_hash,
            data_source: self.data_source.clone(),
            input_path: self.input_path.clone(),
            output_path: self.output_path.clone(),
            state: self.state.clone(),
            file_size: self.file_size,
            shutdown_tx: None, // Don't clone shutdown channel
        }
    }
}

impl Drop for ProgressiveFeeder {
    fn drop(&mut self) {
        self.shutdown();
    }
}

impl std::fmt::Debug for ProgressiveFeeder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ProgressiveFeeder")
            .field("info_hash", &self.info_hash)
            .field("input_path", &self.input_path)
            .field("output_path", &self.output_path)
            .field("file_size", &self.file_size)
            .finish()
    }
}
