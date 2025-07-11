//! FFmpeg abstraction for both production and simulation modes

use std::path::Path;
use std::time::Instant;

use async_trait::async_trait;

use super::{StreamingError, StreamingResult};

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

    /// Ignore incomplete index data when remuxing
    pub ignore_index: bool,

    /// Allow processing of partial/incomplete files
    pub allow_partial: bool,
}

impl Default for RemuxingOptions {
    fn default() -> Self {
        Self {
            video_codec: "copy".to_string(), // Don't re-encode video
            audio_codec: "copy".to_string(), // Don't re-encode audio
            faststart: true,                 // Optimize for streaming
            timeout_seconds: Some(300),      // 5 minute timeout
            ignore_index: false,             // Default: require complete index
            allow_partial: false,            // Default: require complete file
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

        tracing::info!(
            "Starting FFmpeg remux: {} -> {}",
            input_path.display(),
            output_path.display()
        );
        tracing::debug!("Remux config: {:?}", config);

        // Build FFmpeg command for container remuxing
        let mut cmd = tokio::process::Command::new("ffmpeg");
        cmd.arg("-y") // Overwrite output file
            .arg("-i")
            .arg(input_path);

        // Add format-specific options for problematic containers
        let input_extension = input_path
            .extension()
            .and_then(|ext| ext.to_str())
            .unwrap_or("");

        match input_extension.to_lowercase().as_str() {
            "avi" => {
                tracing::info!("Applying AVI-specific FFmpeg options for streaming");
                // AVI files often have problematic timestamps and indexing
                cmd.arg("-fflags").arg("+genpts+igndts"); // Generate timestamps, ignore DTS
                cmd.arg("-avoid_negative_ts").arg("make_zero"); // Fix negative timestamps
                cmd.arg("-max_muxing_queue_size").arg("9999"); // Handle complex streams
                cmd.arg("-err_detect").arg("ignore_err"); // Ignore non-fatal errors
                cmd.arg("-copy_unknown"); // Copy unknown streams (compatibility)
                // Note: movflags will be set later in the general faststart section
                cmd.arg("-f").arg("mp4"); // Force MP4 format
            }
            "mkv" => {
                tracing::info!("Applying MKV-specific FFmpeg options");
                // MKV files can have complex subtitle/attachment streams
                cmd.arg("-map").arg("0:v").arg("-map").arg("0:a?"); // Only video and audio
            }
            _ => {
                tracing::debug!("No special options for extension: {}", input_extension);
            }
        }

        // Handle codec settings - special handling for AVI audio compatibility
        cmd.arg("-c:v").arg(&config.video_codec);

        // If we're transcoding video to H.264, add optimized settings for streaming
        if config.video_codec == "libx264" {
            cmd.arg("-preset")
                .arg("ultrafast") // Prioritize speed over compression
                .arg("-crf")
                .arg("28") // Lower quality but much faster
                .arg("-profile:v")
                .arg("baseline") // Baseline profile for max compatibility
                .arg("-level")
                .arg("3.0") // Lower level for faster encoding
                .arg("-pix_fmt")
                .arg("yuv420p") // Ensure compatible pixel format
                .arg("-tune")
                .arg("zerolatency") // Optimize for low latency
                .arg("-x264opts")
                .arg("keyint=30:min-keyint=30"); // Frequent keyframes for seeking
            tracing::info!(
                "Using H.264 transcoding with speed-optimized settings for fast streaming"
            );
        }

        // For AVI files, check if we need to convert audio for MP4 compatibility
        if input_extension.to_lowercase() == "avi" && config.audio_codec == "copy" {
            // AVI often has MP3/PCM audio which may not work well in MP4
            // Use AAC for better compatibility
            cmd.arg("-c:a").arg("aac").arg("-b:a").arg("192k");
            tracing::info!("Converting AVI audio to AAC for MP4 compatibility");
        } else {
            cmd.arg("-c:a").arg(&config.audio_codec);
        }

        // Add streaming optimization - use fragmented MP4 for progressive streaming
        if config.faststart {
            // For progressive streaming, use fragmented MP4 instead of faststart
            // This allows headers to be written early without waiting for completion
            cmd.arg("-movflags")
                .arg("frag_keyframe+empty_moov+default_base_moof");
        } else {
            // Even without faststart config, we should use fragmented MP4 for streaming
            cmd.arg("-movflags")
                .arg("frag_keyframe+empty_moov+default_base_moof");
        }

        // Force MP4 output format
        cmd.arg("-f").arg("mp4");

        cmd.arg(output_path);

        // Execute FFmpeg command
        tracing::info!("Executing FFmpeg command: {:?}", cmd);

        let output = cmd.output().await.map_err(|e| {
            tracing::error!("Failed to execute FFmpeg: {}", e);
            StreamingError::FfmpegError {
                reason: format!("Failed to execute ffmpeg: {e}"),
            }
        })?;

        // Log FFmpeg output for debugging
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);

        if !stdout.is_empty() {
            tracing::info!("FFmpeg stdout: {}", stdout);
        }
        if !stderr.is_empty() {
            tracing::warn!("FFmpeg stderr: {}", stderr);
        }

        if !output.status.success() {
            tracing::error!("FFmpeg failed with exit code {}: {}", output.status, stderr);
            return Err(StreamingError::FfmpegError {
                reason: format!("FFmpeg failed with exit code {}: {stderr}", output.status),
            });
        }

        // Get output file size
        let output_size = std::fs::metadata(output_path)
            .map_err(|e| StreamingError::IoErrorWithOperation {
                operation: "get output file size".to_string(),
                source: e,
            })?
            .len();

        // Validate that the output is a valid MP4 file
        if output_size < 100 {
            return Err(StreamingError::FfmpegError {
                reason: format!("Output file too small ({output_size} bytes), likely corrupt"),
            });
        }

        // Validate MP4 structure more thoroughly
        if let Ok(mut file) = std::fs::File::open(output_path) {
            use std::io::{Read, Seek, SeekFrom};

            // Check for ftyp box at the beginning
            let mut header = [0u8; 12];
            if file.read_exact(&mut header).is_err() {
                return Err(StreamingError::FfmpegError {
                    reason: "Output file too small to be valid MP4".to_string(),
                });
            }

            if &header[4..8] != b"ftyp" {
                tracing::error!(
                    "Invalid MP4 output - missing ftyp box. Header: {:?}",
                    &header[0..12]
                );
                return Err(StreamingError::FfmpegError {
                    reason: "Output file does not contain valid MP4 ftyp box".to_string(),
                });
            }

            // Check for moov atom (required for playback)
            let mut found_moov = false;
            let mut pos = 0u64;

            while pos < output_size {
                if file.seek(SeekFrom::Start(pos)).is_err() {
                    break;
                }

                let mut size_and_type = [0u8; 8];
                if file.read_exact(&mut size_and_type).is_err() {
                    break;
                }

                let atom_size = u32::from_be_bytes([
                    size_and_type[0],
                    size_and_type[1],
                    size_and_type[2],
                    size_and_type[3],
                ]) as u64;

                let atom_type = &size_and_type[4..8];

                if atom_type == b"moov" {
                    found_moov = true;
                    tracing::debug!("Found moov atom at position {}", pos);
                    break;
                }

                if atom_size == 0 || atom_size > output_size {
                    break;
                }

                pos += atom_size;
            }

            if !found_moov {
                return Err(StreamingError::FfmpegError {
                    reason: "Output MP4 missing moov atom - file is unplayable".to_string(),
                });
            }

            tracing::debug!("MP4 validation passed - ftyp and moov atoms found");
        }

        let processing_time = start_time.elapsed().as_secs_f64();

        tracing::info!(
            "Successfully remuxed {} to MP4: {} bytes in {:.2}s",
            input_path.display(),
            output_size,
            processing_time
        );

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
            .map_err(|e| StreamingError::IoErrorWithOperation {
                operation: "get input file size".to_string(),
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
