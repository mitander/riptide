//! Media-aware simulation for testing streaming with real movie files
//!
//! Enables testing streaming algorithms against real media content including
//! movies, subtitles, and metadata to catch real-world streaming issues.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use tokio::fs;

use super::deterministic::{DeterministicSimulation, EventType, SimulationEvent};
use crate::torrent::PieceIndex;

/// Media file types found in movie folders.
#[derive(Debug, Clone, PartialEq)]
pub enum MediaFileType {
    Video {
        codec: String,
        bitrate: u64,
        duration: Duration,
    },
    Subtitle {
        language: String,
        format: String,
    },
    Metadata {
        format: String,
    },
    Audio {
        codec: String,
        bitrate: u64,
    },
    Image {
        purpose: String,
    }, // Cover art, thumbnails, etc.
}

/// Individual media file within a movie folder.
#[derive(Debug, Clone)]
pub struct MediaFile {
    pub path: PathBuf,
    pub size: u64,
    pub file_type: MediaFileType,
    pub priority: StreamingPriority,
}

/// Streaming priority levels for different file types.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum StreamingPriority {
    Critical,   // Video file start - must have for playback
    High,       // Video continuation, primary subtitles
    Medium,     // Secondary subtitles, metadata
    Low,        // Cover art, thumbnails
    Background, // Extra content, commentary tracks
}

/// Complete movie folder content analysis.
#[derive(Debug, Clone)]
pub struct MovieFolder {
    pub path: PathBuf,
    pub name: String,
    pub total_size: u64,
    pub files: Vec<MediaFile>,
    pub primary_video: Option<usize>, // Index into files vec
    pub subtitle_files: Vec<usize>,   // Indices of subtitle files
    pub streaming_profile: StreamingProfile,
}

/// Streaming characteristics for different content types.
#[derive(Debug, Clone)]
pub struct StreamingProfile {
    pub bitrate: u64,                      // Target playback bitrate
    pub buffer_duration: Duration,         // How much to buffer ahead
    pub startup_buffer: Duration,          // Initial buffer before playback
    pub seek_buffer: Duration,             // Buffer for seeking operations
    pub subtitle_sync_tolerance: Duration, // Subtitle timing tolerance
}

impl Default for StreamingProfile {
    fn default() -> Self {
        Self {
            bitrate: 5_000_000,                                  // 5 Mbps
            buffer_duration: Duration::from_secs(30),            // 30 second buffer
            startup_buffer: Duration::from_secs(5),              // 5 second startup
            seek_buffer: Duration::from_secs(10),                // 10 second seek buffer
            subtitle_sync_tolerance: Duration::from_millis(200), // 200ms sync tolerance
        }
    }
}

/// Media-aware streaming simulation.
pub struct MediaStreamingSimulation {
    simulation: DeterministicSimulation,
    movie_folder: MovieFolder,
    piece_size: u32,
    piece_to_file_map: HashMap<PieceIndex, usize>, // Maps pieces to file indices
    #[allow(dead_code)]
    playback_position: Duration,
    #[allow(dead_code)]
    buffer_state: StreamingBuffer,
}

/// Streaming buffer state tracking.
#[derive(Debug, Clone)]
pub struct StreamingBuffer {
    #[allow(dead_code)]
    buffered_duration: Duration,
    #[allow(dead_code)]
    critical_pieces: Vec<PieceIndex>, // Pieces needed for immediate playback
    #[allow(dead_code)]
    prefetch_pieces: Vec<PieceIndex>, // Pieces for future playback
    #[allow(dead_code)]
    subtitle_pieces: Vec<PieceIndex>, // Subtitle data pieces
}

impl Default for StreamingBuffer {
    fn default() -> Self {
        Self {
            buffered_duration: Duration::ZERO,
            critical_pieces: Vec::new(),
            prefetch_pieces: Vec::new(),
            subtitle_pieces: Vec::new(),
        }
    }
}

