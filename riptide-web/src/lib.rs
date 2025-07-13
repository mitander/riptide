//! Riptide Web - JSON API Server

#![warn(missing_docs)]
#![warn(clippy::missing_errors_doc)]
#![deny(clippy::missing_panics_doc)]
#![warn(clippy::too_many_lines)]
//!
//! Pure JSON API server for torrent management and media streaming.
//! Provides RESTful endpoints for frontend applications and external clients.

pub mod handlers;
pub mod server;
pub mod streaming;

// Re-export main types
pub use server::run_server;
