//! Simulation state tracking and metrics collection.

use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

use riptide_core::torrent::PieceIndex;

use super::resources::ResourceUsage;
use crate::InvariantViolation;

/// Current state of the simulation.
#[derive(Debug, Clone, Default)]
pub struct SimulationState {
    /// Currently connected peers
    pub connected_peers: HashSet<String>,
    /// Pieces being downloaded
    pub downloading_pieces: HashMap<PieceIndex, String>,
    /// Completed pieces
    pub completed_pieces: HashSet<PieceIndex>,
    /// Failed pieces
    pub failed_pieces: HashMap<PieceIndex, String>,
    /// Current resource usage
    pub resource_usage: ResourceUsage,
    /// Active bandwidth throttles
    pub bandwidth_throttles: HashMap<String, u64>,
}

impl SimulationState {
    /// Creates new simulation state.
    pub fn new() -> Self {
        Default::default()
    }

    /// Adds a connected peer.
    pub fn add_peer(&mut self, peer_id: String) {
        self.connected_peers.insert(peer_id);
    }

    /// Removes a connected peer.
    pub fn remove_peer(&mut self, peer_id: &str) {
        self.connected_peers.remove(peer_id);

        // Remove any pieces being downloaded by this peer
        self.downloading_pieces.retain(|_, pid| pid != peer_id);
    }

    /// Starts downloading a piece.
    pub fn start_piece_download(&mut self, piece_index: PieceIndex, peer_id: String) {
        self.downloading_pieces.insert(piece_index, peer_id);
    }

    /// Completes a piece download.
    pub fn complete_piece(&mut self, piece_index: PieceIndex) {
        self.downloading_pieces.remove(&piece_index);
        self.completed_pieces.insert(piece_index);
    }

    /// Fails a piece download.
    pub fn fail_piece(&mut self, piece_index: PieceIndex, reason: String) {
        self.downloading_pieces.remove(&piece_index);
        self.failed_pieces.insert(piece_index, reason);
    }

    /// Returns number of active downloads.
    pub fn active_downloads(&self) -> usize {
        self.downloading_pieces.len()
    }

    /// Returns download progress as a percentage.
    pub fn download_progress(&self, total_pieces: u32) -> f32 {
        if total_pieces == 0 {
            return 100.0;
        }
        (self.completed_pieces.len() as f32 / total_pieces as f32) * 100.0
    }
}

/// Metrics collected during simulation.
#[derive(Debug, Clone)]
pub struct SimulationMetrics {
    /// Total events processed
    pub events_processed: u64,
    /// Events by type
    pub events_by_type: HashMap<String, u64>,
    /// Average piece download time
    pub avg_piece_download_time: Duration,
    /// Number of piece failures
    pub piece_failures: u64,
    /// Number of peer disconnects
    pub peer_disconnects: u64,
    /// Peak number of connections
    pub peak_connections: usize,
    /// Invariant violations detected
    pub invariant_violations: Vec<InvariantViolation>,
    /// Piece download times for averaging
    piece_download_times: Vec<Duration>,
    /// Start time for duration tracking
    start_time: Instant,
}

impl Default for SimulationMetrics {
    fn default() -> Self {
        Self::new()
    }
}

impl SimulationMetrics {
    /// Creates new metrics collector.
    pub fn new() -> Self {
        Self {
            events_processed: 0,
            events_by_type: HashMap::new(),
            avg_piece_download_time: Duration::ZERO,
            piece_failures: 0,
            peer_disconnects: 0,
            peak_connections: 0,
            invariant_violations: Vec::new(),
            piece_download_times: Vec::new(),
            start_time: Instant::now(),
        }
    }

    /// Records an event being processed.
    pub fn record_event(&mut self, event_type_str: &str) {
        self.events_processed += 1;
        *self
            .events_by_type
            .entry(event_type_str.to_string())
            .or_insert(0) += 1;
    }

    /// Records a piece download completion.
    pub fn record_piece_complete(&mut self, download_time: Duration) {
        self.piece_download_times.push(download_time);

        // Update average
        if !self.piece_download_times.is_empty() {
            let total: Duration = self.piece_download_times.iter().sum();
            self.avg_piece_download_time = total / self.piece_download_times.len() as u32;
        }
    }

    /// Records a piece failure.
    pub fn record_piece_failure(&mut self) {
        self.piece_failures += 1;
    }

    /// Records a peer disconnect.
    pub fn record_peer_disconnect(&mut self) {
        self.peer_disconnects += 1;
    }

    /// Updates peak connection count.
    pub fn update_peak_connections(&mut self, current_connections: usize) {
        if current_connections > self.peak_connections {
            self.peak_connections = current_connections;
        }
    }

    /// Records an invariant violation.
    pub fn record_invariant_violation(&mut self, violation: InvariantViolation) {
        self.invariant_violations.push(violation);
    }

    /// Returns total simulation duration.
    pub fn duration(&self) -> Duration {
        self.start_time.elapsed()
    }

    /// Generates summary statistics.
    pub fn summary(&self) -> String {
        let mut summary = String::new();

        summary.push_str(&format!("Events processed: {}\n", self.events_processed));
        summary.push_str(&format!("Piece failures: {}\n", self.piece_failures));
        summary.push_str(&format!("Peer disconnects: {}\n", self.peer_disconnects));
        summary.push_str(&format!("Peak connections: {}\n", self.peak_connections));
        summary.push_str(&format!(
            "Average piece download time: {:?}\n",
            self.avg_piece_download_time
        ));
        summary.push_str(&format!(
            "Invariant violations: {}\n",
            self.invariant_violations.len()
        ));

        summary
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_simulation_state_peer_management() {
        let mut state = SimulationState::new();

        state.add_peer("PEER1".to_string());
        state.add_peer("PEER2".to_string());
        assert_eq!(state.connected_peers.len(), 2);

        state.remove_peer("PEER1");
        assert_eq!(state.connected_peers.len(), 1);
        assert!(!state.connected_peers.contains("PEER1"));
    }

    #[test]
    fn test_simulation_state_piece_tracking() {
        let mut state = SimulationState::new();

        let piece = PieceIndex::new(5);
        state.start_piece_download(piece, "PEER1".to_string());
        assert_eq!(state.active_downloads(), 1);

        state.complete_piece(piece);
        assert_eq!(state.active_downloads(), 0);
        assert!(state.completed_pieces.contains(&piece));
    }

    #[test]
    fn test_download_progress_calculation() {
        let mut state = SimulationState::new();

        assert_eq!(state.download_progress(0), 100.0);
        assert_eq!(state.download_progress(10), 0.0);

        for i in 0..5 {
            state.complete_piece(PieceIndex::new(i));
        }

        assert_eq!(state.download_progress(10), 50.0);
    }

    #[test]
    fn test_metrics_event_recording() {
        let mut metrics = SimulationMetrics::new();

        metrics.record_event("PeerConnect");
        metrics.record_event("PeerConnect");
        metrics.record_event("PieceRequest");

        assert_eq!(metrics.events_processed, 3);
        assert_eq!(metrics.events_by_type["PeerConnect"], 2);
        assert_eq!(metrics.events_by_type["PieceRequest"], 1);
    }

    #[test]
    fn test_metrics_piece_timing() {
        let mut metrics = SimulationMetrics::new();

        metrics.record_piece_complete(Duration::from_secs(2));
        metrics.record_piece_complete(Duration::from_secs(4));
        metrics.record_piece_complete(Duration::from_secs(6));

        assert_eq!(metrics.avg_piece_download_time, Duration::from_secs(4));
    }
}