impl MediaStreamingSimulation {
    /// Creates media streaming simulation from real movie folder.
    ///
    /// Analyzes folder contents and sets up simulation based on actual
    /// file sizes and streaming requirements.
    ///
    /// # Errors
    /// - I/O errors reading folder contents
    /// - Invalid movie folder structure
    pub async fn from_movie_folder(
        folder_path: &Path,
        seed: u64,
        piece_size: u32,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let movie_folder = Self::analyze_movie_folder(folder_path).await?;
        let simulation = DeterministicSimulation::from_seed(seed);
        let piece_to_file_map = Self::build_piece_mapping(&movie_folder, piece_size);

        Ok(Self {
            simulation,
            movie_folder,
            piece_size,
            piece_to_file_map,
            playback_position: Duration::ZERO,
            buffer_state: StreamingBuffer::default(),
        })
    }

    /// Analyzes movie folder to extract media information.
    async fn analyze_movie_folder(
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
    async fn classify_media_file(
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
                        language,
                        format: extension.clone(),
                    },
                    if Self::is_primary_language(&Self::extract_language_from_filename(path)) {
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
        if let Some(video_idx) = primary_video {
            if let MediaFileType::Video { bitrate, .. } = &files[video_idx].file_type {
                return StreamingProfile {
                    bitrate: *bitrate,
                    buffer_duration: Duration::from_secs(if *bitrate > 10_000_000 {
                        60
                    } else {
                        30
                    }),
                    startup_buffer: Duration::from_secs(if *bitrate > 5_000_000 { 8 } else { 5 }),
                    seek_buffer: Duration::from_secs(15),
                    subtitle_sync_tolerance: Duration::from_millis(200),
                };
            }
        }

        StreamingProfile::default()
    }

    /// Builds mapping from pieces to files.
    fn build_piece_mapping(
        movie_folder: &MovieFolder,
        piece_size: u32,
    ) -> HashMap<PieceIndex, usize> {
        let mut mapping = HashMap::new();
        let mut current_piece = 0u32;
        let mut current_offset = 0u64;

        for (file_idx, file) in movie_folder.files.iter().enumerate() {
            let _file_start_piece = current_piece;
            let file_end_offset = current_offset + file.size;

            // Map all pieces that contain this file's data
            while current_offset < file_end_offset {
                mapping.insert(PieceIndex::new(current_piece), file_idx);
                current_piece += 1;
                current_offset += piece_size as u64;
            }
        }

        mapping
    }

    /// Starts streaming simulation with realistic timing.
    pub fn start_streaming_simulation(&mut self) {
        // Schedule initial buffer building
        self.schedule_startup_buffering();

        // Schedule subtitle loading
        self.schedule_subtitle_loading();

        // Schedule periodic playback progression
        self.schedule_playback_progression();
    }

    /// Schedules critical pieces for startup buffering.
    fn schedule_startup_buffering(&mut self) {
        if let Some(video_idx) = self.movie_folder.primary_video {
            let startup_duration = self.movie_folder.streaming_profile.startup_buffer;
            let startup_pieces = self.calculate_pieces_for_duration(video_idx, startup_duration);

            for (delay_ms, piece_idx) in startup_pieces.iter().enumerate() {
                let request_time =
                    self.simulation.clock().now() + Duration::from_millis(delay_ms as u64);
                let event = SimulationEvent {
                    timestamp: request_time,
                    event_type: EventType::PieceRequest,
                    peer_id: None,
                    piece_index: Some(*piece_idx),
                };
                self.simulation.schedule_event(event);
            }
        }
    }

    /// Schedules subtitle file loading.
    fn schedule_subtitle_loading(&mut self) {
        for &subtitle_idx in &self.movie_folder.subtitle_files {
            let file = &self.movie_folder.files[subtitle_idx];

            // High priority subtitles load early
            let delay = match file.priority {
                StreamingPriority::High => Duration::from_secs(2),
                StreamingPriority::Medium => Duration::from_secs(5),
                _ => Duration::from_secs(10),
            };

            let subtitle_pieces = self.calculate_pieces_for_file(subtitle_idx);
            for piece_idx in subtitle_pieces {
                let request_time = self.simulation.clock().now() + delay;
                let event = SimulationEvent {
                    timestamp: request_time,
                    event_type: EventType::PieceRequest,
                    peer_id: None,
                    piece_index: Some(piece_idx),
                };
                self.simulation.schedule_event(event);
            }
        }
    }

    /// Schedules ongoing playback progression.
    fn schedule_playback_progression(&mut self) {
        // Schedule regular buffer checks every 5 seconds
        for i in 1..60 {
            // 5 minute simulation
            let check_time = self.simulation.clock().now() + Duration::from_secs(i * 5);
            let event = SimulationEvent {
                timestamp: check_time,
                event_type: EventType::NetworkChange, // Reuse for buffer check
                peer_id: None,
                piece_index: None,
            };
            self.simulation.schedule_event(event);
        }
    }

    /// Calculates pieces needed for a duration of video.
    fn calculate_pieces_for_duration(
        &self,
        file_idx: usize,
        duration: Duration,
    ) -> Vec<PieceIndex> {
        let file = &self.movie_folder.files[file_idx];
        if let MediaFileType::Video {
            bitrate: _,
            duration: total_duration,
            ..
        } = &file.file_type
        {
            let fraction = duration.as_secs_f64() / total_duration.as_secs_f64();
            let bytes_needed = (file.size as f64 * fraction) as u64;
            let pieces_needed = (bytes_needed / self.piece_size as u64) as u32;

            // Find starting piece for this file
            let start_piece = self
                .piece_to_file_map
                .iter()
                .find(|&(_, idx)| *idx == file_idx)
                .map(|(piece_idx, _)| piece_idx.as_u32())
                .unwrap_or(0);

            (start_piece..start_piece + pieces_needed)
                .map(PieceIndex::new)
                .collect()
        } else {
            Vec::new()
        }
    }

    /// Calculates all pieces for a complete file.
    fn calculate_pieces_for_file(&self, file_idx: usize) -> Vec<PieceIndex> {
        self.piece_to_file_map
            .iter()
            .filter(|&(_, idx)| *idx == file_idx)
            .map(|(piece_idx, _)| *piece_idx)
            .collect()
    }

    /// Runs streaming simulation with media awareness.
    pub fn run_streaming_simulation(&mut self, duration: Duration) -> StreamingResult {
        let start_time = self.simulation.clock().now();
        let events = self.simulation.run_for(duration);
        let end_time = self.simulation.clock().now();

        // Analyze streaming performance
        let piece_requests = events
            .iter()
            .filter(|e| e.event_type == EventType::PieceRequest)
            .count();

        let piece_completions = events
            .iter()
            .filter(|e| e.event_type == EventType::PieceComplete)
            .count();

        // Calculate streaming-specific metrics
        let video_pieces = self.count_video_pieces(&events);
        let subtitle_pieces = self.count_subtitle_pieces(&events);
        let buffering_events = self.count_buffering_events(&events);

        StreamingResult {
            duration: end_time.duration_since(start_time),
            total_events: events.len(),
            piece_requests,
            piece_completions,
            video_pieces_completed: video_pieces,
            subtitle_pieces_completed: subtitle_pieces,
            buffering_events,
            streaming_efficiency: if piece_requests > 0 {
                piece_completions as f64 / piece_requests as f64
            } else {
                0.0
            },
            subtitle_sync_issues: self.detect_subtitle_sync_issues(&events),
        }
    }

    /// Counts video-related piece completions.
    fn count_video_pieces(&self, events: &[SimulationEvent]) -> usize {
        events
            .iter()
            .filter(|e| {
                e.event_type == EventType::PieceComplete
                    && e.piece_index.is_some_and(|piece_idx| {
                        self.piece_to_file_map
                            .get(&piece_idx)
                            .is_some_and(|&file_idx| {
                                matches!(
                                    self.movie_folder.files[file_idx].file_type,
                                    MediaFileType::Video { .. }
                                )
                            })
                    })
            })
            .count()
    }

    /// Counts subtitle-related piece completions.
    fn count_subtitle_pieces(&self, events: &[SimulationEvent]) -> usize {
        events
            .iter()
            .filter(|e| {
                e.event_type == EventType::PieceComplete
                    && e.piece_index.is_some_and(|piece_idx| {
                        self.piece_to_file_map
                            .get(&piece_idx)
                            .is_some_and(|&file_idx| {
                                matches!(
                                    self.movie_folder.files[file_idx].file_type,
                                    MediaFileType::Subtitle { .. }
                                )
                            })
                    })
            })
            .count()
    }

    /// Counts buffering-related events.
    fn count_buffering_events(&self, events: &[SimulationEvent]) -> usize {
        events
            .iter()
            .filter(|e| e.event_type == EventType::NetworkChange)
            .count()
    }

    /// Detects potential subtitle synchronization issues.
    fn detect_subtitle_sync_issues(&self, _events: &[SimulationEvent]) -> usize {
        // Simple heuristic: subtitle pieces arriving much later than video pieces
        let sync_issues = 0;
        let _tolerance = self.movie_folder.streaming_profile.subtitle_sync_tolerance;

        // Implementation would analyze timing between video and subtitle piece arrivals
        // For now, return placeholder
        sync_issues
    }

    /// Returns reference to analyzed movie folder.
    pub fn movie_folder(&self) -> &MovieFolder {
        &self.movie_folder
    }

    /// Returns current simulation clock.
    pub fn clock(&self) -> &super::deterministic::DeterministicClock {
        self.simulation.clock()
    }
}

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

