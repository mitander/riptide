//! FFmpeg abstraction for both production and simulation modes

use std::path::Path;
use std::time::Instant;

use async_trait::async_trait;

use super::strategy::{StreamingError, StreamingResult};

/// Abstraction for FFmpeg operations to enable both real and simulated remuxing
#[async_trait]
pub trait FfmpegProcessor: Send + Sync {
    /// Remux input file to MP4 format with optimal streaming settings
    ///
    /// # Errors
    /// - `StreamingError::FfmpegError` - FFmpeg process failed
    /// - `StreamingError::IoError` - File I/O error
    async fn remux_to_mp4(
        &self,
        input_path: &Path,
        output_path: &Path,
        config: &RemuxingOptions,
    ) -> StreamingResult<RemuxingResult>;

    /// Check if FFmpeg is available and properly configured
    fn is_available(&self) -> bool;

    /// Get estimated output file size for a given input
    ///
    /// This is used for Content-Length headers before remuxing is complete.
    /// Returns None if estimation is not possible.
    async fn estimate_output_size(&self, input_path: &Path) -> Option<u64>;
}

/// Configuration options for remuxing operations
#[derive(Debug, Clone)]
pub struct RemuxingOptions {
    /// Video codec: "copy" to avoid re-encoding, or specific codec like "h264"
    pub video_codec: String,

    /// Audio codec: "copy" to avoid re-encoding, or specific codec like "aac"
    pub audio_codec: String,

    /// Optimize for streaming (move moov atom to beginning)
    pub faststart: bool,

    /// Maximum allowed processing time (None = no limit)
    pub timeout_seconds: Option<u64>,
}

impl Default for RemuxingOptions {
    fn default() -> Self {
        Self {
            video_codec: "copy".to_string(), // Don't re-encode video
            audio_codec: "copy".to_string(), // Don't re-encode audio
            faststart: true,                 // Optimize for streaming
            timeout_seconds: Some(300),      // 5 minute timeout
        }
    }
}

/// Result of a remuxing operation
#[derive(Debug)]
pub struct RemuxingResult {
    /// Size of output file in bytes
    pub output_size: u64,

    /// Processing time in seconds
    pub processing_time: f64,

    /// Whether any streams were re-encoded (vs copied)
    pub streams_reencoded: bool,
}

/// Production FFmpeg implementation using ffmpeg-next crate
pub struct ProductionFfmpegProcessor {
    #[allow(dead_code)]
    ffmpeg_path: Option<std::path::PathBuf>,
}

impl ProductionFfmpegProcessor {
    /// Create new FFmpeg processor with optional custom binary path
    pub fn new(ffmpeg_path: Option<std::path::PathBuf>) -> Self {
        Self { ffmpeg_path }
    }

    /// Verify FFmpeg installation and codecs
    fn verify_installation(&self) -> StreamingResult<()> {
        // Check if ffmpeg binary is available by running version command
        let result = std::process::Command::new("ffmpeg")
            .arg("-version")
            .output();

        match result {
            Ok(output) if output.status.success() => Ok(()),
            Ok(_) => Err(StreamingError::FfmpegError {
                reason: "FFmpeg binary found but returned error".to_string(),
            }),
            Err(_) => Err(StreamingError::FfmpegError {
                reason: "FFmpeg binary not found in PATH".to_string(),
            }),
        }
    }
}

#[async_trait]
impl FfmpegProcessor for ProductionFfmpegProcessor {
    async fn remux_to_mp4(
        &self,
        input_path: &Path,
        output_path: &Path,
        config: &RemuxingOptions,
    ) -> StreamingResult<RemuxingResult> {
        let start_time = Instant::now();

        // Build FFmpeg command for container remuxing
        let mut cmd = tokio::process::Command::new("ffmpeg");
        cmd.arg("-y") // Overwrite output file
            .arg("-i")
            .arg(input_path)
            .arg("-c:v")
            .arg(&config.video_codec)
            .arg("-c:a")
            .arg(&config.audio_codec);

        // Add streaming optimization
        if config.faststart {
            cmd.arg("-movflags").arg("faststart");
        }

        cmd.arg(output_path);

        // Execute FFmpeg command
        tracing::debug!("Executing FFmpeg command: {:?}", cmd);

        let output = cmd
            .output()
            .await
            .map_err(|e| StreamingError::FfmpegError {
                reason: format!("Failed to execute ffmpeg: {e}"),
            })?;

        // Log FFmpeg output for debugging
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);

        if !stdout.is_empty() {
            tracing::debug!("FFmpeg stdout: {}", stdout);
        }
        if !stderr.is_empty() {
            tracing::debug!("FFmpeg stderr: {}", stderr);
        }

        if !output.status.success() {
            return Err(StreamingError::FfmpegError {
                reason: format!("FFmpeg failed with exit code {}: {stderr}", output.status),
            });
        }

        // Get output file size
        let output_size = std::fs::metadata(output_path)
            .map_err(|e| StreamingError::IoError {
                operation: "get output file size".to_string(),
                path: output_path.to_string_lossy().to_string(),
                source: e,
            })?
            .len();

        let processing_time = start_time.elapsed().as_secs_f64();

