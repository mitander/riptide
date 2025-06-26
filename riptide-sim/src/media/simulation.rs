//! Media-aware streaming simulation core logic

use std::collections::HashMap;
use std::path::Path;
use std::time::Duration;

use riptide_core::torrent::PieceIndex;

use super::analysis::MediaAnalyzer;
use super::results::StreamingResult;
use super::types::{MediaFileType, MovieFolder, StreamingBuffer, StreamingPriority};
use crate::EventPriority;
use crate::deterministic::{DeterministicSimulation, EventType};

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

impl MediaStreamingSimulation {
    /// Helper to schedule events with error logging instead of panicking.
    fn schedule_event(&mut self, delay: Duration, event: EventType, priority: EventPriority) {
        if let Err(e) = self.simulation.schedule_delayed(delay, event, priority) {
            // Log error in simulation context instead of panicking
            eprintln!("Warning: Failed to schedule simulation event: {e}");
        }
    }

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
        let movie_folder = MediaAnalyzer::analyze_movie_folder(folder_path).await?;
        let config = riptide_core::config::SimulationConfig {
            enabled: true,
            deterministic_seed: Some(seed),
            network_latency_ms: 50,
            packet_loss_rate: 0.01,
            max_simulated_peers: 20,
            simulated_download_speed: 5_242_880,
            use_mock_data: true,
        };
        let simulation = DeterministicSimulation::new(config)
            .map_err(|e| format!("Failed to create simulation: {e}"))?;
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

