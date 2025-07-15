//! Real-time remuxer for progressive streaming
//!
//! This module provides a streaming remuxer that processes media data as it arrives,
//! enabling playback to begin after minimal download. Unlike batch remuxing, this
//! component outputs remuxed chunks immediately as FFmpeg produces them, reducing
//! time-to-first-byte from 85%+ to ~2MB download.

use std::io;
use std::process::Stdio;
use std::sync::Arc;
use std::time::{Duration, Instant};

use thiserror::Error;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt};
use tokio::process::{Child, Command as TokioCommand};
use tokio::sync::{RwLock, mpsc};
use tokio::task::JoinHandle;
use tracing::{debug, error, info, warn};

/// Errors that can occur during remuxing operations
#[derive(Debug, Error)]
pub enum RemuxError {
    /// Failed to start FFmpeg process
    #[error("Failed to start FFmpeg: {0}")]
    ProcessStart(io::Error),

    /// FFmpeg process failed with non-zero exit status
    #[error("FFmpeg remuxing failed with exit status: {0}")]
    ProcessFailed(std::process::ExitStatus),

    /// Failed to write data to FFmpeg stdin
    #[error("Failed to write to FFmpeg stdin: {0}")]
    WriteError(io::Error),

    /// Failed to read from FFmpeg stdout
    #[error("Failed to read from FFmpeg stdout: {0}")]
    ReadError(io::Error),

    /// Remuxing pipeline was terminated
    #[error("Remuxing pipeline terminated")]
    PipelineTerminated,

    /// Invalid configuration provided
    #[error("Invalid remuxer configuration: {0}")]
    InvalidConfig(String),

    /// Buffer overflow during remuxing
    #[error("Remux buffer full, cannot accept more data")]
    BufferFull,

    /// FFmpeg process crashed unexpectedly
    #[error("FFmpeg process crashed")]
    ProcessCrashed,
}

/// Configuration for real-time remuxing
#[derive(Debug, Clone)]
pub struct RemuxConfig {
    /// Input format (avi, mkv, mp4, auto)
    pub input_format: String,

    /// Size of chunks to read from FFmpeg stdout
    pub chunk_size: usize,

    /// Maximum number of chunks to buffer
    pub buffer_capacity: usize,

    /// Timeout for FFmpeg operations
    pub operation_timeout: Duration,

    /// Whether to use fragmented MP4 for streaming
    pub use_fragmented_mp4: bool,

    /// Analyze duration for input format detection (microseconds)
    pub analyze_duration: u64,

    /// Probe size for input format detection (bytes)
    pub probe_size: u64,

    /// Additional FFmpeg arguments
    pub extra_args: Vec<String>,
}

impl Default for RemuxConfig {
    fn default() -> Self {
        Self {
            input_format: "auto".to_string(),
            chunk_size: 256 * 1024, // 256KB chunks for low latency
            buffer_capacity: 128,   // 32MB buffer (128 * 256KB)
            operation_timeout: Duration::from_secs(30),
            use_fragmented_mp4: true,
            analyze_duration: 10_000_000, // 10 seconds
            probe_size: 20_971_520,       // 20MB
            extra_args: Vec::new(),
        }
    }
}

impl RemuxConfig {
    /// Creates configuration optimized for AVI files
    ///
    /// AVI files need extended analysis to properly detect duration
    /// and avoid truncation issues.
    pub fn for_avi() -> Self {
        Self {
            input_format: "avi".to_string(),
            analyze_duration: 100_000_000, // 100 seconds
            probe_size: 100_000_000,       // 100MB
            ..Default::default()
        }
    }

    /// Creates configuration optimized for low-latency streaming
    ///
    /// Uses smaller chunks and buffers for faster response times.
    pub fn low_latency() -> Self {
        Self {
            chunk_size: 128 * 1024, // 128KB chunks
            buffer_capacity: 64,    // 8MB buffer
            operation_timeout: Duration::from_secs(15),
            ..Default::default()
        }
    }

