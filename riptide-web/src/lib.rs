//! Riptide Web - Modern HTMX + Tailwind Web Interface
//!
//! Server-driven web interface with real-time updates using HTMX.
//! Provides clean, responsive UI for torrent management and media streaming.

pub mod components;
pub mod handlers;
pub mod htmx;
pub mod pages;
pub mod server;
pub mod streaming;
pub mod templates;

// Re-export main types
pub use server::run_server;
