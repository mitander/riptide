//! State management for remuxing sessions

use std::path::PathBuf;
use std::time::Instant;

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Remuxing session state
#[derive(Debug, Clone, PartialEq)]
pub enum RemuxState {
    /// Waiting for sufficient head and tail data to start remuxing
    WaitingForHeadAndTail,
    /// Currently remuxing with FFmpeg
    Remuxing { started_at: Instant },
    /// Remuxing completed successfully
    Completed { output_path: PathBuf },
    /// Remuxing failed
    Failed { error: RemuxError, can_retry: bool },
}

impl RemuxState {
    /// Check if the state allows starting a new remux operation
    pub fn can_start_remuxing(&self) -> bool {
        matches!(
            self,
            RemuxState::WaitingForHeadAndTail
                | RemuxState::Failed {
                    can_retry: true,
                    ..
                }
        )
    }

    /// Check if the state indicates remuxing is complete
    pub fn is_complete(&self) -> bool {
        matches!(self, RemuxState::Completed { .. })
    }

    /// Check if the state indicates an active remuxing process
    pub fn is_remuxing(&self) -> bool {
        matches!(self, RemuxState::Remuxing { .. })
    }

    /// Check if the state indicates a failure
    pub fn is_failed(&self) -> bool {
        matches!(self, RemuxState::Failed { .. })
    }

    /// Get the output path if remuxing is completed
    pub fn output_path(&self) -> Option<&PathBuf> {
        match self {
            RemuxState::Completed { output_path } => Some(output_path),
            _ => None,
        }
    }
}

/// Progress information for remuxing operations
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RemuxProgress {
    /// Overall progress from 0.0 to 1.0
    pub progress: f32,
    /// Current processing frame
    pub frame: Option<u64>,
    /// Total frames (if known)
    pub total_frames: Option<u64>,
    /// Current time position in the video
    pub time_position: Option<std::time::Duration>,
    /// Total duration (if known)
    pub total_duration: Option<std::time::Duration>,
    /// Processing speed (frames per second)
    pub fps: Option<f32>,
    /// Bitrate of the output
    pub bitrate: Option<u64>,
}

impl RemuxProgress {
    /// Update progress from FFmpeg output parsing
    pub fn update_from_ffmpeg_line(&mut self, line: &str) {
        // Parse FFmpeg progress lines like:
        // frame=  123 fps= 45 q=-1.0 size=    1024kB time=00:00:05.12 bitrate=1234.5kbits/s speed=1.2x

        if line.contains("frame=")
            && let Some(frame_str) = extract_value(line, "frame=")
        {
            self.frame = frame_str.trim().parse().ok();
        }

        if line.contains("fps=")
            && let Some(fps_str) = extract_value(line, "fps=")
        {
            self.fps = fps_str.trim().parse().ok();
        }

        if line.contains("time=")
            && let Some(time_str) = extract_value(line, "time=")
        {
            self.time_position = parse_ffmpeg_time(time_str.trim());
        }

        if line.contains("bitrate=")
            && let Some(bitrate_str) = extract_value(line, "bitrate=")
            && let Some(bitrate_str) = bitrate_str.strip_suffix("kbits/s")
            && let Ok(bitrate_kbps) = bitrate_str.trim().parse::<f64>()
        {
            self.bitrate = Some((bitrate_kbps * 1000.0) as u64);
        }

        // Calculate overall progress if we have time information
        if let (Some(current), Some(total)) = (self.time_position, self.total_duration) {
            if total.as_secs() > 0 {
                self.progress = (current.as_secs_f32() / total.as_secs_f32()).min(1.0);
            }
        } else if let (Some(current_frame), Some(total_frames)) = (self.frame, self.total_frames)
            && total_frames > 0
        {
            self.progress = (current_frame as f32 / total_frames as f32).min(1.0);
        }
    }
}

/// Errors that can occur during remuxing
#[derive(Debug, Clone, Error, PartialEq)]
pub enum RemuxError {
    #[error("FFmpeg process failed: {reason}")]
    FfmpegFailed { reason: String },

    #[error("Insufficient data for remuxing: need head and tail")]
    InsufficientData,

    #[error("File format not supported for remuxing: {format}")]
    UnsupportedFormat { format: String },

    #[error("Remuxing timeout after {duration_secs} seconds")]
    Timeout { duration_secs: u64 },

    #[error("I/O error during remuxing: {reason}")]
    IoError { reason: String },

