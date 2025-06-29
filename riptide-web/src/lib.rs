//! Riptide Web - Simple Web UI and API server
//!
//! Clean, minimal web interface using vanilla JavaScript,
//! providing modern UI for managing torrents and streaming media.

pub mod server;
pub mod templates;

// Re-export main types
pub use server::run_server;
