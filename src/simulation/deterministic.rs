//! Deterministic simulation framework for reproducible BitTorrent testing
//!
//! Provides controlled, deterministic execution for finding and reproducing bugs.
//! Inspired by TigerBeetle's deterministic simulation approach.

use std::collections::{BTreeMap, VecDeque};
use std::time::{Duration, Instant};

use rand::{Rng, SeedableRng};
use rand_chacha::ChaCha8Rng;

use super::{MockPeer, NetworkSimulator};
use crate::torrent::PieceIndex;

/// Deterministic clock for simulation time control.
///
/// Provides controlled time advancement for reproducible test scenarios.
/// All time-dependent operations should use this clock during simulation.
#[derive(Debug, Clone)]
pub struct DeterministicClock {
    current_time: Instant,
    start_time: Instant,
}

impl Default for DeterministicClock {
    fn default() -> Self {
        Self::new()
    }
}

impl DeterministicClock {
    /// Creates new deterministic clock starting at simulation time zero.
    pub fn new() -> Self {
        let start = Instant::now();
        Self {
            current_time: start,
            start_time: start,
        }
    }

    /// Returns current simulation time.
    pub fn now(&self) -> Instant {
        self.current_time
    }

    /// Returns elapsed simulation time since start.
    pub fn elapsed(&self) -> Duration {
        self.current_time.duration_since(self.start_time)
    }

    /// Advances simulation time by specified duration.
    pub fn advance(&mut self, duration: Duration) {
        self.current_time += duration;
    }

    /// Advances to specific simulation time.
    pub fn advance_to(&mut self, target_time: Instant) {
        if target_time > self.current_time {
            self.current_time = target_time;
        }
    }
}

/// Scheduled event in deterministic simulation.
///
/// Events are processed in chronological order to ensure deterministic
/// execution regardless of timing variations.
#[derive(Debug, Clone)]
pub struct SimulationEvent {
    pub timestamp: Instant,
    pub event_type: EventType,
    pub peer_id: Option<String>,
    pub piece_index: Option<PieceIndex>,
}

/// Types of events that can occur during simulation.
#[derive(Debug, Clone, PartialEq)]
pub enum EventType {
    /// Peer connects to swarm
    PeerConnect,
    /// Peer disconnects from swarm
    PeerDisconnect,
    /// Piece download request initiated
    PieceRequest,
    /// Piece download completed successfully
    PieceComplete,
    /// Piece download failed
    PieceFailed,
    /// Network condition change (latency, bandwidth)
    NetworkChange,
    /// Tracker announce performed
    TrackerAnnounce,
}

/// Deterministic random number generator for reproducible behavior.
///
/// Uses ChaCha8 algorithm for cryptographically secure but deterministic
/// random number generation from fixed seeds.
pub struct DeterministicRng {
    rng: ChaCha8Rng,
    seed: u64,
}

impl DeterministicRng {
    /// Creates deterministic RNG from seed value.
    pub fn from_seed(seed: u64) -> Self {
        Self {
            rng: ChaCha8Rng::seed_from_u64(seed),
            seed,
        }
    }

    /// Returns the seed used for this RNG.
    pub fn seed(&self) -> u64 {
        self.seed
    }

    /// Generates random value in range.
    pub fn gen_range<T, R>(&mut self, range: R) -> T
    where
        T: rand::distributions::uniform::SampleUniform,
        R: rand::distributions::uniform::SampleRange<T>,
    {
        self.rng.gen_range(range)
    }

    /// Generates random boolean with given probability.
    pub fn gen_bool(&mut self, probability: f64) -> bool {
        self.rng.gen_bool(probability)
    }

    /// Generates random duration within range.
    pub fn gen_duration(&mut self, min: Duration, max: Duration) -> Duration {
        let min_ms = min.as_millis() as u64;
        let max_ms = max.as_millis() as u64;
        let random_ms = self.gen_range(min_ms..=max_ms);
        Duration::from_millis(random_ms)
    }
}

