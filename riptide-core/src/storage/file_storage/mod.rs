//! File-based storage implementation
//!
//! Provides filesystem-based storage for torrent pieces with directory organization
//! by info hash. Supports piece verification and torrent completion handling.

pub mod operations;
pub mod types;

// Re-export public API
pub use types::FileStorage;
