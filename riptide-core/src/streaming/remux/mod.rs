//! Remux streaming module for converting video formats to MP4 for browser compatibility
//!
//! This module provides a robust, state-driven system for remuxing video files
//! from various container formats (AVI, MKV, etc.) to MP4 for streaming.
//!
//! # Architecture
//!
//! The remux system follows a state machine pattern with these key components:
//! - `RemuxSessionManager`: Coordinates remuxing sessions and prevents race conditions
//! - `RemuxSession`: Tracks individual remuxing operations with explicit state
//! - `StreamingStrategy`: Provides the interface for different streaming approaches
//!
//! # State Management
//!
//! Each remuxing session progresses through these states:
//! - `WaitingForHeadAndTail`: Initial state, waiting for sufficient data
//! - `Remuxing`: FFmpeg process is running
//! - `Completed`: Remuxing finished successfully
//! - `Failed`: Remuxing failed, may be retryable

pub mod session_manager;
pub mod state;
pub mod strategy;
pub mod types;

#[cfg(test)]
pub mod readiness_tests;

pub use session_manager::RemuxSessionManager;
pub use state::{RemuxError, RemuxProgress, RemuxState};
pub use strategy::{DirectStreamStrategy, RemuxStreamStrategy, StreamingStrategy};
pub use types::{
    ContainerFormat, RemuxConfig, RemuxSession, StrategyError, StreamData, StreamHandle,
    StreamReadiness, StreamingError, StreamingResult, StreamingStatus,
};
