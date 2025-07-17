//! Real-time remuxing implementation for incompatible media formats.
//!
//! This module provides streaming remuxing for media files that require
//! container format conversion (MKV→MP4, AVI→MP4). It uses FFmpeg in
//! copy mode to avoid re-encoding, achieving fast conversion with
//! minimal CPU usage.

use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;

use axum::body::Body;
use axum::http::{HeaderValue, Request, StatusCode, header};
use axum::response::{IntoResponse, Response};
use futures::stream;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::process::Command;
use tracing::{debug, error, info, warn};

use crate::streaming::traits::{PieceProvider, PieceProviderError, StreamProducer};

/// Size of chunks to read from the piece provider and feed to FFmpeg.
const CHUNK_SIZE: usize = 256 * 1024; // 256KB

/// Retry delay when pieces are not yet available.
const RETRY_DELAY: Duration = Duration::from_millis(100);

/// Real-time remuxing producer for non-browser-compatible formats.
///
/// Converts media containers on-the-fly using FFmpeg in copy mode,
/// enabling streaming of MKV, AVI, and other formats as fragmented MP4.
/// The remuxing happens in real-time as data flows through the pipeline.
pub struct RemuxStreamProducer {
    /// The underlying piece provider for reading torrent data.
    provider: Arc<dyn PieceProvider>,
    /// Source container format (e.g., "mkv", "avi").
    source_format: String,
}

impl RemuxStreamProducer {
    /// Creates a new remuxing stream producer.
    ///
    /// The source format is used for logging and potential format-specific
    /// optimizations but doesn't affect the core remuxing logic.
    pub fn new(provider: Arc<dyn PieceProvider>, source_format: String) -> Self {
        Self {
            provider,
            source_format,
        }
    }

    /// Builds the FFmpeg command arguments for remuxing.
    ///
    /// Uses copy mode to avoid re-encoding and fragmented MP4 output
    /// for immediate streaming capability.
    fn build_ffmpeg_args() -> Vec<String> {
        vec![
            "-hide_banner".to_string(),
            "-loglevel".to_string(),
            "error".to_string(),
            "-i".to_string(),
            "pipe:0".to_string(),
            "-c:v".to_string(),
            "copy".to_string(),
            "-c:a".to_string(),
            "copy".to_string(),
            "-movflags".to_string(),
            "frag_keyframe+empty_moov+default_base_moof".to_string(),
            "-f".to_string(),
            "mp4".to_string(),
            "pipe:1".to_string(),
        ]
    }

    /// Spawns the FFmpeg process with proper pipe configuration.
    ///
    /// Returns the child process handle with stdin and stdout available
    /// for streaming data through.
    async fn spawn_ffmpeg() -> Result<tokio::process::Child, std::io::Error> {
        Command::new("ffmpeg")
            .args(Self::build_ffmpeg_args())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
    }

    /// Creates the input pump task that feeds data from the provider to FFmpeg.
    ///
    /// This task runs independently, reading chunks from the piece provider
    /// and writing them to FFmpeg's stdin. It handles piece availability
    /// gracefully by retrying when data isn't ready yet.
    fn spawn_input_pump(
        provider: Arc<dyn PieceProvider>,
        mut stdin: tokio::process::ChildStdin,
    ) -> tokio::task::JoinHandle<()> {
        tokio::spawn(async move {
            let mut offset = 0u64;
            let file_size = provider.size().await;

            debug!("Starting input pump for {} bytes", file_size);

            while offset < file_size {
                let chunk_size = std::cmp::min(CHUNK_SIZE as u64, file_size - offset) as usize;

                match provider.read_at(offset, chunk_size).await {
                    Ok(bytes) => {
                        match stdin.write_all(&bytes).await {
                            Ok(()) => {
                                offset += bytes.len() as u64;
                                if offset % (10 * 1024 * 1024) == 0 {
                                    debug!("Input pump progress: {}/{} bytes", offset, file_size);
                                }
                            }
                            Err(e) => {
                                // FFmpeg process likely exited
                                warn!("Failed to write to FFmpeg stdin: {}", e);
                                break;
                            }
                        }
                    }
                    Err(PieceProviderError::NotYetAvailable) => {
                        // Pieces not downloaded yet, wait and retry
                        tokio::time::sleep(RETRY_DELAY).await;
                    }
                    Err(e) => {
                        error!("Fatal error reading from piece provider: {}", e);
                        break;
                    }
                }
            }

            // Close stdin to signal EOF to FFmpeg
            drop(stdin);
            info!("Input pump completed, fed {} bytes to FFmpeg", offset);
        })
    }

    /// Spawns the stderr reader to log FFmpeg errors.
    ///
    /// This prevents the stderr buffer from filling up and blocking FFmpeg,
    /// while also providing visibility into any conversion issues.
    fn spawn_stderr_reader(mut stderr: tokio::process::ChildStderr) -> tokio::task::JoinHandle<()> {
        tokio::spawn(async move {
            use tokio::io::{AsyncBufReadExt, BufReader};

            let mut reader = BufReader::new(&mut stderr);
            let mut line = String::new();

            while reader.read_line(&mut line).await.unwrap_or(0) > 0 {
                if !line.trim().is_empty() {
                    warn!("FFmpeg stderr: {}", line.trim());
                }
                line.clear();
            }
        })
    }
}