/// Complete deterministic simulation environment.
///
/// Provides reproducible testing environment with controlled time,
/// deterministic randomness, and event scheduling.
pub struct DeterministicSimulation {
    clock: DeterministicClock,
    rng: DeterministicRng,
    event_queue: BTreeMap<Instant, VecDeque<SimulationEvent>>,
    network: NetworkSimulator,
    peers: Vec<MockPeer>,
    seed: u64,
}

impl DeterministicSimulation {
    /// Creates new deterministic simulation with given seed.
    ///
    /// Same seed always produces identical simulation behavior for
    /// reproducible bug testing and performance validation.
    pub fn from_seed(seed: u64) -> Self {
        Self {
            clock: DeterministicClock::new(),
            rng: DeterministicRng::from_seed(seed),
            event_queue: BTreeMap::new(),
            network: NetworkSimulator::new(),
            peers: Vec::new(),
            seed,
        }
    }

    /// Returns simulation seed for reproduction.
    pub fn seed(&self) -> u64 {
        self.seed
    }

    /// Returns current simulation clock.
    pub fn clock(&self) -> &DeterministicClock {
        &self.clock
    }

    /// Returns mutable reference to deterministic RNG.
    pub fn rng(&mut self) -> &mut DeterministicRng {
        &mut self.rng
    }

    /// Schedules event at specific simulation time.
    pub fn schedule_event(&mut self, event: SimulationEvent) {
        self.event_queue
            .entry(event.timestamp)
            .or_insert_with(VecDeque::new)
            .push_back(event);
    }

    /// Schedules event after delay from current time.
    pub fn schedule_delayed(&mut self, delay: Duration, event_type: EventType) {
        let timestamp = self.clock.now() + delay;
        let event = SimulationEvent {
            timestamp,
            event_type,
            peer_id: None,
            piece_index: None,
        };
        self.schedule_event(event);
    }

    /// Processes all events up to target time.
    ///
    /// Returns list of events processed for analysis.
    pub fn run_until(&mut self, target_time: Instant) -> Vec<SimulationEvent> {
        let mut processed_events = Vec::new();

        while let Some((&event_time, _)) = self.event_queue.first_key_value() {
            if event_time > target_time {
                break;
            }

            if let Some(mut events_at_time) = self.event_queue.remove(&event_time) {
                self.clock.advance_to(event_time);

                while let Some(event) = events_at_time.pop_front() {
                    self.process_event(&event);
                    processed_events.push(event);
                }
            }
        }

        self.clock.advance_to(target_time);
        processed_events
    }

    /// Runs simulation for specified duration.
    pub fn run_for(&mut self, duration: Duration) -> Vec<SimulationEvent> {
        let target_time = self.clock.now() + duration;
        self.run_until(target_time)
    }

    /// Adds deterministic peer with controlled behavior.
    pub fn add_deterministic_peer(&mut self, peer_id: String, upload_speed: u64) {
        let latency = self
            .rng
            .gen_duration(Duration::from_millis(10), Duration::from_millis(200));

        let reliability = 0.9 + (self.rng.gen_range(0..10) as f32 * 0.01);

        let peer = MockPeer::builder()
            .upload_speed(upload_speed)
            .reliability(reliability)
            .latency(latency)
            .peer_id(peer_id.clone())
            .build();

        self.peers.push(peer);

        // Schedule initial connection
        let connect_delay = self
            .rng
            .gen_duration(Duration::from_millis(100), Duration::from_secs(5));

        let connect_event = SimulationEvent {
            timestamp: self.clock.now() + connect_delay,
            event_type: EventType::PeerConnect,
            peer_id: Some(peer_id),
            piece_index: None,
        };

        self.schedule_event(connect_event);
    }

    /// Simulates piece download scenario with controlled timing.
    pub fn simulate_piece_download(&mut self, piece_index: PieceIndex, piece_size: u32) {
        let request_event = SimulationEvent {
            timestamp: self.clock.now(),
            event_type: EventType::PieceRequest,
            peer_id: None,
            piece_index: Some(piece_index),
        };

        // Calculate download time based on network conditions
        let download_time = self.calculate_download_time(piece_size);

        let completion_event = SimulationEvent {
            timestamp: self.clock.now() + download_time,
            event_type: if self.rng.gen_bool(0.95) {
                EventType::PieceComplete
            } else {
                EventType::PieceFailed
            },
            peer_id: None,
            piece_index: Some(piece_index),
        };

        self.schedule_event(request_event);
        self.schedule_event(completion_event);
    }

