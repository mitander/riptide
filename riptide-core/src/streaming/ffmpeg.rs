//! FFmpeg abstraction for both production and simulation modes

use std::path::Path;

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
        // Check if ffmpeg-next can initialize
        match ffmpeg_next::init() {
            Ok(()) => Ok(()),
            Err(e) => Err(StreamingError::FfmpegError {
                reason: format!("FFmpeg initialization failed: {e}"),
            }),
        }
    }
}

#[async_trait]
impl FfmpegProcessor for ProductionFfmpegProcessor {
    async fn remux_to_mp4(
        &self,
        _input_path: &Path,
        _output_path: &Path,
        _config: &RemuxingOptions,
    ) -> StreamingResult<RemuxingResult> {
        // TODO: Implement proper FFmpeg remuxing
        // For now, return an error since the FFmpeg API integration needs more work
        Err(StreamingError::FfmpegError {
            reason:
                "Production FFmpeg implementation not yet complete - use simulation for testing"
                    .to_string(),
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
pub struct SimulationFfmpegProcessor {
    /// Simulated processing time per megabyte
    processing_speed_mb_per_sec: f64,

    /// Whether to simulate FFmpeg as available
    is_available: bool,

    /// Simulated size change ratio (output/input)
    size_change_ratio: f64,
}

impl SimulationFfmpegProcessor {
    /// Create new simulation processor
    pub fn new() -> Self {
        Self {
            processing_speed_mb_per_sec: 50.0, // Simulate 50 MB/s processing
            is_available: true,
            size_change_ratio: 0.98, // Simulate slight size reduction
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

    /// Configure size change ratio
    pub fn with_size_ratio(mut self, ratio: f64) -> Self {
        self.size_change_ratio = ratio;
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
        _config: &RemuxingOptions,
    ) -> StreamingResult<RemuxingResult> {
        if !self.is_available {
            return Err(StreamingError::FfmpegError {
                reason: "FFmpeg not available in simulation".to_string(),
            });
        }

        // Get input file size
        let input_size = std::fs::metadata(input_path)
            .map_err(|e| StreamingError::IoError {
                operation: "get input file size".to_string(),
                path: input_path.to_string_lossy().to_string(),
                source: e,
            })?
            .len();

        // Simulate processing time based on file size
        let input_mb = input_size as f64 / (1024.0 * 1024.0);
        let processing_time = input_mb / self.processing_speed_mb_per_sec;

        // Simulate processing delay
        tokio::time::sleep(tokio::time::Duration::from_secs_f64(processing_time)).await;

        // Calculate output size
        let output_size = (input_size as f64 * self.size_change_ratio) as u64;

        // Create a dummy output file with the simulated size
        let mut output_file =
            std::fs::File::create(output_path).map_err(|e| StreamingError::IoError {
                operation: "create simulated output file".to_string(),
                path: output_path.to_string_lossy().to_string(),
                source: e,
            })?;

        // Write dummy MP4 header for realistic simulation
        use std::io::Write;

        // Minimal MP4 header
        let mp4_header = [
            0x00, 0x00, 0x00, 0x20, // box size
            b'f', b't', b'y', b'p', // box type 'ftyp'
            b'm', b'p', b'4', b'1', // brand 'mp41'
            0x00, 0x00, 0x00, 0x00, // minor version
            b'm', b'p', b'4', b'1', // compatible brand
            b'i', b's', b'o', b'm', // compatible brand
        ];

        output_file
            .write_all(&mp4_header)
            .map_err(|e| StreamingError::IoError {
                operation: "write simulated MP4 header".to_string(),
                path: output_path.to_string_lossy().to_string(),
                source: e,
            })?;

        // Pad file to simulated size
        let remaining_size = output_size.saturating_sub(mp4_header.len() as u64);
        let padding = vec![0u8; remaining_size as usize];
        output_file
            .write_all(&padding)
            .map_err(|e| StreamingError::IoError {
                operation: "write simulated file padding".to_string(),
                path: output_path.to_string_lossy().to_string(),
                source: e,
            })?;

        output_file
            .sync_all()
            .map_err(|e| StreamingError::IoError {
                operation: "sync simulated output file".to_string(),
                path: output_path.to_string_lossy().to_string(),
                source: e,
            })?;

        Ok(RemuxingResult {
            output_size,
            processing_time,
            streams_reencoded: false, // Simulate copy operation
        })
    }

    fn is_available(&self) -> bool {
        self.is_available
    }

    async fn estimate_output_size(&self, input_path: &Path) -> Option<u64> {
        std::fs::metadata(input_path)
            .ok()
            .map(|m| (m.len() as f64 * self.size_change_ratio) as u64)
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

        // Create dummy input file
        std::fs::write(&input_path, vec![0u8; 1024 * 1024]).unwrap(); // 1MB file

        let processor = SimulationFfmpegProcessor::new()
            .with_speed(10.0) // 10 MB/s for fast test
            .with_size_ratio(0.9);

        let options = RemuxingOptions::default();
        let result = processor
            .remux_to_mp4(&input_path, &output_path, &options)
            .await
            .unwrap();

        // Verify output file was created
        assert!(output_path.exists());

        // Verify simulated size reduction
        assert_eq!(result.output_size, (1024 * 1024 * 90) / 100); // 90% of input

        // Verify processing time simulation
        assert!(result.processing_time > 0.0);
        assert!(result.processing_time < 1.0); // Should be quick for 1MB at 10MB/s
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