        if let Some(video_idx) = movie_folder.primary_video {
            let video_file = &movie_folder.files[video_idx];
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

#[cfg(test)]
mod tests {
    use tempfile::{TempDir, tempdir};
    use tokio::fs::File;
    use tokio::io::AsyncWriteExt;

    use super::*;

    async fn create_test_movie_folder() -> (PathBuf, TempDir) {
        let temp_dir = tempdir().unwrap();
        let movie_dir = temp_dir.path().join("Test.Movie.2024");
        fs::create_dir(&movie_dir).await.unwrap();

        // Create test video file
        let mut video_file = File::create(movie_dir.join("Test.Movie.2024.1080p.mp4"))
            .await
            .unwrap();
        video_file.write_all(&vec![0; 1_000_000]).await.unwrap(); // 1MB test file

        // Create subtitle files
        let mut sub_en = File::create(movie_dir.join("Test.Movie.2024.en.srt"))
            .await
            .unwrap();
        sub_en
            .write_all(b"1\n00:00:01,000 --> 00:00:03,000\nTest subtitle\n")
            .await
            .unwrap();

        let mut sub_es = File::create(movie_dir.join("Test.Movie.2024.es.srt"))
            .await
            .unwrap();
        sub_es
            .write_all(b"1\n00:00:01,000 --> 00:00:03,000\nSubtitulo de prueba\n")
            .await
            .unwrap();

        // Create metadata file
        let mut nfo_file = File::create(movie_dir.join("Test.Movie.2024.nfo"))
            .await
            .unwrap();
        nfo_file
            .write_all(b"<movie><title>Test Movie</title></movie>")
            .await
            .unwrap();

        (movie_dir, temp_dir)
    }

    #[tokio::test]
    async fn test_movie_folder_analysis() {
        let (movie_dir, _temp_dir) = create_test_movie_folder().await;
        let movie_folder = MediaStreamingSimulation::analyze_movie_folder(&movie_dir)
            .await
            .unwrap();

        assert_eq!(movie_folder.name, "Test.Movie.2024");
        assert!(movie_folder.files.len() >= 3); // Video, 2 subs, metadata
        assert!(movie_folder.primary_video.is_some());
        assert_eq!(movie_folder.subtitle_files.len(), 2);
    }

    #[tokio::test]
    async fn test_media_file_classification() {
        let (movie_dir, _temp_dir) = create_test_movie_folder().await;
        let video_path = movie_dir.join("Test.Movie.2024.1080p.mp4");

        let media_file = MediaStreamingSimulation::classify_media_file(&video_path, 1_000_000)
            .await
            .unwrap()
            .unwrap();

        assert!(matches!(media_file.file_type, MediaFileType::Video { .. }));
        assert_eq!(media_file.priority, StreamingPriority::Critical);
    }

    #[tokio::test]
    async fn test_streaming_simulation() {
        let (movie_dir, _temp_dir) = create_test_movie_folder().await;
        let mut simulation = MediaStreamingSimulation::from_movie_folder(&movie_dir, 42, 262144)
            .await
            .unwrap();

        simulation.start_streaming_simulation();
        let result = simulation.run_streaming_simulation(Duration::from_secs(30));

        assert!(result.total_events > 0);
        assert!(result.piece_requests > 0);
    }
}
