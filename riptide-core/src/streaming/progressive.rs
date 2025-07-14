//! Progressive streaming support for feeding data to FFmpeg as it becomes available
//!
//! This module provides a simple, robust approach to progressive streaming that prioritizes
//! correctness, testability, and debuggability over complex async coordination.

use std::io::{self, Write};
use std::path::PathBuf;
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;
use std::time::Duration;

use tokio::task::JoinHandle;
use tracing::{debug, error, info, warn};

use crate::engine::TorrentEngineHandle;
use crate::storage::data_source::{DataError, DataSource};
use crate::torrent::InfoHash;

/// Minimum data needed before starting FFmpeg (20MB for AVI headers)
const MIN_HEADER_SIZE: u64 = 20 * 1024 * 1024;

/// Size of chunks to read from the data source
const CHUNK_SIZE: u64 = 1024 * 1024; // 1MB

/// How long to wait between read attempts when data isn't available
const RETRY_DELAY: Duration = Duration::from_millis(100);

/// Errors that can occur during streaming
#[derive(Debug, thiserror::Error)]
pub enum ProgressiveStreamError {
    /// FFmpeg process failed to start
    #[error("Failed to start FFmpeg: {0}")]
    FfmpegStart(io::Error),

    /// FFmpeg process failed during execution
    #[error("FFmpeg failed with status: {0}")]
    FfmpegFailed(ExitStatus),

    /// Failed to write to FFmpeg stdin
    #[error("Failed to write to FFmpeg: {0}")]
    WriteFailed(io::Error),

    /// Failed to read from data source
    #[error("Failed to read data: {0}")]
    ReadFailed(String),

    /// No stdin handle available
    #[error("FFmpeg stdin not available")]
    NoStdin,

    /// Timeout waiting for initial data
    #[error("Timeout waiting for initial data")]
    InitialDataTimeout,
}

/// Core streaming pump that synchronously reads from source and writes to sink.
///
/// This is the heart of the progressive streaming system. It's intentionally
/// synchronous to avoid complex async coordination issues.
pub struct StreamPump {
    source: Arc<dyn DataSource>,
    info_hash: InfoHash,
    file_size: u64,
    bytes_pumped: AtomicU64,
    torrent_engine: Option<TorrentEngineHandle>,
    runtime_handle: tokio::runtime::Handle,
}

impl Clone for StreamPump {
    fn clone(&self) -> Self {
        Self {
            source: Arc::clone(&self.source),
            info_hash: self.info_hash,
            file_size: self.file_size,
            bytes_pumped: AtomicU64::new(self.bytes_pumped.load(Ordering::Relaxed)),
            torrent_engine: self.torrent_engine.clone(),
            runtime_handle: self.runtime_handle.clone(),
        }
    }
}

impl StreamPump {
    /// Creates a new stream pump for the given data source.
    pub fn new(
        source: Arc<dyn DataSource>,
        info_hash: InfoHash,
        file_size: u64,
        torrent_engine: Option<TorrentEngineHandle>,
        runtime_handle: tokio::runtime::Handle,
    ) -> Self {
        Self {
            source,
            info_hash,
            file_size,
            bytes_pumped: AtomicU64::new(0),
            torrent_engine,
            runtime_handle,
        }
    }

