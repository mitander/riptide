//! Search service implementations for media discovery.
//!
//! Contains mock and real implementations of torrent search providers,
//! keeping external API integration separate from core logic.

pub mod mock_provider;

pub use mock_provider::{MockMagnetoProvider, MockMagnetoProviderBuilder, TorrentEntryParams};
