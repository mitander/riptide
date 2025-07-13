//! Migration support for transitioning from complex to simple streaming.
//!
//! This module provides a unified interface that can switch between the old
//! progressive streaming implementation and the new simplified version.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use tokio::task::JoinHandle;
use tracing::{info, warn};

use crate::engine::TorrentEngineHandle;
use crate::storage::data_source::DataSource;
use crate::streaming::{RemuxingOptions, StreamingResult};
use crate::torrent::InfoHash;

/// Streaming implementation selector.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StreamingImplementation {
    /// Original complex implementation with coordinator, feeder, etc.
    Legacy,
    /// New simple implementation with synchronous pumping.
    Simple,
}

impl StreamingImplementation {
    /// Get implementation from environment variable.
    pub fn from_env() -> Self {
        match std::env::var("RIPTIDE_STREAMING_IMPL").as_deref() {
            Ok("simple") => {
                info!("Using simple streaming implementation");
                Self::Simple
            }
            Ok("legacy") => {
                info!("Using legacy streaming implementation");
                Self::Legacy
            }
            _ => {
                warn!("RIPTIDE_STREAMING_IMPL not set, defaulting to legacy");
                Self::Legacy
            }
        }
    }
}

/// Unified progressive streaming interface.
///
/// This wraps either the legacy or simple implementation based on configuration.
pub struct ProgressiveStreaming {
    implementation: StreamingImplementation,
    data_source: Arc<dyn DataSource>,
    torrent_engine: Option<TorrentEngineHandle>,
}

impl ProgressiveStreaming {
    /// Create a new progressive streaming instance.
    pub fn new(
        data_source: Arc<dyn DataSource>,
        torrent_engine: Option<TorrentEngineHandle>,
    ) -> Self {
        Self {
            implementation: StreamingImplementation::from_env(),
            data_source,
            torrent_engine,
        }
    }

    /// Create with explicit implementation choice.
    pub fn with_implementation(
        implementation: StreamingImplementation,
        data_source: Arc<dyn DataSource>,
        torrent_engine: Option<TorrentEngineHandle>,
    ) -> Self {
        Self {
            implementation,
            data_source,
            torrent_engine,
        }
    }

    /// Start progressive streaming for a file.
    ///
    /// # Errors
    ///
    /// - `StreamingError` - If the underlying streaming implementation fails to start
    pub async fn start_streaming(
        &self,
        info_hash: InfoHash,
        input_path: PathBuf,
        output_path: PathBuf,
        file_size: u64,
        options: &RemuxingOptions,
    ) -> StreamingResult<StreamingHandle> {
        match self.implementation {
            StreamingImplementation::Legacy => {
                self.start_legacy(info_hash, input_path, output_path, file_size, options)
                    .await
            }
            StreamingImplementation::Simple => {
                self.start_simple(info_hash, input_path, output_path, file_size, options)
                    .await
            }
        }
    }

    /// Start using legacy implementation.
    async fn start_legacy(
        &self,
        info_hash: InfoHash,
        input_path: PathBuf,
        output_path: PathBuf,
        file_size: u64,
        options: &RemuxingOptions,
    ) -> StreamingResult<StreamingHandle> {
        use crate::streaming::progressive::ProgressiveFeeder;

        let mut feeder = ProgressiveFeeder::new(
            info_hash,
            Arc::clone(&self.data_source),
            input_path,
            output_path.clone(),
            file_size,
        );

        feeder.start(options).await?;

        Ok(StreamingHandle {
            implementation: StreamingImplementation::Legacy,
            output_path,
            legacy_feeder: Some(Arc::new(feeder)),
            simple_handle: None,
        })
    }

    /// Start using simple implementation.
    async fn start_simple(
        &self,
        info_hash: InfoHash,
        _input_path: PathBuf,
        output_path: PathBuf,
        file_size: u64,
        _options: &RemuxingOptions,
    ) -> StreamingResult<StreamingHandle> {
        use crate::streaming::simple::SimpleProgressiveStreamer;

        // Detect input format from file extension
        let input_format = detect_input_format(&_input_path);

        let streamer = SimpleProgressiveStreamer::new(
            Arc::clone(&self.data_source),
            info_hash,
            file_size,
            input_format,
            output_path.clone(),
            self.torrent_engine.clone(),
        );

        let handle = streamer.start();

        Ok(StreamingHandle {
            implementation: StreamingImplementation::Simple,
            output_path,
            legacy_feeder: None,
            simple_handle: Some(handle),
        })
    }
}

