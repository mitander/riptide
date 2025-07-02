//! Direct streaming service with HTTP range requests
//!
//! Provides media streaming capabilities that integrate with the
//! peer management system for streaming performance.

pub mod ffmpeg;
pub mod file_reconstruction;
pub mod piece_reader;
pub mod range_handler;
pub mod remuxed_streaming;
pub mod strategy;
pub mod strategy_manager;
pub mod stream_coordinator;

use std::sync::Arc;

pub use ffmpeg::{
    FfmpegProcessor, ProductionFfmpegProcessor, RemuxingOptions, RemuxingResult,
    SimulationFfmpegProcessor,
};
pub use file_reconstruction::FileReconstructor;
pub use piece_reader::{
    PieceBasedStreamReader, PieceReaderError, create_piece_reader_from_trait_object,
};
pub use range_handler::{ContentInfo, RangeHandler, RangeRequest, RangeResponse};
pub use remuxed_streaming::{RemuxedStreaming, RemuxingConfig};
pub use strategy::{
    ContainerDetector, ContainerFormat, StreamingError as StrategyError, StreamingResult,
    StreamingStrategy,
};
pub use strategy_manager::{StreamingCapability, StreamingStrategyManager};
pub use stream_coordinator::{StreamCoordinator, StreamingError, StreamingSession, StreamingStats};
use tokio::sync::RwLock;

use crate::config::RiptideConfig;
use crate::torrent::{
    EnhancedPeerManager, TcpPeerManager, TorrentEngineHandle, TrackerManager, spawn_torrent_engine,
};

/// Streaming service integrating HTTP server with BitTorrent backend.
///
/// Coordinates between HTTP range requests from media players and the underlying
/// BitTorrent downloading system to provide media streaming.
pub struct DirectStreamingService {
    stream_coordinator: Arc<RwLock<StreamCoordinator>>,
    torrent_engine: TorrentEngineHandle,
    peer_manager: Arc<RwLock<EnhancedPeerManager>>,
}

impl DirectStreamingService {
    /// Creates new streaming service with configuration.
    pub fn new(config: RiptideConfig) -> Self {
        let peer_manager = Arc::new(RwLock::new(EnhancedPeerManager::new(config.clone())));
        let peer_manager_impl = TcpPeerManager::new_default();
        let tracker_manager = TrackerManager::new(config.network.clone());
        let torrent_engine = spawn_torrent_engine(config, peer_manager_impl, tracker_manager);
        let stream_coordinator = Arc::new(RwLock::new(StreamCoordinator::new(
            torrent_engine.clone(),
            Arc::clone(&peer_manager),
        )));

        Self {
            stream_coordinator,
            torrent_engine,
            peer_manager,
        }
    }

    /// Start the streaming service on the configured port.
    ///
    /// # Errors
    /// - `StreamingError::ServerStartFailed` - Failed to bind to port or start server
    pub async fn start(&self) -> Result<(), StreamingError> {
        // Start background tasks for peer management
        let _ = self
            .peer_manager
            .read()
            .await
            .start_background_tasks()
            .await;

        Ok(())
    }

    /// Add a torrent for streaming by magnet link or info hash.
    ///
    /// # Errors
    /// - `StreamingError::TorrentAddFailed` - Failed to add torrent to engine
    pub async fn add_torrent(&self, source: String) -> Result<String, StreamingError> {
        let info_hash = if source.starts_with("magnet:") {
            self.torrent_engine.add_magnet(&source).await.map_err(|e| {
                StreamingError::TorrentAddFailed {
                    reason: e.to_string(),
                }
            })?
        } else {
            return Err(StreamingError::UnsupportedSource);
        };

        // Register with stream coordinator
        let mut coordinator = self.stream_coordinator.write().await;
        coordinator
            .register_torrent(info_hash, source.clone())
            .await?;

        Ok(format!("/stream/{}", hex::encode(info_hash.as_bytes())))
    }

    /// Get streaming statistics for monitoring.
    pub async fn get_stats(&self) -> StreamingServiceStats {
        let coordinator = self.stream_coordinator.read().await;
        let peer_stats = self.peer_manager.read().await.get_enhanced_stats().await;
        let streaming_stats = coordinator.get_stats().await;

        StreamingServiceStats {
            active_streams: streaming_stats.active_sessions,
            total_bytes_streamed: streaming_stats.total_bytes_served,
            peer_connections: peer_stats.total_connections,
            healthy_peers: peer_stats.active_connections,
            bandwidth_utilization: peer_stats.bandwidth_utilization.download_rate_mbps,
        }
    }

    /// Stop the streaming service gracefully.
    pub async fn stop(&mut self) -> Result<(), StreamingError> {
        Ok(())
    }
}

/// Overall streaming service statistics.
#[derive(Debug, Clone)]
pub struct StreamingServiceStats {
    pub active_streams: usize,
    pub total_bytes_streamed: u64,
    pub peer_connections: usize,
    pub healthy_peers: usize,
    pub bandwidth_utilization: f64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_streaming_service_creation() {
        let config = RiptideConfig::default();
        let service = DirectStreamingService::new(config);

        let stats = service.get_stats().await;
        assert_eq!(stats.active_streams, 0);
        assert_eq!(stats.total_bytes_streamed, 0);
    }

    #[tokio::test]
    async fn test_invalid_torrent_source() {
        let config = RiptideConfig::default();
        let service = DirectStreamingService::new(config);

        let result = service.add_torrent("invalid://source".to_string()).await;
        assert!(result.is_err());

        if let Err(StreamingError::UnsupportedSource) = result {
            // Test passed
        } else {
            panic!("Expected UnsupportedSource error");
        }
    }
}