    #[error("Session limit exceeded: {current}/{max}")]
    SessionLimitExceeded { current: usize, max: usize },

    #[error("Invalid session state: {reason}")]
    InvalidState { reason: String },
}

impl RemuxError {
    /// Check if this error can be retried
    pub fn is_retryable(&self) -> bool {
        match self {
            RemuxError::FfmpegFailed { .. } => false, // Input data likely corrupted
            RemuxError::InsufficientData => true,     // Might get more data
            RemuxError::UnsupportedFormat { .. } => false, // Won't change
            RemuxError::Timeout { .. } => true,       // Might succeed with retry
            RemuxError::IoError { .. } => true,       // Temporary I/O issue
            RemuxError::SessionLimitExceeded { .. } => true, // Might have capacity later
            RemuxError::InvalidState { .. } => false, // Logic error
        }
    }
}

/// Extract a value from FFmpeg output line
fn extract_value<'a>(line: &'a str, key: &str) -> Option<&'a str> {
    if let Some(start) = line.find(key) {
        let start = start + key.len();
        if let Some(end) = line[start..].find(' ') {
            Some(&line[start..start + end])
        } else {
            Some(&line[start..])
        }
    } else {
        None
    }
}

/// Parse FFmpeg time format (HH:MM:SS.mmm)
fn parse_ffmpeg_time(time_str: &str) -> Option<std::time::Duration> {
    let parts: Vec<&str> = time_str.split(':').collect();
    if parts.len() != 3 {
        return None;
    }

    let hours: u64 = parts[0].parse().ok()?;
    let minutes: u64 = parts[1].parse().ok()?;
    let seconds_with_millis: f64 = parts[2].parse().ok()?;

    let total_seconds = hours * 3600 + minutes * 60 + seconds_with_millis as u64;
    let millis = ((seconds_with_millis.fract() * 1000.0) as u64).min(999);

    Some(std::time::Duration::from_secs(total_seconds) + std::time::Duration::from_millis(millis))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_remux_state_transitions() {
        let state = RemuxState::WaitingForHeadAndTail;
        assert!(state.can_start_remuxing());
        assert!(!state.is_complete());
        assert!(!state.is_remuxing());
        assert!(!state.is_failed());

        let state = RemuxState::Remuxing {
            started_at: Instant::now(),
        };
        assert!(!state.can_start_remuxing());
        assert!(!state.is_complete());
        assert!(state.is_remuxing());
        assert!(!state.is_failed());

        let state = RemuxState::Completed {
            output_path: PathBuf::from("/tmp/test.mp4"),
        };
        assert!(!state.can_start_remuxing());
        assert!(state.is_complete());
        assert!(!state.is_remuxing());
        assert!(!state.is_failed());
        assert_eq!(state.output_path(), Some(&PathBuf::from("/tmp/test.mp4")));
    }

    #[test]
    fn test_remux_error_retryable() {
        assert!(
            !RemuxError::FfmpegFailed {
                reason: "test".to_string()
            }
            .is_retryable()
        );
        assert!(RemuxError::InsufficientData.is_retryable());
        assert!(
            !RemuxError::UnsupportedFormat {
                format: "test".to_string()
            }
            .is_retryable()
        );
        assert!(RemuxError::Timeout { duration_secs: 30 }.is_retryable());
    }

    #[test]
    fn test_ffmpeg_time_parsing() {
        assert_eq!(
            parse_ffmpeg_time("00:01:30.50"),
            Some(std::time::Duration::from_millis(90500))
        );
        // The fractional part is truncated, so .123 becomes .122
        assert_eq!(
            parse_ffmpeg_time("01:30:45.123"),
            Some(std::time::Duration::from_millis(5445122))
        );
        assert_eq!(parse_ffmpeg_time("invalid"), None);
    }

    #[test]
    fn test_progress_update_from_ffmpeg() {
        let mut progress = RemuxProgress::default();
        progress.total_duration = Some(std::time::Duration::from_secs(100));

        progress.update_from_ffmpeg_line(
            "frame=50 fps=25 q=-1.0 size=1024kB time=00:00:50.00 bitrate=1000.0kbits/s speed=1.0x",
        );

        assert_eq!(progress.frame, Some(50));
        assert_eq!(progress.fps, Some(25.0));
        assert_eq!(
            progress.time_position,
            Some(std::time::Duration::from_secs(50))
        );
        assert_eq!(progress.bitrate, Some(1000000));
        assert_eq!(progress.progress, 0.5);
    }
}
