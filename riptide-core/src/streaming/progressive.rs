//! Progressive streaming support for feeding data to FFmpeg as it becomes available
//!
//! This module provides functionality to start FFmpeg remuxing/transcoding operations
//! before the entire input file is available, enabling true progressive streaming from
//! partial torrent data.

use std::io;
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::Arc;
use std::time::{Duration, Instant};

use tokio::process::{Child, Command};
use tokio::sync::{RwLock, oneshot};
use tokio::time::interval;
use tracing::{debug, error, info, trace, warn};

use crate::storage::{DataError, DataSource};
use crate::streaming::{RemuxingOptions, StreamingError, StreamingResult};
use crate::torrent::InfoHash;

/// Minimum amount of data required to start FFmpeg processing
const MIN_HEADER_SIZE: u64 = 10 * 1024 * 1024; // 10MB for AVI files with headers
const CHUNK_SIZE: u64 = 1024 * 1024; // 1MB chunks for progressive feeding
const FEED_INTERVAL: Duration = Duration::from_millis(100);
const STALL_TIMEOUT: Duration = Duration::from_secs(300); // 5 minutes for large files

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
            stdin
                .write_all(&initial_data)
                .await
                .map_err(|e| StreamingError::RemuxingFailed {
                    reason: format!("Failed to write initial data to FFmpeg: {e}"),
                })?;
            debug!(
                "Successfully wrote initial data to FFmpeg stdin for {}",
                self.info_hash
            );
        } else {
            error!("FFmpeg stdin not available for {}", self.info_hash);
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

        // Don't wait for completion - let it run in background
        tokio::spawn(async move {
            if let Err(e) = feeder_handle.await {
                error!("Progressive feeder task failed: {}", e);
            }
        });

        debug!(
            "Progressive feeder background task started for {}, output: {}",
            self.info_hash,
            self.output_path.display()
        );

        Ok(())
    }

    /// Start FFmpeg process with pipe input
    async fn start_ffmpeg_process(&self, options: &RemuxingOptions) -> StreamingResult<Child> {
        let mut cmd = Command::new("ffmpeg");

        // Basic FFmpeg options
        cmd.arg("-y") // Overwrite output
            .arg("-i")
            .arg("pipe:0"); // Read from stdin

        // Video codec
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

        // Allow incomplete input and handle corrupt data gracefully
        cmd.arg("-analyzeduration")
            .arg("10M")
            .arg("-probesize")
            .arg("10M")
            .arg("-err_detect")
            .arg("ignore_err")
            .arg("-fflags")
            .arg("+igndts+ignidx")
            .arg("-avoid_negative_ts")
            .arg("make_zero");

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

        let mut child = cmd.spawn().map_err(|e| StreamingError::RemuxingFailed {
            reason: format!("Failed to start FFmpeg: {e}"),
        })?;

        // Capture stderr for debugging
        if let Some(stderr) = child.stderr.take() {
            let info_hash = self.info_hash;
            tokio::spawn(async move {
                use tokio::io::{AsyncBufReadExt, BufReader};
                let reader = BufReader::new(stderr);
                let mut lines = reader.lines();
                while let Ok(Some(line)) = lines.next_line().await {
                    debug!("FFmpeg stderr [{}]: {}", info_hash, line);
                }
            });
        }

        Ok(child)
    }

    /// Background loop to feed data progressively
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

                    // Calculate next chunk
                    let chunk_end = (bytes_written + CHUNK_SIZE).min(self.file_size);

                    // Check if chunk is available
                    match self.data_source
                        .check_range_availability(self.info_hash, bytes_written..chunk_end)
                        .await
                    {
                        Ok(availability) if availability.available => {
                            // Read and write chunk to pipe
                            match self.write_chunk_to_pipe(&mut ffmpeg_process, bytes_written, chunk_end).await {
                                Ok(()) => {
                                    bytes_written = chunk_end;
                                    last_progress = Instant::now();
                                    consecutive_errors = 0;

                                    // Update state
                                    let mut state = self.state.write().await;
                                    *state = ProgressiveState::Feeding {
                                        bytes_fed: bytes_written,
                                        last_progress,
                                    };

                                    trace!(
                                        "Wrote chunk {}-{} to FFmpeg pipe for {}",
                                        bytes_written - CHUNK_SIZE,
                                        bytes_written,
                                        self.info_hash
                                    );
                                }
                                Err(e) => {
                                    consecutive_errors += 1;
                                    warn!(
                                        "Failed to write chunk to pipe for {} (attempt {}): {}",
                                        self.info_hash, consecutive_errors, e
                                    );

                                    if consecutive_errors > 3 {
                                        return Err(ProgressiveError::ProcessFailed {
                                            reason: format!("Too many consecutive pipe write errors: {e}"),
                                        });
                                    }
                                }
                            }
                        }
                        Ok(_) => {
                            // Data not yet available
                            if last_progress.elapsed() > STALL_TIMEOUT {
                                warn!(
                                    "Progressive feeding stalled for {} at byte {}",
                                    self.info_hash, bytes_written
                                );
                                return Err(ProgressiveError::Timeout);
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
                            info!("FFmpeg completed successfully for {} with exit status: {}", self.info_hash, exit_status);
                            break;
                        }
                        Ok(exit_status) => {
                            // FFmpeg failed - check if it's due to incomplete data
                            warn!(
                                "FFmpeg exited with non-zero status {} for {} (fed {}/{} bytes)",
                                exit_status, self.info_hash, bytes_written, self.file_size
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
        let data = self
            .data_source
            .read_range(self.info_hash, start..end)
            .await?;

        if let Some(stdin) = ffmpeg_process.stdin.as_mut() {
            debug!(
                "Writing chunk {}-{} ({} bytes) to FFmpeg stdin for {}",
                start,
                end,
                data.len(),
                self.info_hash
            );
            use tokio::io::AsyncWriteExt;
            stdin
                .write_all(&data)
                .await
                .map_err(|e| ProgressiveError::ProcessFailed {
                    reason: format!("Failed to write to FFmpeg stdin: {e}"),
                })?;
            debug!(
                "Successfully wrote chunk to FFmpeg stdin for {}",
                self.info_hash
            );
        } else {
            error!(
                "FFmpeg stdin not available when writing chunk for {}",
                self.info_hash
            );
            return Err(ProgressiveError::ProcessFailed {
                reason: "FFmpeg stdin not available".to_string(),
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
