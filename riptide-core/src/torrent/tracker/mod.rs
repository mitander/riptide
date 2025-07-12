//! BitTorrent tracker communication abstractions and implementations.
//!
//! HTTP tracker client following BEP 3 with announce/scrape operations.
//! Supports automatic URL encoding, compact peer list parsing, and error handling.

pub mod client;
pub mod manager;
pub mod protocol;
pub mod types;

// Re-export public API
pub use client::HttpTrackerClient;
pub use manager::{Tracker, TrackerManager};
pub use types::{
    AnnounceEvent, AnnounceRequest, AnnounceResponse, ScrapeRequest, ScrapeResponse, ScrapeStats,
    TrackerClient,
};
