//! Deterministic simulation framework for testing BitTorrent streaming behavior.
//!
//! This module provides controlled, reproducible simulation environments for testing
//! complex streaming scenarios with predictable outcomes.

mod clock;
mod events;
mod invariants;
mod resources;
mod simulation;
mod state;

// Re-export core types for public API
pub use clock::{DeterministicClock, DeterministicRng};
pub use events::{EventPriority, EventType, SimulationEvent, ThrottleDirection};
pub use invariants::{
    BandwidthInvariant, DataIntegrityInvariant, Invariant, InvariantViolation,
    MinimumPeersInvariant, ResourceLimitInvariant, StreamingInvariant,
};
pub use resources::{ResourceLimits, ResourceType, ResourceUsage};
pub use simulation::{DeterministicSimulation, PeerBehavior, SimulationError, SimulationReport};
pub use state::{SimulationMetrics, SimulationState};

#[cfg(test)]
mod tests;
