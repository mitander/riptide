//! Edge case scenarios for BitTorrent streaming simulation.
//!
//! Provides focused test scenarios that verify system behavior under
//! challenging network conditions and edge cases.

pub mod streaming_edge_cases;
pub mod streaming_invariants;

// Re-export the essential scenario functions
pub use streaming_edge_cases::{
    cascading_piece_failures_scenario, extreme_peer_churn_scenario, resource_exhaustion_scenario,
    severe_network_degradation_scenario, total_peer_failure_scenario,
};