    /// Synchronously pump data from source to writer.
    ///
    /// This is the ENTIRE progressive streaming logic. It reads chunks from the
    /// data source and writes them to the provided writer (FFmpeg's stdin).
    ///
    /// # Errors
    ///
    /// - `ProgressiveStreamError::WriteFailed` - If writing to output fails
    /// - `ProgressiveStreamError::InitialDataTimeout` - If initial data times out
    pub fn pump_to<W: Write>(&self, mut writer: W) -> Result<u64, ProgressiveStreamError> {
        let mut offset = 0u64;

        // First, wait for minimum header data
        info!(
            "Waiting for initial {} bytes for {}",
            MIN_HEADER_SIZE, self.info_hash
        );

        let wait_start = std::time::Instant::now();
        while offset < MIN_HEADER_SIZE.min(self.file_size) {
            if wait_start.elapsed() > Duration::from_secs(60) {
                return Err(ProgressiveStreamError::InitialDataTimeout);
            }

            match self.read_chunk(offset, (offset + CHUNK_SIZE).min(self.file_size)) {
                Ok(data) if !data.is_empty() => {
                    writer
                        .write_all(&data)
                        .map_err(ProgressiveStreamError::WriteFailed)?;
                    offset += data.len() as u64;
                    self.bytes_pumped.store(offset, Ordering::Relaxed);

                    // Update torrent engine streaming position
                    if let Some(engine) = &self.torrent_engine {
                        // TODO: Add streaming position update when API is available
                        let _ = engine;
                    }

                    debug!(
                        "Initial data progress: {}/{} bytes",
                        offset,
                        MIN_HEADER_SIZE.min(self.file_size)
                    );
                }
                _ => {
                    thread::sleep(RETRY_DELAY);
                }
            }
        }

        info!(
            "Initial data ready for {}, continuing with streaming",
            self.info_hash
        );

        // Now stream the rest of the file
        let mut consecutive_failures = 0;
        const MAX_CONSECUTIVE_FAILURES: u32 = 180; // Allow 3 minutes of consecutive failures (180 * 1s)
        let mut last_progress_time = std::time::Instant::now();
        const PROGRESS_TIMEOUT: Duration = Duration::from_secs(300); // 5 minutes without any progress

        while offset < self.file_size {
            let chunk_end = (offset + CHUNK_SIZE).min(self.file_size);

            match self.read_chunk(offset, chunk_end) {
                Ok(data) if !data.is_empty() => {
                    writer
                        .write_all(&data)
                        .map_err(ProgressiveStreamError::WriteFailed)?;
                    offset += data.len() as u64;
                    self.bytes_pumped.store(offset, Ordering::Relaxed);

                    // Reset failure tracking on successful read
                    consecutive_failures = 0;
                    last_progress_time = std::time::Instant::now();

                    // Update torrent engine streaming position
                    if let Some(engine) = &self.torrent_engine {
                        // TODO: Add streaming position update when API is available
                        let _ = engine;
                    }

                    // Log progress every 1MB to track streaming more closely
                    if offset % (1024 * 1024) == 0 || offset > self.file_size - (10 * 1024 * 1024) {
                        info!(
                            "Streaming progress: {}/{} bytes ({:.1}%) - continuing...",
                            offset,
                            self.file_size,
                            (offset as f64 / self.file_size as f64) * 100.0
                        );
                    }
                }
                Ok(_) => {
                    // Empty data means it's not available yet - wait longer for torrent pieces
                    consecutive_failures += 1;
                    info!(
                        "No data available at offset {} ({:.1}%), waiting for download... (attempt {})",
                        offset,
                        (offset as f64 / self.file_size as f64) * 100.0,
                        consecutive_failures
                    );
                    thread::sleep(Duration::from_millis(500)); // Increased from 100ms to 500ms
                }
                Err(e) => {
                    consecutive_failures += 1;
                    // Differentiate between different error types
                    match e {
                        DataError::InsufficientData { missing_count, .. } => {
                            warn!(
                                "STREAMING BLOCKED: Waiting for {} missing pieces at offset {} ({:.1}%) (attempt {})",
                                missing_count,
                                offset,
                                (offset as f64 / self.file_size as f64) * 100.0,
                                consecutive_failures
                            );
                            thread::sleep(Duration::from_secs(1)); // Wait longer for missing pieces
                        }
                        DataError::Storage { reason } if reason.contains("timeout") => {
                            warn!(
                                "STREAMING TIMEOUT: Read timeout at offset {} ({:.1}%), continuing to wait for data... (attempt {})",
                                offset,
                                (offset as f64 / self.file_size as f64) * 100.0,
                                consecutive_failures
                            );
                            thread::sleep(Duration::from_secs(2)); // Longer wait for timeouts
                        }
                        _ => {
                            error!(
                                "STREAMING ERROR: Read error at offset {} ({:.1}%): {} (attempt {})",
                                offset,
                                (offset as f64 / self.file_size as f64) * 100.0,
                                e,
                                consecutive_failures
                            );
                            thread::sleep(RETRY_DELAY);
                        }
                    }
                }
            }

            // Check for streaming stall conditions
            if consecutive_failures >= MAX_CONSECUTIVE_FAILURES {
                error!(
                    "STREAMING STALLED: {} consecutive failures at offset {} ({:.1}%) - stopping progressive streaming",
                    consecutive_failures,
                    offset,
                    (offset as f64 / self.file_size as f64) * 100.0
                );
                break;
            }

            if last_progress_time.elapsed() > PROGRESS_TIMEOUT {
                error!(
                    "STREAMING TIMEOUT: No progress for {} seconds at offset {} ({:.1}%) - stopping progressive streaming",
                    PROGRESS_TIMEOUT.as_secs(),
                    offset,
                    (offset as f64 / self.file_size as f64) * 100.0
                );
                break;
            }
        }

        if offset >= self.file_size {
            info!(
                "Successfully completed streaming {} bytes for {} (100%)",
                offset, self.info_hash
            );
        } else {
            warn!(
                "Progressive streaming stopped early: {} bytes / {} bytes ({:.1}%) for {}",
                offset,
                self.file_size,
                (offset as f64 / self.file_size as f64) * 100.0,
                self.info_hash
            );
        }
        Ok(offset)
    }

