//! Streaming simulation results and analysis

use std::time::Duration;

use super::types::{MediaFileType, MovieFolder};

/// Results from media streaming simulation.
#[derive(Debug, Clone)]
pub struct StreamingResult {
    pub duration: Duration,
    pub total_events: usize,
    pub piece_requests: usize,
    pub piece_completions: usize,
    pub video_pieces_completed: usize,
    pub subtitle_pieces_completed: usize,
    pub buffering_events: usize,
    pub streaming_efficiency: f64,
    pub subtitle_sync_issues: usize,
}

impl StreamingResult {
    /// Returns video streaming success rate.
    pub fn video_success_rate(&self) -> f64 {
        if self.piece_requests == 0 {
            0.0
        } else {
            self.video_pieces_completed as f64 / self.piece_requests as f64
        }
    }

    /// Returns subtitle availability rate.
    pub fn subtitle_availability(&self) -> f64 {
        if self.subtitle_pieces_completed == 0 {
            0.0
        } else {
            1.0 - (self.subtitle_sync_issues as f64 / self.subtitle_pieces_completed as f64)
        }
    }

    /// Prints detailed streaming analysis.
    pub fn print_analysis(&self, movie_folder: &MovieFolder) {
        println!("Media Streaming Analysis: {}", movie_folder.name);
        println!("{:-<60}", "");

        println!("Content Analysis:");
        println!("  Total files: {}", movie_folder.files.len());
        println!(
            "  Total size: {:.1} GB",
            movie_folder.total_size as f64 / 1_073_741_824.0
        );

        if let Some(video_index) = movie_folder.primary_video {
            let video_file = &movie_folder.files[video_index];
            if let MediaFileType::Video {
                bitrate, duration, ..
            } = &video_file.file_type
            {
                println!(
                    "  Video: {:.1} Mbps, {:?}",
                    *bitrate as f64 / 1_000_000.0,
                    duration
                );
            }
        }

        println!("  Subtitles: {} files", movie_folder.subtitle_files.len());

        println!("\nStreaming Performance:");
        println!(
            "  Overall efficiency: {:.1}%",
            self.streaming_efficiency * 100.0
        );
        println!(
            "  Video success rate: {:.1}%",
            self.video_success_rate() * 100.0
        );
        println!(
            "  Subtitle availability: {:.1}%",
            self.subtitle_availability() * 100.0
        );
        println!("  Buffering events: {}", self.buffering_events);
        println!("  Total events: {}", self.total_events);

        if self.subtitle_sync_issues > 0 {
            println!(
                "  Warning: {} subtitle sync issues detected",
                self.subtitle_sync_issues
            );
        }
    }
}
