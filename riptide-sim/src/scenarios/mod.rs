//! Pre-built simulation scenarios for common BitTorrent testing patterns.
//!
//! Provides ready-to-use simulation scenarios that reproduce specific
//! network conditions and edge cases for systematic testing.

pub mod builders;
pub mod runner;
pub mod types;

// Re-export main types
pub use builders::SimulationScenarios;
pub use runner::{ScenarioResult, ScenarioResults, ScenarioRunner};
pub use types::{StreamingScenario, StressScenario};

// Re-export from existing modules
pub mod streaming_edge_cases;
pub mod streaming_invariants;