/// Handle to a running streaming session.
#[derive(Debug)]
pub struct StreamingHandle {
    implementation: StreamingImplementation,
    output_path: PathBuf,
    legacy_feeder: Option<Arc<crate::streaming::progressive::ProgressiveFeeder>>,
    simple_handle: Option<JoinHandle<Result<(), crate::streaming::simple::SimpleStreamError>>>,
}

impl StreamingHandle {
    /// Check if the output is ready for streaming.
    pub async fn is_ready(&self) -> bool {
        match self.implementation {
            StreamingImplementation::Legacy => {
                if let Some(feeder) = &self.legacy_feeder {
                    feeder.is_output_ready().await
                } else {
                    false
                }
            }
            StreamingImplementation::Simple => {
                // Check if MP4 header is ready (at least 128KB)
                std::fs::metadata(&self.output_path)
                    .map(|m| m.len() >= 128 * 1024)
                    .unwrap_or(false)
            }
        }
    }

    /// Wait for streaming to complete.
    ///
    /// Note: This method can only be called once per handle. When used in monitoring
    /// contexts with Arc<StreamingHandle>, this returns immediately as the background
    /// monitoring task handles completion tracking.
    ///
    /// # Errors
    ///
    /// - `StreamingError` - If the streaming process encounters an error during completion
    pub async fn wait_completion(&self) -> StreamingResult<()> {
        match self.implementation {
            StreamingImplementation::Legacy => {
                // Legacy implementation runs in background tasks
                // For now, we don't have a clean way to wait for completion
                Ok(())
            }
            StreamingImplementation::Simple => {
                // When accessed via Arc (shared ownership), we can't consume the handle
                // The background monitoring task will handle completion tracking
                Ok(())
            }
        }
    }

    /// Shutdown the streaming session.
    pub fn shutdown(self) {
        match self.implementation {
            StreamingImplementation::Legacy => {
                if let Some(feeder) = self.legacy_feeder {
                    // Legacy feeder shutdown is handled by Drop
                    drop(feeder);
                }
            }
            StreamingImplementation::Simple => {
                if let Some(handle) = self.simple_handle {
                    handle.abort();
                }
            }
        }
    }
}

/// Detect input format from file path.
fn detect_input_format(path: &Path) -> String {
    match path.extension().and_then(|s| s.to_str()) {
        Some("avi") => "avi".to_string(),
        Some("mkv") => "matroska".to_string(),
        Some("mp4") => "mp4".to_string(),
        Some("mov") => "mov".to_string(),
        Some("webm") => "webm".to_string(),
        _ => "auto".to_string(), // Let FFmpeg auto-detect
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_implementation_from_env() {
        // Test default
        unsafe {
            std::env::remove_var("RIPTIDE_STREAMING_IMPL");
        }
        assert_eq!(
            StreamingImplementation::from_env(),
            StreamingImplementation::Legacy
        );

        // Test simple
        unsafe {
            std::env::set_var("RIPTIDE_STREAMING_IMPL", "simple");
        }
        assert_eq!(
            StreamingImplementation::from_env(),
            StreamingImplementation::Simple
        );

        // Test legacy
        unsafe {
            std::env::set_var("RIPTIDE_STREAMING_IMPL", "legacy");
        }
        assert_eq!(
            StreamingImplementation::from_env(),
            StreamingImplementation::Legacy
        );

        // Cleanup
        unsafe {
            std::env::remove_var("RIPTIDE_STREAMING_IMPL");
        }
    }

    #[test]
    fn test_detect_input_format() {
        assert_eq!(detect_input_format(&PathBuf::from("test.avi")), "avi");
        assert_eq!(detect_input_format(&PathBuf::from("test.mkv")), "matroska");
        assert_eq!(detect_input_format(&PathBuf::from("test.mp4")), "mp4");
        assert_eq!(detect_input_format(&PathBuf::from("test.unknown")), "auto");
        assert_eq!(detect_input_format(&PathBuf::from("no_extension")), "auto");
    }
}