    /// Builds mapping from pieces to files.
    fn build_piece_mapping(
        movie_folder: &MovieFolder,
        piece_size: u32,
    ) -> HashMap<PieceIndex, usize> {
        let mut mapping = HashMap::new();
        let mut current_piece = 0u32;
        let mut current_offset = 0u64;

        for (file_index, file) in movie_folder.files.iter().enumerate() {
            let file_end_offset = current_offset + file.size;

            // Map all pieces that contain this file's data
            while current_offset < file_end_offset {
                mapping.insert(PieceIndex::new(current_piece), file_index);
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
        if let Some(video_index) = self.movie_folder.primary_video {
            let startup_duration = self.movie_folder.streaming_profile.startup_buffer;
            let startup_pieces = self.calculate_pieces_for_duration(video_index, startup_duration);

            for (delay_ms, piece_index) in startup_pieces.iter().enumerate() {
                let delay = Duration::from_millis(delay_ms as u64);
                self.schedule_event(
                    delay,
                    EventType::PieceRequest {
                        peer_id: format!("STREAM_PEER_{}", delay_ms % 10),
                        piece_index: *piece_index,
                    },
                    EventPriority::Normal,
                );
            }
        }
    }

    /// Schedules subtitle file loading.
    fn schedule_subtitle_loading(&mut self) {
        // Clone subtitle file indices to avoid borrow conflict
        let subtitle_indices = self.movie_folder.subtitle_files.clone();

        for subtitle_index in subtitle_indices {
            let file = &self.movie_folder.files[subtitle_index];

            // High priority subtitles load early
            let delay = match file.priority {
                StreamingPriority::High => Duration::from_secs(2),
                StreamingPriority::Medium => Duration::from_secs(5),
                _ => Duration::from_secs(10),
            };

            let subtitle_pieces = self.calculate_pieces_for_file(subtitle_index);
            for piece_index in subtitle_pieces {
                self.schedule_event(
                    delay,
                    EventType::PieceRequest {
                        peer_id: format!("SUBTITLE_PEER_{}", subtitle_index % 5),
                        piece_index,
                    },
                    EventPriority::Low,
                );
            }
        }
    }

    /// Schedules ongoing playback progression.
    fn schedule_playback_progression(&mut self) {
        // Schedule regular buffer checks every 5 seconds
        for i in 1..60 {
            // 5 minute simulation
            let delay = Duration::from_secs(i * 5);
            self.schedule_event(
                delay,
                EventType::NetworkChange {
                    latency_ms: 50,
                    packet_loss_rate: 0.01,
                },
                EventPriority::Low,
            );
        }
    }

    /// Calculates pieces needed for a duration of video.
    fn calculate_pieces_for_duration(
        &self,
        file_index: usize,
        duration: Duration,
    ) -> Vec<PieceIndex> {
        let file = &self.movie_folder.files[file_index];
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
                .find(|&(_, index)| *index == file_index)
                .map(|(piece_index, _)| piece_index.as_u32())
                .unwrap_or(0);

            (start_piece..start_piece + pieces_needed)
                .map(PieceIndex::new)
                .collect()
        } else {
            Vec::new()
        }
    }

    /// Calculates all pieces for a complete file.
    fn calculate_pieces_for_file(&self, file_index: usize) -> Vec<PieceIndex> {
        self.piece_to_file_map
            .iter()
            .filter(|&(_, index)| *index == file_index)
            .map(|(piece_index, _)| *piece_index)
            .collect()
    }

    /// Runs streaming simulation with media awareness.
    ///
    /// # Errors
    /// - `SimulationError` - If simulation fails to run
    pub fn run_streaming_simulation(
        &mut self,
        duration: Duration,
    ) -> Result<StreamingResult, crate::SimulationError> {
        let start_time = self.simulation.clock().now();
        let report = self.simulation.run_for(duration)?;
        let end_time = self.simulation.clock().now();

        // Analyze streaming performance
        // Count piece requests
        let piece_requests = report
            .metrics
            .events_by_type
            .get("PieceRequest")
            .copied()
            .unwrap_or(0) as usize;

        let piece_completions = report
            .metrics
            .events_by_type
            .get("PieceComplete")
            .copied()
            .unwrap_or(0) as usize;

        // Calculate streaming-specific metrics
        let video_pieces = self.count_video_pieces_from_report(&report);
        let subtitle_pieces = self.count_subtitle_pieces_from_report(&report);
        let buffering_events = self.count_buffering_events_from_report(&report);

        Ok(StreamingResult {
            duration: end_time.duration_since(start_time),
            total_events: report.event_count as usize,
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
            subtitle_sync_issues: self.detect_subtitle_sync_issues_from_report(&report),
        })
    }

    /// Counts video-related piece completions from report.
    fn count_video_pieces_from_report(&self, report: &crate::SimulationReport) -> usize {
        // For now, estimate based on completed pieces and file mapping
        report
            .final_state
            .completed_pieces
            .iter()
            .filter(|piece_index| {
                self.piece_to_file_map
                    .get(piece_index)
                    .is_some_and(|&file_index| {
                        matches!(
                            self.movie_folder.files[file_index].file_type,
                            MediaFileType::Video { .. }
                        )
                    })
            })
            .count()
    }

    /// Counts subtitle-related piece completions from report.
    fn count_subtitle_pieces_from_report(&self, report: &crate::SimulationReport) -> usize {
        // Count completed pieces that belong to subtitle files
        report
            .final_state
            .completed_pieces
            .iter()
            .filter(|piece_index| {
                self.piece_to_file_map
                    .get(piece_index)
                    .is_some_and(|&file_index| {
                        matches!(
                            self.movie_folder.files[file_index].file_type,
                            MediaFileType::Subtitle { .. }
                        )
                    })
            })
            .count()
    }

    /// Counts buffering events from report.
    fn count_buffering_events_from_report(&self, report: &crate::SimulationReport) -> usize {
        // For now, count network change events as potential buffering triggers
        report
            .metrics
            .events_by_type
            .get("NetworkChange")
            .copied()
            .unwrap_or(0) as usize
    }

    /// Detects subtitle synchronization issues from report.
    fn detect_subtitle_sync_issues_from_report(&self, _report: &crate::SimulationReport) -> usize {
        // TODO: Implement actual subtitle sync detection by analyzing timing between
        // video and subtitle piece arrivals in the simulation report

        // Simple heuristic: subtitle pieces arriving much later than video pieces
        let sync_issues = 0;
        let _tolerance = self.movie_folder.streaming_profile.subtitle_sync_tolerance;

        // For now, return placeholder
        sync_issues
    }

    /// Returns reference to analyzed movie folder.
    pub fn movie_folder(&self) -> &MovieFolder {
        &self.movie_folder
    }

    /// Returns current simulation clock.
    pub fn clock(&self) -> &super::super::deterministic::DeterministicClock {
        self.simulation.clock()
    }
}
