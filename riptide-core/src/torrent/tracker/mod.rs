//! BitTorrent tracker communication abstractions and implementations.
//!
//! HTTP tracker client following BEP 3 with announce/scrape operations.
//! Supports automatic URL encoding, compact peer list parsing, and error handling.

pub mod client;
pub mod manager;
pub mod protocol;
pub mod simulated;
pub mod types;

// Re-export public API
pub use client::HttpTrackerClient;
pub use manager::{TrackerManagement, TrackerManager};
pub use simulated::{ResponseConfig, SimulatedTrackerClient, SimulatedTrackerManager};
pub use types::{
    AnnounceEvent, AnnounceRequest, AnnounceResponse, ScrapeRequest, ScrapeResponse, ScrapeStats,
    TrackerClient,
};