#[async_trait::async_trait]
impl StreamProducer for RemuxStreamProducer {
    async fn produce_stream(&self, _request: Request<()>) -> Response {
        info!(
            "Starting real-time remux from {} to fragmented MP4",
            self.source_format
        );

        // Spawn FFmpeg process
        let mut child = match Self::spawn_ffmpeg().await {
            Ok(child) => child,
            Err(e) => {
                error!("Failed to spawn FFmpeg: {}", e);
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "Failed to start media conversion",
                )
                    .into_response();
            }
        };

        // Take ownership of the pipes
        let stdin = match child.stdin.take() {
            Some(stdin) => stdin,
            None => {
                error!("Failed to get FFmpeg stdin handle");
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "Failed to configure media conversion",
                )
                    .into_response();
            }
        };

        let stdout = match child.stdout.take() {
            Some(stdout) => stdout,
            None => {
                error!("Failed to get FFmpeg stdout handle");
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "Failed to configure media conversion",
                )
                    .into_response();
            }
        };

        let stderr = match child.stderr.take() {
            Some(stderr) => stderr,
            None => {
                error!("Failed to get FFmpeg stderr handle");
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "Failed to configure media conversion",
                )
                    .into_response();
            }
        };

        // Spawn input pump to feed data to FFmpeg
        let pump_handle = Self::spawn_input_pump(self.provider.clone(), stdin);

        // Spawn stderr reader to prevent blocking
        let stderr_handle = Self::spawn_stderr_reader(stderr);

        // Create stream from FFmpeg's stdout
        let stream = stream::unfold(stdout, |mut stdout| async move {
            let mut buffer = vec![0u8; 8192];
            match stdout.read(&mut buffer).await {
                Ok(0) => None, // EOF
                Ok(n) => {
                    buffer.truncate(n);
                    Some((Ok(bytes::Bytes::from(buffer)), stdout))
                }
                Err(e) => Some((Err(e), stdout)),
            }
        });
        let body = Body::from_stream(stream);

        // Spawn cleanup task
        tokio::spawn(async move {
            // Wait for FFmpeg to complete
            match child.wait().await {
                Ok(status) => {
                    if status.success() {
                        info!("FFmpeg remuxing completed successfully");
                    } else {
                        warn!("FFmpeg exited with status: {}", status);
                    }
                }
                Err(e) => {
                    error!("Failed to wait for FFmpeg: {}", e);
                }
            }

            // Ensure background tasks complete
            let _ = pump_handle.await;
            let _ = stderr_handle.await;
        });

        // Build response with appropriate headers for streaming
        Response::builder()
            .status(StatusCode::OK)
            .header(header::CONTENT_TYPE, "video/mp4")
            .header(header::CACHE_CONTROL, "no-cache")
            .header(
                header::ACCESS_CONTROL_ALLOW_ORIGIN,
                HeaderValue::from_static("*"),
            )
            .body(body)
            .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response())
    }
}

#[cfg(test)]
mod tests {
    use bytes::Bytes;

    use super::*;

    // Mock implementation for testing
    struct MockPieceProvider {
        data: bytes::Bytes,
    }

    impl MockPieceProvider {
        fn new(data: bytes::Bytes) -> Self {
            Self { data }
        }
    }

    #[async_trait::async_trait]
    impl PieceProvider for MockPieceProvider {
        async fn read_at(
            &self,
            offset: u64,
            length: usize,
        ) -> Result<bytes::Bytes, PieceProviderError> {
            let file_size = self.data.len() as u64;
            if offset + length as u64 > file_size {
                return Err(PieceProviderError::InvalidRange {
                    offset,
                    length,
                    file_size,
                });
            }

            let start = offset as usize;
            let end = start + length;
            Ok(self.data.slice(start..end))
        }

        async fn size(&self) -> u64 {
            self.data.len() as u64
        }
    }

    #[test]
    fn test_ffmpeg_args() {
        let args = RemuxStreamProducer::build_ffmpeg_args();

        // Verify critical arguments
        assert!(args.contains(&"-i".to_string()));
        assert!(args.contains(&"pipe:0".to_string()));
        assert!(args.contains(&"-c:v".to_string()));
        assert!(args.contains(&"copy".to_string()));
        assert!(args.contains(&"-c:a".to_string()));
        assert!(args.contains(&"copy".to_string()));
        assert!(args.contains(&"-movflags".to_string()));
        assert!(args.contains(&"frag_keyframe+empty_moov+default_base_moof".to_string()));
        assert!(args.contains(&"-f".to_string()));
        assert!(args.contains(&"mp4".to_string()));
        assert!(args.contains(&"pipe:1".to_string()));
    }

    #[tokio::test]
    async fn test_remux_stream_producer_creation() {
        let data = Bytes::from(vec![0u8; 1024]);
        let provider = Arc::new(MockPieceProvider::new(data));
        let producer = RemuxStreamProducer::new(provider, "mkv".to_string());

        assert_eq!(producer.source_format, "mkv");
    }

    // Note: Full integration tests with actual FFmpeg would go in
    // riptide-tests/integration/ to avoid requiring FFmpeg for unit tests
}