    /// Creates streaming scenario with sequential piece requests.
    pub fn create_streaming_scenario(&mut self, total_pieces: u32, start_delay: Duration) {
        for piece_idx in 0..total_pieces {
            let request_time =
                self.clock.now() + start_delay + Duration::from_millis(piece_idx as u64 * 100);

            let request_event = SimulationEvent {
                timestamp: request_time,
                event_type: EventType::PieceRequest,
                peer_id: None,
                piece_index: Some(PieceIndex::new(piece_idx)),
            };

            self.schedule_event(request_event);
        }
    }

    /// Calculates deterministic download time based on piece size.
    fn calculate_download_time(&mut self, piece_size: u32) -> Duration {
        // Base download speed with some variation
        let base_speed = 1_000_000; // 1 MB/s
        let speed_variation = self.rng.gen_range(0.8..1.2);
        let effective_speed = (base_speed as f64 * speed_variation) as u64;

        let download_ms = (piece_size as u64 * 1000) / effective_speed;
        Duration::from_millis(download_ms)
    }

    /// Processes individual simulation event.
    fn process_event(&mut self, event: &SimulationEvent) {
        match event.event_type {
            EventType::PeerConnect => {
                // Peer connection logic - schedule completion events
                if let Some(peer_id) = &event.peer_id {
                    let completion_delay = self
                        .rng
                        .gen_duration(Duration::from_millis(50), Duration::from_millis(500));

                    // Peer successfully connects after handshake delay
                    let connect_complete = SimulationEvent {
                        timestamp: self.clock.now() + completion_delay,
                        event_type: EventType::PeerConnect,
                        peer_id: Some(peer_id.clone()),
                        piece_index: None,
                    };

                    // Don't re-schedule to avoid infinite loop
                    // Instead, mark peer as available for requests
                }
            }
            EventType::PeerDisconnect => {
                // Peer disconnection logic - clean up any pending requests
            }
            EventType::PieceRequest => {
                // Piece request initiation - schedule realistic completion
                if let Some(piece_idx) = event.piece_index {
                    let piece_size = 262144; // 256KB default piece size
                    let download_time = self.calculate_download_time(piece_size);

                    // Add some randomness for realistic behavior
                    let jitter = self
                        .rng
                        .gen_duration(Duration::from_millis(0), Duration::from_millis(100));

                    let completion_event = SimulationEvent {
                        timestamp: self.clock.now() + download_time + jitter,
                        event_type: if self.rng.gen_bool(0.95) {
                            EventType::PieceComplete
                        } else {
                            EventType::PieceFailed
                        },
                        peer_id: event.peer_id.clone(),
                        piece_index: Some(piece_idx),
                    };

                    self.schedule_event(completion_event);
                }
            }
            EventType::PieceComplete => {
                // Piece download completion - no additional action needed
            }
            EventType::PieceFailed => {
                // Piece download failure handling - schedule retry
                if let Some(piece_idx) = event.piece_index {
                    let retry_delay = self
                        .rng
                        .gen_duration(Duration::from_secs(1), Duration::from_secs(5));

                    let retry_event = SimulationEvent {
                        timestamp: self.clock.now() + retry_delay,
                        event_type: EventType::PieceRequest,
                        peer_id: None,
                        piece_index: Some(piece_idx),
                    };

                    self.schedule_event(retry_event);
                }
            }
            EventType::NetworkChange => {
                // Network condition updates - simulate buffering check
            }
            EventType::TrackerAnnounce => {
                // Tracker communication - schedule peer discovery
                let peer_discovery_delay = Duration::from_secs(2);

                // Add new peer after tracker announce
                let new_peer_count = self.rng.gen_range(1..=3);
                for i in 0..new_peer_count {
                    let peer_id = format!("TRACKER_PEER_{}", self.rng.gen_range(1000..9999));
                    let connect_delay = Duration::from_millis(500 + i * 200);

                    let peer_connect = SimulationEvent {
                        timestamp: self.clock.now() + peer_discovery_delay + connect_delay,
                        event_type: EventType::PeerConnect,
                        peer_id: Some(peer_id.clone()),
                        piece_index: None,
                    };

                    self.schedule_event(peer_connect);

                    // Add to peers list
                    let upload_speed = self.rng.gen_range(500_000..5_000_000);
                    let reliability = 0.8 + (self.rng.gen_range(0..20) as f32 * 0.01);
                    let latency = self
                        .rng
                        .gen_duration(Duration::from_millis(20), Duration::from_millis(150));

                    let peer = MockPeer::builder()
                        .upload_speed(upload_speed)
                        .reliability(reliability)
                        .latency(latency)
                        .peer_id(peer_id)
                        .build();

                    self.peers.push(peer);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_deterministic_clock_advancement() {
        let mut clock = DeterministicClock::new();
        let start_time = clock.now();

        clock.advance(Duration::from_secs(10));
        assert_eq!(clock.elapsed(), Duration::from_secs(10));

        clock.advance(Duration::from_secs(5));
        assert_eq!(clock.elapsed(), Duration::from_secs(15));

        assert!(clock.now() > start_time);
    }

    #[test]
    fn test_deterministic_rng_reproducibility() {
        let mut rng1 = DeterministicRng::from_seed(12345);
        let mut rng2 = DeterministicRng::from_seed(12345);

        // Same seed should produce identical sequences
        for _ in 0..100 {
            assert_eq!(rng1.gen_range(0..1000), rng2.gen_range(0..1000));
        }
    }

    #[test]
    fn test_simulation_event_scheduling() {
        let mut sim = DeterministicSimulation::from_seed(42);
        let _start_time = sim.clock.now();

        // Schedule events at different times
        sim.schedule_delayed(Duration::from_secs(1), EventType::PeerConnect);
        sim.schedule_delayed(Duration::from_secs(3), EventType::PieceRequest);
        sim.schedule_delayed(Duration::from_secs(2), EventType::PeerDisconnect);

        // Run simulation and verify event order
        let events = sim.run_for(Duration::from_secs(5));
        assert_eq!(events.len(), 3);

        // Events should be processed in chronological order
        assert_eq!(events[0].event_type, EventType::PeerConnect);
        assert_eq!(events[1].event_type, EventType::PeerDisconnect);
        assert_eq!(events[2].event_type, EventType::PieceRequest);
    }

    #[test]
    fn test_simulation_reproducibility() {
        let seed = 98765;

        // Run same simulation twice
        let mut sim1 = DeterministicSimulation::from_seed(seed);
        sim1.create_streaming_scenario(5, Duration::from_secs(1));
        let events1 = sim1.run_for(Duration::from_secs(10));

        let mut sim2 = DeterministicSimulation::from_seed(seed);
        sim2.create_streaming_scenario(5, Duration::from_secs(1));
        let events2 = sim2.run_for(Duration::from_secs(10));

        // Results should be identical
        assert_eq!(events1.len(), events2.len());
        for (e1, e2) in events1.iter().zip(events2.iter()) {
            assert_eq!(e1.event_type, e2.event_type);
            assert_eq!(e1.piece_index, e2.piece_index);
        }
    }

    #[test]
    fn test_piece_download_simulation() {
        let mut sim = DeterministicSimulation::from_seed(555);
        let piece_index = PieceIndex::new(0);
        let piece_size = 262144; // 256 KB

        sim.simulate_piece_download(piece_index, piece_size);
        let events = sim.run_for(Duration::from_secs(30));

        // Should have request and completion events
        assert!(events.len() >= 2);
        assert_eq!(events[0].event_type, EventType::PieceRequest);
        assert!(matches!(
            events[1].event_type,
            EventType::PieceComplete | EventType::PieceFailed
        ));
    }
}
