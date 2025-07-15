//! Remux streaming module for converting video formats to MP4 for browser compatibility
//!
//! This module provides a robust, state-driven system for remuxing video files
//! from various container formats (AVI, MKV, etc.) to MP4 for streaming.
//!
//! # Architecture
//!
//! The remux system follows a state machine pattern with these key components:
//! - `RemuxSessionCoordinator`: Coordinates remuxing sessions and prevents race conditions
//! - `RemuxSession`: Tracks individual remuxing operations with explicit state
//! - `StreamingStrategy`: Provides the interface for different streaming approaches
//! - `RealtimeRemuxer`: Real-time remuxing pipeline for progressive streaming
//! - `ChunkServer`: Serves remuxed chunks as they're produced
//! - `DownloadManager`: Manages on-demand piece downloading for streaming
//!
//! # State Management
//!
//! Each remuxing session progresses through these states:
//! - `WaitingForHeadAndTail`: Initial state, waiting for sufficient data
//! - `Remuxing`: FFmpeg process is running
//! - `Completed`: Remuxing finished successfully
//! - `Failed`: Remuxing failed, may be retryable

pub mod remuxer;
pub mod state;
pub mod strategy;
pub mod types;

#[cfg(test)]
pub mod readiness_tests;

#[cfg(test)]
pub mod avi_progressive_test;

pub use remuxer::Remuxer;
pub use state::{RemuxError, RemuxProgress, RemuxState};
pub use strategy::{DirectStreamStrategy, RemuxStreamStrategy, StreamingStrategy};
pub use types::{
    ContainerFormat, RemuxConfig, RemuxSession, StrategyError, StreamData, StreamHandle,
    StreamReadiness, StreamingError, StreamingResult, StreamingStatus,
};

// Re-export streaming components for real-time remuxing pipeline
pub use crate::streaming::chunk_server::{
    ChunkBuffer, ChunkRequest, ChunkResponse, ChunkServer, ChunkServerError,
    RemuxedChunk as StreamChunk,
};
pub use crate::streaming::download_manager::{
    DownloadManager, DownloadManagerConfig, DownloadPriority, DownloadRequest, DownloadStats,
};
pub use crate::streaming::realtime_remuxer::{
    RealtimeRemuxer, RemuxHandle, RemuxStatus, RemuxedChunk,
};
