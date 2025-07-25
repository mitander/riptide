//! Web streaming utilities
//!
//! Provides web-specific streaming functionality like browser compatibility detection.
//! All core streaming logic is now handled by riptide-core HttpStreaming.

pub mod browser_compatibility;
pub mod debug;

pub use browser_compatibility::{BrowserCapabilityMatrix, BrowserCompatibilityTester, BrowserType};
pub use debug::{
    CacheFileInfo, DebugStreaming, StreamingDebugEndpoint, StreamingDebugInfo, SystemDebugInfo,
    TorrentDebugInfo,
};
