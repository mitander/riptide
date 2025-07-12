//! Unified torrent engine with actor-based concurrency model.
//!
//! This module provides a high-performance, deadlock-free torrent engine using
//! the actor model for state management. All interactions with the engine happen
//! through message passing via the `TorrentEngineHandle`, eliminating shared
//! state locks and ensuring predictable performance under load.
//!
//! # Architecture
//!
//! The engine consists of several key components:
//!
//! - **Actor**: Single-threaded message processor that manages all engine state
//! - **Handle**: Multi-producer interface for sending commands to the actor
//! - **Commands**: Message protocol defining all possible operations
//! - **Core**: Internal engine implementation with torrent management logic
//!
//! # Usage
//!
//! ```rust,no_run
//! # async fn example() -> Result<(), Box<dyn std::error::Error>> {
//! use riptide_core::engine::spawn_torrent_engine;
//! use riptide_core::config::RiptideConfig;
//! use riptide_core::torrent::{TcpPeers, Tracker};
//!
//! let config = RiptideConfig::default();
//! let peers = TcpPeers::new_default();
//! let tracker = Tracker::new(config.network.clone());
//!
//! // Spawn the engine actor
//! let engine = spawn_torrent_engine(config, peers, tracker);
//!
//! // Use the handle to interact with the engine
//! let info_hash = engine.add_magnet("magnet:?xt=...").await?;
//! engine.start_download(info_hash).await?;
//! # Ok(())
//! # }
//! ```

mod actor;
mod commands;
mod core;
mod handle;

#[cfg(any(test, feature = "test-utils"))]
mod test_mocks;

// Re-export public API
pub use actor::spawn_torrent_engine;
pub use commands::{EngineStats, TorrentSession, TorrentSessionParams};
pub use handle::TorrentEngineHandle;
#[cfg(any(test, feature = "test-utils"))]
pub use test_mocks::{MockPeers, MockTracker};