    /// Read a chunk of data, blocking until available or timeout.
    fn read_chunk(&self, start: u64, end: u64) -> Result<Vec<u8>, DataError> {
        // Use block_on to call async code from sync context
        self.runtime_handle.block_on(async {
            tokio::time::timeout(
                Duration::from_secs(30), // Increased from 5s to 30s for torrent piece availability
                self.source.read_range(self.info_hash, start..end),
            )
            .await
            .map_err(|_| DataError::Storage {
                reason: "Read timeout after 30 seconds".to_string(),
            })?
        })
    }

    /// Get the number of bytes pumped so far.
    pub fn bytes_pumped(&self) -> u64 {
        self.bytes_pumped.load(Ordering::Relaxed)
    }
}

/// Simple FFmpeg process management.
pub struct FfmpegRunner {
    input_format: String,
    output_path: PathBuf,
}

impl FfmpegRunner {
    /// Creates a new FFmpeg runner.
    pub fn new(input_format: String, output_path: PathBuf) -> Self {
        Self {
            input_format,
            output_path,
        }
    }

    /// Run FFmpeg with progressive input.
    ///
    /// Returns a handle to write input data.
    ///
    /// # Errors
    ///
    /// - `ProgressiveStreamError::FfmpegStart` - If FFmpeg fails to start
    pub fn run_progressive(&self) -> Result<FfmpegHandle, ProgressiveStreamError> {
        let mut cmd = Command::new("ffmpeg");

        // Global options
        cmd.arg("-y"); // Overwrite output

        // Input options
        cmd.args([
            "-analyzeduration",
            "10000000", // 10 seconds
            "-probesize",
            "20971520", // 20MB
            "-err_detect",
            "ignore_err",
            "-fflags",
            "+igndts+ignidx+genpts+discardcorrupt",
            "-avoid_negative_ts",
            "make_zero",
            "-thread_queue_size",
            "2048",
        ]);

        // Only specify format if it's not "auto" - let FFmpeg auto-detect otherwise
        if self.input_format != "auto" {
            cmd.args(["-f", &self.input_format]);
        }

        cmd.args(["-i", "pipe:0"]);

        // Output options - simple and reliable
        cmd.args([
            "-c:v",
            "libx264",
            "-preset",
            "ultrafast",
            "-crf",
            "23",
            "-profile:v",
            "high",
            "-level",
            "4.1",
            "-pix_fmt",
            "yuv420p",
            "-c:a",
            "aac",
            "-b:a",
            "192k",
            "-movflags",
            "frag_keyframe+empty_moov+default_base_moof",
            "-max_muxing_queue_size",
            "4096",
            "-f",
            "mp4",
        ]);

        cmd.arg(&self.output_path);

        // Configure process
        cmd.stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::piped());

        info!("Starting FFmpeg: {:?}", cmd);

        let mut child = cmd.spawn().map_err(ProgressiveStreamError::FfmpegStart)?;

