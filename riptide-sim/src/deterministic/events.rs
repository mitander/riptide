//! Event types and scheduling for deterministic simulations.

use std::cmp::Ordering;
use std::time::{Duration, Instant};

use riptide_core::torrent::PieceIndex;

/// Priority levels for simulation events.
///
/// Lower numeric values have higher priority when events
/// occur at the same timestamp.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum EventPriority {
    Critical = 0, // Network failures, disconnects
    High = 1,     // Protocol messages
    Normal = 2,   // Regular operations
    Low = 3,      // Background tasks
}

/// Direction of bandwidth throttling.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThrottleDirection {
    Upload,
    Download,
}

/// Types of events that can occur in the simulation.
#[derive(Debug, Clone, PartialEq)]
pub enum EventType {
    /// Peer connects to swarm
    PeerConnect { peer_id: String },
    /// Peer disconnects from swarm
    PeerDisconnect { peer_id: String },
    /// Piece download request initiated
    PieceRequest {
        peer_id: String,
        piece_index: PieceIndex,
    },
    /// Piece download completed successfully
    PieceComplete {
        peer_id: String,
        piece_index: PieceIndex,
        download_time: Duration,
    },
    /// Piece download failed
    PieceFailed {
        peer_id: String,
        piece_index: PieceIndex,
        reason: String,
    },
    /// Tracker announce event
    TrackerAnnounce {
        info_hash: riptide_core::torrent::InfoHash,
        peer_count: usize,
    },
    /// Network conditions change
    NetworkChange {
        latency_ms: u32,
        packet_loss_rate: f64,
    },
    /// Bandwidth throttling applied
    BandwidthThrottle {
        direction: ThrottleDirection,
        rate_bytes_per_sec: u64,
    },
    /// Resource limit reached
    ResourceLimit {
        resource: crate::ResourceType,
        current: u64,
        limit: u64,
    },
    /// Custom test event
    Custom { name: String, data: String },
}

impl EventType {
    /// Returns string representation of event type for metrics.
    pub fn as_str(&self) -> &'static str {
        match self {
            EventType::PeerConnect { .. } => "PeerConnect",
            EventType::PeerDisconnect { .. } => "PeerDisconnect",
            EventType::PieceRequest { .. } => "PieceRequest",
            EventType::PieceComplete { .. } => "PieceComplete",
            EventType::PieceFailed { .. } => "PieceFailed",
            EventType::TrackerAnnounce { .. } => "TrackerAnnounce",
            EventType::NetworkChange { .. } => "NetworkChange",
            EventType::BandwidthThrottle { .. } => "BandwidthThrottle",
            EventType::ResourceLimit { .. } => "ResourceLimit",
            EventType::Custom { .. } => "Custom",
        }
    }
}

/// Simulation event with timestamp and priority.
#[derive(Debug, Clone)]
pub struct SimulationEvent {
    /// Unique event ID for deterministic ordering
    pub id: u64,
    /// Scheduled execution time
    pub timestamp: Instant,
    /// Type of event
    pub event_type: EventType,
    /// Priority for events at same timestamp
    pub priority: EventPriority,
}

impl SimulationEvent {
    /// Creates new simulation event.
    pub fn new(
        id: u64,
        timestamp: Instant,
        event_type: EventType,
        priority: EventPriority,
    ) -> Self {
        Self {
            id,
            timestamp,
            event_type,
            priority,
        }
    }
}

impl Eq for SimulationEvent {}

impl PartialEq for SimulationEvent {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}

impl Ord for SimulationEvent {
    fn cmp(&self, other: &Self) -> Ordering {
        // Earlier timestamp first
        match self.timestamp.cmp(&other.timestamp) {
            Ordering::Equal => {
                // Higher priority first (lower numeric value)
                match self.priority.cmp(&other.priority) {
                    Ordering::Equal => {
                        // Deterministic by ID for reproducibility
                        self.id.cmp(&other.id)
                    }
                    other => other,
                }
            }
            other => other.reverse(), // Reverse for min-heap behavior
        }
    }
}

impl PartialOrd for SimulationEvent {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_event_priority_ordering() {
        let now = Instant::now();

        let critical = SimulationEvent::new(
            1,
            now,
            EventType::PeerDisconnect {
                peer_id: "A".to_string(),
            },
            EventPriority::Critical,
        );

        let normal = SimulationEvent::new(
            2,
            now,
            EventType::PeerConnect {
                peer_id: "B".to_string(),
            },
            EventPriority::Normal,
        );

        // Critical priority should come first when timestamps are equal
        assert!(critical < normal); // Remember: reversed for min-heap
    }

    #[test]
    fn test_event_timestamp_ordering() {
        let now = Instant::now();

        let early = SimulationEvent::new(
            1,
            now,
            EventType::PeerConnect {
                peer_id: "A".to_string(),
            },
            EventPriority::Low,
        );

        let late = SimulationEvent::new(
            2,
            now + Duration::from_secs(1),
            EventType::PeerConnect {
                peer_id: "B".to_string(),
            },
            EventPriority::Critical,
        );

        // Earlier timestamp should come first, regardless of priority
        // Note: Ord is reversed for min-heap behavior, so early > late
        assert!(early > late);
    }

    #[test]
    fn test_event_type_string_conversion() {
        let event = EventType::PieceComplete {
            peer_id: "TEST".to_string(),
            piece_index: PieceIndex::new(0),
            download_time: Duration::from_secs(1),
        };

        assert_eq!(event.as_str(), "PieceComplete");
    }
}
