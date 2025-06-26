//! Custom magneto provider for deterministic torrent discovery simulation
//!
//! Provides offline torrent discovery with realistic data for testing BitTorrent
//! functionality without network dependencies or rate limiting.

mod builder;
mod client;
mod content_database;
mod provider;

pub use builder::*;
pub use client::*;
pub use content_database::*;
pub use provider::*;
