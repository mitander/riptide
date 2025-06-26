//! Media file analysis functions for streaming simulation

use std::path::Path;
use std::time::Duration;

use tokio::fs;

use super::types::{MediaFile, MediaFileType, MovieFolder, StreamingPriority, StreamingProfile};

/// Media file analysis functions.
pub struct MediaAnalyzer;

impl MediaAnalyzer {
    /// Analyzes movie folder to extract media information.
    pub async fn analyze_movie_folder(
        folder_path: &Path,
    ) -> Result<MovieFolder, Box<dyn std::error::Error>> {
        let mut entries = fs::read_dir(folder_path).await?;
        let mut files = Vec::new();
        let mut total_size = 0;
        let mut primary_video = None;
        let mut subtitle_files = Vec::new();

        while let Some(entry) = entries.next_entry().await? {
            let path = entry.path();
            if path.is_file() {
                let metadata = entry.metadata().await?;
                let size = metadata.len();
                total_size += size;

                if let Some(media_file) = Self::classify_media_file(&path, size).await? {
                    match &media_file.file_type {
                        MediaFileType::Video { .. } => {
                            if primary_video.is_none() {
                                primary_video = Some(files.len());
                            }
                        }
                        MediaFileType::Subtitle { .. } => {
                            subtitle_files.push(files.len());
                        }
                        _ => {}
                    }
                    files.push(media_file);
                }
            }
        }

        let streaming_profile = Self::determine_streaming_profile(&files, primary_video);

        Ok(MovieFolder {
            path: folder_path.to_path_buf(),
            name: folder_path
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string(),
            total_size,
            files,
            primary_video,
            subtitle_files,
            streaming_profile,
        })
    }

    /// Classifies a file based on extension and size.
    pub async fn classify_media_file(
        path: &Path,
        size: u64,
    ) -> Result<Option<MediaFile>, Box<dyn std::error::Error>> {
        let extension = path
            .extension()
            .and_then(|ext| ext.to_str())
            .unwrap_or("")
            .to_lowercase();

        let (file_type, priority) = match extension.as_str() {
            // Video files
            "mp4" | "mkv" | "avi" | "mov" | "wmv" | "webm" => {
                let bitrate = Self::estimate_video_bitrate(size);
                let duration = Self::estimate_duration(size, bitrate);
                (
                    MediaFileType::Video {
                        codec: Self::guess_codec(&extension),
                        bitrate,
                        duration,
                    },
                    StreamingPriority::Critical,
                )
            }

            // Subtitle files
            "srt" | "vtt" | "ass" | "ssa" | "sub" | "idx" => {
                let language = Self::extract_language_from_filename(path);
                (
                    MediaFileType::Subtitle {
                        language: language.clone(),
                        format: extension.clone(),
                    },
                    if Self::is_primary_language(&language) {
                        StreamingPriority::High
                    } else {
                        StreamingPriority::Medium
                    },
                )
            }

            // Audio files
            "mp3" | "aac" | "flac" | "ogg" => (
                MediaFileType::Audio {
                    codec: extension.clone(),
                    bitrate: Self::estimate_audio_bitrate(size),
                },
                StreamingPriority::Medium,
            ),

            // Metadata files
            "nfo" | "txt" | "xml" => (
                MediaFileType::Metadata {
                    format: extension.clone(),
                },
                StreamingPriority::Low,
            ),

            // Image files
            "jpg" | "jpeg" | "png" | "bmp" | "gif" => (
                MediaFileType::Image {
                    purpose: if path
                        .file_name()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .to_lowercase()
                        .contains("poster")
                    {
                        "poster".to_string()
                    } else {
                        "thumbnail".to_string()
                    },
                },
                StreamingPriority::Background,
            ),

            _ => return Ok(None), // Unknown file type
        };

        Ok(Some(MediaFile {
            path: path.to_path_buf(),
            size,
            file_type,
            priority,
        }))
    }

    /// Estimates video bitrate from file size (rough approximation).
    fn estimate_video_bitrate(size: u64) -> u64 {
        // Assume ~2 hour movie, rough bitrate calculation
        let estimated_duration_seconds = 7200; // 2 hours
        let size_bits = size * 8;
        size_bits / estimated_duration_seconds
    }

    /// Estimates duration from size and bitrate.
    fn estimate_duration(size: u64, bitrate: u64) -> Duration {
        if bitrate == 0 {
            return Duration::from_secs(7200); // Default 2 hours
        }
        let size_bits = size * 8;
        Duration::from_secs(size_bits / bitrate)
    }

    /// Estimates audio bitrate.
    fn estimate_audio_bitrate(size: u64) -> u64 {
        // Assume typical audio track duration
        let estimated_duration_seconds = 7200;
        let size_bits = size * 8;
        (size_bits / estimated_duration_seconds).min(320_000) // Cap at 320kbps
    }

    /// Guesses codec from file extension.
    fn guess_codec(extension: &str) -> String {
        match extension {
            "mp4" => "h264".to_string(),
            "mkv" => "h265".to_string(),
            "webm" => "vp9".to_string(),
            _ => "unknown".to_string(),
        }
    }

    /// Extracts language from filename patterns.
    fn extract_language_from_filename(path: &Path) -> String {
        let filename = path
            .file_stem()
            .unwrap_or_default()
            .to_string_lossy()
            .to_lowercase();

        if filename.contains(".en") || filename.contains("english") {
            "en".to_string()
        } else if filename.contains(".es") || filename.contains("spanish") {
            "es".to_string()
        } else if filename.contains(".fr") || filename.contains("french") {
            "fr".to_string()
        } else if filename.contains(".de") || filename.contains("german") {
            "de".to_string()
        } else {
            "unknown".to_string()
        }
    }

    /// Determines if language is primary (English assumed).
    fn is_primary_language(language: &str) -> bool {
        language == "en" || language == "unknown"
    }

    /// Determines streaming profile based on video characteristics.
    fn determine_streaming_profile(
        files: &[MediaFile],
        primary_video: Option<usize>,
    ) -> StreamingProfile {
        if let Some(video_index) = primary_video
            && let MediaFileType::Video { bitrate, .. } = &files[video_index].file_type
        {
            return StreamingProfile {
                bitrate: *bitrate,
                buffer_duration: Duration::from_secs(if *bitrate > 10_000_000 { 60 } else { 30 }),
                startup_buffer: Duration::from_secs(if *bitrate > 5_000_000 { 8 } else { 5 }),
                seek_buffer: Duration::from_secs(15),
                subtitle_sync_tolerance: Duration::from_millis(200),
            };
        }

        StreamingProfile::default()
    }
}