        // Capture stderr for debugging in a separate thread
        if let Some(stderr) = child.stderr.take() {
            thread::spawn(move || {
                use std::io::{BufRead, BufReader};
                let reader = BufReader::new(stderr);
                for line in reader.lines().map_while(Result::ok) {
                    debug!("FFmpeg: {}", line);
                }
            });
        }

        let stdin = child.stdin.take().ok_or(ProgressiveStreamError::NoStdin)?;

        Ok(FfmpegHandle { child, stdin })
    }
}

/// Handle to a running FFmpeg process.
pub struct FfmpegHandle {
    child: Child,
    stdin: std::process::ChildStdin,
}

impl FfmpegHandle {
    /// Get the stdin to write data.
    pub fn into_stdin(self) -> std::process::ChildStdin {
        self.stdin
    }

    /// Wait for FFmpeg process to complete.
    ///
    /// # Errors
    ///
    /// - `io::Error` - If waiting for the process fails
    pub fn wait(mut self) -> Result<ExitStatus, io::Error> {
        drop(self.stdin); // Close stdin first
        self.child.wait()
    }

    /// Split into stdin and waiter.
    pub fn split(self) -> (std::process::ChildStdin, FfmpegWaiter) {
        (self.stdin, FfmpegWaiter { child: self.child })
    }
}

/// Waiter for FFmpeg process completion.
pub struct FfmpegWaiter {
    child: Child,
}

impl FfmpegWaiter {
    /// Wait for FFmpeg process to complete.
    ///
    /// # Errors
    ///
    /// - `io::Error` - If waiting for the process fails
    pub fn wait(mut self) -> Result<ExitStatus, io::Error> {
        self.child.wait()
    }
}

/// Progressive streaming coordinator that manages the entire streaming pipeline.
pub struct ProgressiveStreamer {
    pump: StreamPump,
    runner: FfmpegRunner,
}

impl ProgressiveStreamer {
    /// Creates a new progressive streamer.
    pub fn new(
        source: Arc<dyn DataSource>,
        info_hash: InfoHash,
        file_size: u64,
        input_format: String,
        output_path: PathBuf,
        torrent_engine: Option<TorrentEngineHandle>,
    ) -> Self {
        let runtime_handle = tokio::runtime::Handle::current();
        let pump = StreamPump::new(source, info_hash, file_size, torrent_engine, runtime_handle);
        let runner = FfmpegRunner::new(input_format, output_path.clone());

        Self { pump, runner }
    }

    /// Start progressive streaming in a background thread.
    ///
    /// Returns a join handle that resolves when streaming completes.
    pub fn start(self) -> JoinHandle<Result<(), ProgressiveStreamError>> {
        tokio::task::spawn_blocking(move || {
            // Start FFmpeg
            let handle = self.runner.run_progressive()?;
            let (stdin, waiter) = handle.split();

            // Create a thread for pumping data
            let pump = self.pump.clone();
            let pump_handle = thread::spawn(move || pump.pump_to(stdin));

            // Wait for pumping to complete
            match pump_handle.join() {
                Ok(Ok(bytes)) => {
                    info!("Pumped {} bytes successfully", bytes);
                }
                Ok(Err(e)) => {
                    error!("Pump failed: {}", e);
                    return Err(e);
                }
                Err(_) => {
                    error!("Pump thread panicked");
                    return Err(ProgressiveStreamError::WriteFailed(io::Error::other(
                        "Pump thread panicked",
                    )));
                }
            }

            // Wait for FFmpeg to finish
            let status = waiter.wait().map_err(ProgressiveStreamError::FfmpegStart)?;

            if !status.success() {
                return Err(ProgressiveStreamError::FfmpegFailed(status));
            }

            Ok(())
        })
    }

    /// Check if output is ready for streaming.
    pub fn is_ready(&self) -> bool {
        // Check if we have enough MP4 header data (ftyp + moov atoms)
        if let Ok(metadata) = std::fs::metadata(&self.runner.output_path) {
            let size = metadata.len();
            // Need at least 128KB for a valid streamable MP4 header
            size >= 128 * 1024
        } else {
            false
        }
    }

    /// Get progress information.
    pub fn progress(&self) -> ProgressInfo {
        ProgressInfo {
            bytes_pumped: self.pump.bytes_pumped(),
            file_size: self.pump.file_size,
            output_path: self.runner.output_path.clone(),
        }
    }
}