        Ok(RemuxingResult {
            output_size,
            processing_time,
            streams_reencoded: config.video_codec != "copy" || config.audio_codec != "copy",
        })
    }

    fn is_available(&self) -> bool {
        self.verify_installation().is_ok()
    }

    async fn estimate_output_size(&self, input_path: &Path) -> Option<u64> {
        // For copy operations, output size is usually similar to input size
        std::fs::metadata(input_path).ok().map(|m| m.len())
    }
}

/// Simulation FFmpeg implementation for testing
///
/// Uses real FFmpeg with accelerated processing for consistent output quality.
/// Simulates faster processing speeds while maintaining compatibility with browsers.
pub struct SimulationFfmpegProcessor {
    /// Base FFmpeg processor for real conversion
    base_processor: ProductionFfmpegProcessor,

    /// Simulated processing time per megabyte (for timing simulation)
    processing_speed_mb_per_sec: f64,

    /// Whether to simulate FFmpeg as available
    is_available: bool,
}

impl SimulationFfmpegProcessor {
    /// Create new simulation processor
    pub fn new() -> Self {
        Self {
            base_processor: ProductionFfmpegProcessor::new(None),
            processing_speed_mb_per_sec: 100.0, // Simulate 100 MB/s (faster than real)
            is_available: true,
        }
    }

    /// Configure simulation parameters
    pub fn with_speed(mut self, mb_per_sec: f64) -> Self {
        self.processing_speed_mb_per_sec = mb_per_sec;
        self
    }

    /// Simulate FFmpeg being unavailable
    pub fn unavailable(mut self) -> Self {
        self.is_available = false;
        self
    }
}

impl Default for SimulationFfmpegProcessor {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl FfmpegProcessor for SimulationFfmpegProcessor {
    async fn remux_to_mp4(
        &self,
        input_path: &Path,
        output_path: &Path,
        config: &RemuxingOptions,
    ) -> StreamingResult<RemuxingResult> {
        if !self.is_available {
            return Err(StreamingError::FfmpegError {
                reason: "FFmpeg not available in simulation".to_string(),
            });
        }

        // Use real FFmpeg conversion through base processor
        let start_time = Instant::now();
        let result = self
            .base_processor
            .remux_to_mp4(input_path, output_path, config)
            .await?;
        let actual_processing_time = start_time.elapsed().as_secs_f64();

        // Calculate simulated processing time based on file size
        let input_size = std::fs::metadata(input_path)
            .map_err(|e| StreamingError::IoError {
                operation: "get input file size".to_string(),
                path: input_path.to_string_lossy().to_string(),
                source: e,
            })?
            .len();

        let input_mb = input_size as f64 / (1024.0 * 1024.0);
        let simulated_processing_time = input_mb / self.processing_speed_mb_per_sec;

        // If actual processing was faster than simulation target, add delay
        if actual_processing_time < simulated_processing_time {
            let delay = simulated_processing_time - actual_processing_time;
            tokio::time::sleep(tokio::time::Duration::from_secs_f64(delay)).await;
        }

        // Return result with simulated timing
        Ok(RemuxingResult {
            output_size: result.output_size,
            processing_time: simulated_processing_time.max(actual_processing_time),
            streams_reencoded: result.streams_reencoded,
        })
    }

    fn is_available(&self) -> bool {
        self.is_available && self.base_processor.is_available()
    }

    async fn estimate_output_size(&self, input_path: &Path) -> Option<u64> {
        self.base_processor.estimate_output_size(input_path).await
    }
}

#[cfg(test)]
mod tests {
    use tempfile::tempdir;

    use super::*;

    #[tokio::test]
    async fn test_simulation_ffmpeg_processor() {
        let temp_dir = tempdir().unwrap();
        let input_path = temp_dir.path().join("input.mkv");
        let output_path = temp_dir.path().join("output.mp4");

        // Create dummy input file (invalid format will cause FFmpeg to fail)
        std::fs::write(&input_path, vec![0u8; 1024 * 1024]).unwrap(); // 1MB file

        let processor = SimulationFfmpegProcessor::new().with_speed(10.0); // 10 MB/s for fast test

        let options = RemuxingOptions::default();
        let result = processor
            .remux_to_mp4(&input_path, &output_path, &options)
            .await;

        // Since we're providing invalid input data, FFmpeg should fail
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            StreamingError::FfmpegError { .. }
        ));
    }

    #[tokio::test]
    async fn test_simulation_ffmpeg_unavailable() {
        let temp_dir = tempdir().unwrap();
        let input_path = temp_dir.path().join("input.mkv");
        let output_path = temp_dir.path().join("output.mp4");

        std::fs::write(&input_path, b"dummy content").unwrap();

        let processor = SimulationFfmpegProcessor::new().unavailable();
        let options = RemuxingOptions::default();

        let result = processor
            .remux_to_mp4(&input_path, &output_path, &options)
            .await;
        assert!(matches!(result, Err(StreamingError::FfmpegError { .. })));
    }

    #[test]
    fn test_remuxing_options_defaults() {
        let options = RemuxingOptions::default();
        assert_eq!(options.video_codec, "copy");
        assert_eq!(options.audio_codec, "copy");
        assert!(options.faststart);
        assert_eq!(options.timeout_seconds, Some(300));
    }
}