    /// Builds FFmpeg command arguments for remuxing
    ///
    /// Uses copy codecs for fast remuxing without re-encoding.
    pub fn build_ffmpeg_args(&self) -> Vec<String> {
        let mut args = vec![
            "-y".to_string(), // Overwrite output
            "-analyzeduration".to_string(),
            self.analyze_duration.to_string(),
            "-probesize".to_string(),
            self.probe_size.to_string(),
            "-err_detect".to_string(),
            "ignore_err".to_string(),
            "-fflags".to_string(),
            "+igndts+ignidx+genpts+discardcorrupt+nobuffer".to_string(),
            "-avoid_negative_ts".to_string(),
            "make_zero".to_string(),
            "-thread_queue_size".to_string(),
            "2048".to_string(),
        ];

        // Input format if not auto-detect
        if self.input_format != "auto" {
            args.extend(["-f".to_string(), self.input_format.clone()]);
        }

        // Input from stdin
        args.extend(["-i".to_string(), "pipe:0".to_string()]);

        // Copy codecs for remuxing (no transcoding)
        args.extend([
            "-c:v".to_string(),
            "copy".to_string(),
            "-c:a".to_string(),
            "copy".to_string(),
        ]);

        // MP4 container options - use fragmented MP4 for streaming (faststart doesn't work with pipes)
        if self.use_fragmented_mp4 {
            if self.input_format.to_lowercase() == "avi" {
                // For AVI files, use fragmented MP4 with larger fragments to minimize duration issues
                args.extend([
                    "-movflags".to_string(),
                    "frag_keyframe+empty_moov+default_base_moof+flush_packets".to_string(),
                    "-frag_duration".to_string(),
                    "5000000".to_string(), // 5 seconds fragments for AVI to preserve duration
                    "-copyts".to_string(), // Copy timestamps to preserve original timing
                ]);
            } else {
                // For other formats, use standard fragmented MP4 for streaming
                args.extend([
                    "-movflags".to_string(),
                    "frag_keyframe+empty_moov+default_base_moof".to_string(),
                    "-frag_duration".to_string(),
                    "2000000".to_string(), // 2 seconds fragments
                ]);
            }
        }

        // Output format and destination
        args.extend([
            "-f".to_string(),
            "mp4".to_string(),
            "-max_muxing_queue_size".to_string(),
            "4096".to_string(),
        ]);

        // Add extra arguments
        args.extend(self.extra_args.clone());

        // Output to stdout
        args.push("pipe:1".to_string());

        args
    }
}

/// Represents a remuxed chunk ready for serving
#[derive(Debug, Clone)]
pub struct RemuxedChunk {
    /// Offset in the output stream
    pub offset: u64,

    /// The remuxed data
    pub data: Vec<u8>,

    /// Whether this contains header information
    pub is_header: bool,

    /// When this chunk was produced
    pub timestamp: Instant,
}

/// Status of a remuxing session
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RemuxStatus {
    /// Remuxer is initializing
    Initializing,

    /// Actively remuxing data
    Running,

    /// Completed successfully
    Completed,

    /// Failed with an error
    Failed,

    /// Terminated by user request
    Terminated,
}

/// Handle for controlling a remuxing session
pub struct RemuxHandle {
    /// Channel for sending input data
    pub input_sender: mpsc::Sender<Vec<u8>>,

    /// Channel for receiving remuxed chunks
    pub output_receiver: mpsc::Receiver<RemuxedChunk>,

    /// Current remuxing status
    pub status: Arc<RwLock<RemuxStatus>>,

    /// Handle to the FFmpeg process
    process_handle: JoinHandle<Result<(), RemuxError>>,
}

impl RemuxHandle {
    /// Checks if the remuxer is still running
    pub async fn is_running(&self) -> bool {
        let status = *self.status.read().await;
        matches!(status, RemuxStatus::Initializing | RemuxStatus::Running)
    }

    /// Sends input data to the remuxer
    ///
    /// # Errors
    ///
    /// - `RemuxError::PipelineTerminated` - If the pipeline has stopped
    pub async fn send_input(&self, data: Vec<u8>) -> Result<(), RemuxError> {
        self.input_sender
            .send(data)
            .await
            .map_err(|_| RemuxError::PipelineTerminated)
    }

    /// Receives the next remuxed chunk
    ///
    /// Returns None when remuxing is complete.
    pub async fn recv_chunk(&mut self) -> Option<RemuxedChunk> {
        self.output_receiver.recv().await
    }

    /// Terminates the remuxing session
    pub async fn terminate(&self) {
        *self.status.write().await = RemuxStatus::Terminated;
        self.process_handle.abort();
    }
}

/// Real-time remuxer for progressive streaming
///
/// This component uses FFmpeg to remux media containers in real-time,
/// allowing streaming to begin after minimal download. It focuses on
/// container format conversion without re-encoding streams.
pub struct RealtimeRemuxer {
    config: RemuxConfig,
}

impl RealtimeRemuxer {
    /// Creates a new real-time remuxer
    ///
    /// # Errors
    ///
    /// - `RemuxError::InvalidConfig` - If configuration is invalid
    pub fn new(config: RemuxConfig) -> Result<Self, RemuxError> {
        // Validate configuration
        if config.chunk_size == 0 {
            return Err(RemuxError::InvalidConfig(
                "chunk_size must be greater than 0".to_string(),
            ));
        }

        if config.buffer_capacity == 0 {
            return Err(RemuxError::InvalidConfig(
                "buffer_capacity must be greater than 0".to_string(),
            ));
        }

        Ok(Self { config })
    }

