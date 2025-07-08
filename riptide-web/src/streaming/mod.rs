//! Web streaming services and HTTP handlers
//!
//! Provides HTTP streaming functionality that integrates with riptide-core's
//! FileAssembler and FFmpeg remuxing components.

pub mod browser_compatibility;
pub mod http_streaming;

pub use browser_compatibility::{BrowserCapabilityMatrix, BrowserCompatibilityTester, BrowserType};
pub use http_streaming::{
    ClientCapabilities, HttpStreamingConfig, HttpStreamingError, HttpStreamingService,
    SimpleRangeRequest, StreamingPerformanceMetrics, StreamingRequest, StreamingResponse,
    StreamingStrategy,
};
