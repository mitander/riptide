//! Web streaming services and HTTP handlers
//!
//! Provides HTTP streaming functionality that integrates with riptide-core's
//! FileAssembler and TranscodingService components.

pub mod http_streaming;

pub use http_streaming::{
    ClientCapabilities, HttpStreamingConfig, HttpStreamingError, HttpStreamingService,
    HttpStreamingStats, SimpleRangeRequest, StreamingRequest, StreamingResponse,
};