    /// Starts a real-time remuxing session
    ///
    /// Returns a handle for controlling the remuxing process.
    /// Data can be sent through the handle's input channel and
    /// remuxed chunks will be available on the output channel.
    ///
    /// # Errors
    ///
    /// - `RemuxError::ProcessStart` - If FFmpeg fails to start
    pub async fn start(self) -> Result<RemuxHandle, RemuxError> {
        let (input_tx, input_rx) = mpsc::channel::<Vec<u8>>(16);
        let (output_tx, output_rx) = mpsc::channel::<RemuxedChunk>(self.config.buffer_capacity);
        let status = Arc::new(RwLock::new(RemuxStatus::Initializing));

        // Build FFmpeg command
        let mut cmd = TokioCommand::new("ffmpeg");
        for arg in self.config.build_ffmpeg_args() {
            cmd.arg(arg);
        }

        // Configure process pipes
        cmd.stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        info!("Starting FFmpeg remuxing process");
        debug!("FFmpeg command: {:?}", cmd);

        let mut child = cmd.spawn().map_err(RemuxError::ProcessStart)?;

        // Extract pipes
        let stdin = child.stdin.take().ok_or_else(|| {
            RemuxError::ProcessStart(io::Error::other("Failed to get stdin handle"))
        })?;

        let stdout = child.stdout.take().ok_or_else(|| {
            RemuxError::ProcessStart(io::Error::other("Failed to get stdout handle"))
        })?;

        let stderr = child.stderr.take();

        // Start stderr logging
        if let Some(stderr) = stderr {
            tokio::spawn(async move {
                let mut reader = tokio::io::BufReader::new(stderr);
                let mut line = String::new();

                loop {
                    line.clear();
                    match reader.read_line(&mut line).await {
                        Ok(0) => break,
                        Ok(_) => {
                            if !line.trim().is_empty() {
                                debug!("FFmpeg: {}", line.trim());
                            }
                        }
                        Err(e) => {
                            warn!("Error reading FFmpeg stderr: {}", e);
                            break;
                        }
                    }
                }
            });
        }

        // Start processing task
        let config = self.config.clone();
        let status_clone = Arc::clone(&status);

        let process_handle = tokio::spawn(async move {
            let result = Self::process_remuxing(
                child,
                stdin,
                stdout,
                input_rx,
                output_tx,
                config,
                Arc::clone(&status_clone),
            )
            .await;

            // Update final status
            let final_status = match result {
                Ok(_) => RemuxStatus::Completed,
                Err(_) => RemuxStatus::Failed,
            };
            *status_clone.write().await = final_status;

            result
        });

        // Mark as running
        *status.write().await = RemuxStatus::Running;

        Ok(RemuxHandle {
            input_sender: input_tx,
            output_receiver: output_rx,
            status,
            process_handle,
        })
    }