/// Progress information for streaming.
#[derive(Debug, Clone)]
pub struct ProgressInfo {
    /// Bytes pumped to FFmpeg so far
    pub bytes_pumped: u64,
    /// Total file size
    pub file_size: u64,
    /// Output file path
    pub output_path: PathBuf,
}

#[cfg(test)]
mod tests {

    use super::*;

    /// Mock data source for testing
    struct MockDataSource {
        data: Vec<u8>,
        delays: tokio::sync::Mutex<Vec<(std::ops::Range<u64>, Duration)>>,
        read_count: tokio::sync::Mutex<usize>,
    }

    impl MockDataSource {
        fn new(size: u64) -> Self {
            Self {
                data: vec![0xFF; size as usize],
                delays: tokio::sync::Mutex::new(Vec::new()),
                read_count: tokio::sync::Mutex::new(0),
            }
        }

        fn with_delay(self, range: std::ops::Range<u64>, delay: Duration) -> Self {
            self.delays.blocking_lock().push((range, delay));
            self
        }
    }

    #[async_trait::async_trait]
    impl DataSource for MockDataSource {
        async fn file_size(&self, _info_hash: InfoHash) -> Result<u64, DataError> {
            Ok(self.data.len() as u64)
        }

        fn source_type(&self) -> &'static str {
            "mock"
        }

        async fn can_handle(&self, _info_hash: InfoHash) -> bool {
            true
        }
        async fn read_range(
            &self,
            _info_hash: InfoHash,
            range: std::ops::Range<u64>,
        ) -> Result<Vec<u8>, DataError> {
            *self.read_count.lock().await += 1;

            // Check if this range has a delay
            let delay_duration = {
                let delays = self.delays.lock().await;
                delays
                    .iter()
                    .find(|(delay_range, _)| {
                        delay_range.start <= range.start && range.end <= delay_range.end
                    })
                    .map(|(_, duration)| *duration)
            };

            if let Some(duration) = delay_duration {
                tokio::time::sleep(duration).await;
            }

            Ok(self.data[range.start as usize..range.end as usize].to_vec())
        }

        async fn check_range_availability(
            &self,
            _info_hash: InfoHash,
            range: std::ops::Range<u64>,
        ) -> Result<crate::storage::data_source::RangeAvailability, DataError> {
            Ok(crate::storage::data_source::RangeAvailability {
                available: range.end <= self.data.len() as u64,
                missing_pieces: vec![],
                cache_hit: false,
            })
        }
    }

    #[tokio::test]
    async fn test_stream_pump_basic() {
        let source = Arc::new(MockDataSource::new(1024 * 1024));
        let pump = StreamPump::new(
            source.clone(),
            InfoHash::new(*b"test_hash_simple_001"),
            1024 * 1024,
            None,
            tokio::runtime::Handle::current(),
        );

        let (result, output) = tokio::task::spawn_blocking(move || {
            let mut output = Vec::new();
            let result = pump.pump_to(&mut output);
            (result, output)
        })
        .await
        .unwrap();

        assert!(result.is_ok());
        assert_eq!(result.unwrap(), 1024 * 1024);
        assert_eq!(output.len(), 1024 * 1024);
    }

    #[tokio::test]
    #[ignore = "Temporarily disabled due to runtime conflicts - will be replaced in refactor"]
    async fn test_stream_pump_with_delays() {
        let source = Arc::new(
            MockDataSource::new(2 * 1024 * 1024)
                .with_delay(0..1024 * 1024, Duration::from_millis(50)),
        );
        let pump = StreamPump::new(
            source.clone(),
            InfoHash::new(*b"test_hash_simple_002"),
            2 * 1024 * 1024,
            None,
            tokio::runtime::Handle::current(),
        );

        let start = std::time::Instant::now();
        let (result, _output) = tokio::task::spawn_blocking(move || {
            let mut output = Vec::new();
            let result = pump.pump_to(&mut output);
            (result, output)
        })
        .await
        .unwrap();
        let elapsed = start.elapsed();

        assert!(result.is_ok());
        assert_eq!(result.unwrap(), 2 * 1024 * 1024);
        assert!(elapsed >= Duration::from_millis(50));
    }
}
