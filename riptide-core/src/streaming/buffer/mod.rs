//! Chunk buffer for streaming remuxed media data
//!
//! This module provides efficient buffering and serving of remuxed media chunks
//! for HTTP range requests. It implements an LRU eviction policy and supports
//! real-time streaming with minimal latency.

use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use std::time::{Duration, Instant};

use tokio::sync::{Notify, RwLock};
use tracing::{debug, info, warn};

pub use self::chunk_buffer::ChunkBuffer;
pub use self::stats::BufferStats;
use crate::streaming::realtime_remuxer::RemuxedChunk;

mod chunk_buffer;
mod stats;

/// Error types for chunk buffer operations
#[derive(Debug, thiserror::Error)]
pub enum BufferError {
    /// Buffer has reached capacity and cannot accept more data
    #[error("Buffer capacity exceeded")]
    CapacityExceeded,

    /// Requested range is not available in buffer
    #[error("Range {start}-{end} not available in buffer")]
    RangeNotAvailable { start: u64, end: u64 },

    /// Buffer operation timed out
    #[error("Buffer operation timed out after {timeout_ms}ms")]
    Timeout { timeout_ms: u64 },
}

/// Configuration for chunk buffer behavior
#[derive(Debug, Clone)]
pub struct BufferConfig {
    /// Maximum number of chunks to buffer
    pub max_chunks: usize,

    /// Maximum total bytes to buffer
    pub max_bytes: u64,

    /// Whether to cache header chunks permanently
    pub cache_headers: bool,

    /// Timeout for waiting on new chunks
    pub wait_timeout: Duration,
}

impl Default for BufferConfig {
    fn default() -> Self {
        Self {
            max_chunks: 256,             // 256 chunks
            max_bytes: 64 * 1024 * 1024, // 64MB
            cache_headers: true,
            wait_timeout: Duration::from_secs(30),
        }
    }
}

impl BufferConfig {
    /// Creates configuration optimized for low-latency streaming
    pub fn low_latency() -> Self {
        Self {
            max_chunks: 128,             // 128 chunks
            max_bytes: 32 * 1024 * 1024, // 32MB
            wait_timeout: Duration::from_secs(10),
            ..Default::default()
        }
    }

    /// Creates configuration optimized for high-throughput streaming
    pub fn high_throughput() -> Self {
        Self {
            max_chunks: 512,              // 512 chunks
            max_bytes: 128 * 1024 * 1024, // 128MB
            wait_timeout: Duration::from_secs(60),
            ..Default::default()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_buffer_config_defaults() {
        let config = BufferConfig::default();
        assert_eq!(config.max_chunks, 256);
        assert_eq!(config.max_bytes, 64 * 1024 * 1024);
        assert!(config.cache_headers);
    }

    #[test]
    fn test_low_latency_config() {
        let config = BufferConfig::low_latency();
        assert_eq!(config.max_chunks, 128);
        assert_eq!(config.max_bytes, 32 * 1024 * 1024);
        assert_eq!(config.wait_timeout, Duration::from_secs(10));
    }

    #[test]
    fn test_high_throughput_config() {
        let config = BufferConfig::high_throughput();
        assert_eq!(config.max_chunks, 512);
        assert_eq!(config.max_bytes, 128 * 1024 * 1024);
        assert_eq!(config.wait_timeout, Duration::from_secs(60));
    }
}
