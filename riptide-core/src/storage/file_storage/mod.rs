//! File-based storage implementation
//!
//! Provides filesystem-based storage for torrent pieces with directory organization
//! by info hash. Supports piece verification and torrent completion handling.

pub mod storage;
pub mod tests;
pub mod types;

// Re-export public API
pub use types::FileStorage;