    /// Main remuxing processing loop
    async fn process_remuxing(
        mut child: Child,
        stdin: tokio::process::ChildStdin,
        stdout: tokio::process::ChildStdout,
        mut input_rx: mpsc::Receiver<Vec<u8>>,
        output_tx: mpsc::Sender<RemuxedChunk>,
        config: RemuxConfig,
        status: Arc<RwLock<RemuxStatus>>,
    ) -> Result<(), RemuxError> {
        // Input writer task
        let mut stdin_writer = stdin;
        let write_handle = tokio::spawn(async move {
            while let Some(data) = input_rx.recv().await {
                match tokio::time::timeout(config.operation_timeout, stdin_writer.write_all(&data))
                    .await
                {
                    Ok(Ok(_)) => {
                        debug!("Wrote {} bytes to FFmpeg stdin", data.len());
                    }
                    Ok(Err(e)) => {
                        error!("Failed to write to FFmpeg stdin: {}", e);
                        return Err(RemuxError::WriteError(e));
                    }
                    Err(_) => {
                        error!("Write to FFmpeg stdin timed out");
                        return Err(RemuxError::WriteError(io::Error::new(
                            io::ErrorKind::TimedOut,
                            "Write timeout",
                        )));
                    }
                }
            }

            // Close stdin to signal EOF
            drop(stdin_writer);
            info!("Closed FFmpeg stdin");
            Ok(())
        });

        // Output reader task
        let mut stdout_reader = stdout;
        let chunk_size = config.chunk_size;
        let read_timeout = config.operation_timeout;
        let output_tx_clone = output_tx.clone();

        let read_handle = tokio::spawn(async move {
            let mut offset = 0u64;
            let mut buffer = vec![0u8; chunk_size];
            let mut header_sent = false;

            loop {
                match tokio::time::timeout(read_timeout, stdout_reader.read(&mut buffer)).await {
                    Ok(Ok(0)) => {
                        info!("FFmpeg stdout closed, remuxing complete");
                        break;
                    }
                    Ok(Ok(n)) => {
                        let data = buffer[..n].to_vec();

                        // Check if this is header data (first 32KB)
                        let is_header = offset < 32768 && !header_sent;
                        if is_header {
                            header_sent = true;
                            info!("Sending MP4 header chunk ({} bytes)", n);
                        }

                        let chunk = RemuxedChunk {
                            offset,
                            data,
                            is_header,
                            timestamp: Instant::now(),
                        };

                        offset += n as u64;

                        if output_tx_clone.send(chunk).await.is_err() {
                            warn!("Output channel closed, stopping read");
                            break;
                        }

                        // Log progress
                        if offset % (5 * 1024 * 1024) == 0 {
                            info!("Remuxed {} MB", offset / 1024 / 1024);
                        }
                    }
                    Ok(Err(e)) => {
                        error!("Failed to read from FFmpeg stdout: {}", e);
                        return Err(RemuxError::ReadError(e));
                    }
                    Err(_) => {
                        // Timeout - check if we should terminate
                        if *status.read().await == RemuxStatus::Terminated {
                            info!("Remuxing terminated by user");
                            break;
                        }
                        debug!("Read timeout, retrying...");
                        continue;
                    }
                }
            }

            Ok(())
        });

        // Monitor child process
        let monitor_handle = tokio::spawn(async move {
            match child.wait().await {
                Ok(exit_status) => {
                    if exit_status.success() {
                        info!("FFmpeg remuxing completed successfully");
                        Ok(())
                    } else {
                        error!("FFmpeg remuxing failed: {}", exit_status);
                        Err(RemuxError::ProcessFailed(exit_status))
                    }
                }
                Err(e) => {
                    error!("Failed to wait for FFmpeg process: {}", e);
                    Err(RemuxError::ProcessStart(e))
                }
            }
        });

        // Wait for tasks to complete
        tokio::select! {
            write_result = write_handle => {
                write_result.map_err(|_| RemuxError::PipelineTerminated)??;
            }
            read_result = read_handle => {
                read_result.map_err(|_| RemuxError::PipelineTerminated)??;
            }
            monitor_result = monitor_handle => {
                monitor_result.map_err(|_| RemuxError::PipelineTerminated)??;
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_remux_config_default() {
        let config = RemuxConfig::default();
        assert_eq!(config.input_format, "auto");
        assert_eq!(config.chunk_size, 256 * 1024);
        assert!(config.use_fragmented_mp4);
    }

    #[test]
    fn test_remux_config_avi() {
        let config = RemuxConfig::for_avi();
        assert_eq!(config.input_format, "avi");
        assert_eq!(config.analyze_duration, 100_000_000);
        assert_eq!(config.probe_size, 100_000_000);
    }

    #[test]
    fn test_remux_config_low_latency() {
        let config = RemuxConfig::low_latency();
        assert_eq!(config.chunk_size, 128 * 1024);
        assert_eq!(config.buffer_capacity, 64);
    }

    #[test]
    fn test_ffmpeg_args_include_copy_codecs() {
        let config = RemuxConfig::default();
        let args = config.build_ffmpeg_args();

        // Should use copy codecs for remuxing
        assert!(args.contains(&"-c:v".to_string()));
        assert!(args.contains(&"copy".to_string()));
        assert!(args.contains(&"-c:a".to_string()));

        // Should use fragmented MP4
        assert!(args.contains(&"frag_keyframe+empty_moov+default_base_moof".to_string()));

        // Should output to pipe
        assert!(args.contains(&"pipe:1".to_string()));
    }

    #[test]
    fn test_remux_config_validation() {
        let mut config = RemuxConfig::default();

        // Valid config should work
        assert!(RealtimeRemuxer::new(config.clone()).is_ok());

        // Invalid chunk size should fail
        config.chunk_size = 0;
        assert!(RealtimeRemuxer::new(config.clone()).is_err());

        // Invalid buffer capacity should fail
        config.chunk_size = 1024;
        config.buffer_capacity = 0;
        assert!(RealtimeRemuxer::new(config).is_err());
    }

    #[test]
    fn test_remuxed_chunk_properties() {
        let chunk = RemuxedChunk {
            offset: 0,
            data: vec![0, 0, 0, 32], // Mock ftyp box
            is_header: true,
            timestamp: Instant::now(),
        };

        assert_eq!(chunk.offset, 0);
        assert!(chunk.is_header);
        assert_eq!(chunk.data.len(), 4);
    }

    #[test]
    fn test_remux_status_transitions() {
        assert_eq!(RemuxStatus::Initializing, RemuxStatus::Initializing);
        assert_ne!(RemuxStatus::Running, RemuxStatus::Completed);

        // Test that we can match on status
        let status = RemuxStatus::Running;
        assert!(matches!(status, RemuxStatus::Running));
    }
}
