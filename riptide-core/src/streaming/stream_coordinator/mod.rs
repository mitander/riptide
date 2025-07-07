//! Stream coordinator for managing torrent-based streaming sessions
//!
//! Coordinates between HTTP requests and BitTorrent downloading to provide
//! media streaming with buffering and piece prioritization.

pub use types::{
    ActiveRange, StreamCoordinator, StreamingBufferState, StreamingError,
    StreamingPerformanceMetrics, StreamingSession, StreamingStats, TorrentMetadata,
};

mod coordinator;
mod session;
mod types;

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use tokio::sync::RwLock;

    use super::*;
    use crate::config::RiptideConfig;
    use crate::torrent::test_data::create_test_info_hash;
    use crate::torrent::{
        EnhancedPeerManager, TcpPeerManager, TrackerManager, spawn_torrent_engine,
    };

    fn create_test_coordinator() -> StreamCoordinator {
        let config = RiptideConfig::default();
        let peer_manager_impl = TcpPeerManager::new_default();
        let tracker_manager = TrackerManager::new(config.network.clone());
        let torrent_engine =
            spawn_torrent_engine(config.clone(), peer_manager_impl, tracker_manager);
        let peer_manager = Arc::new(RwLock::new(EnhancedPeerManager::new(config)));

        StreamCoordinator::new(torrent_engine, peer_manager)
    }

    #[tokio::test]
    async fn test_stream_coordinator_creation() {
        let coordinator = create_test_coordinator();
        let stats = coordinator.statistics().await;

        assert_eq!(stats.active_sessions, 0);
        assert_eq!(stats.total_torrents, 0);
    }

    #[tokio::test]
    async fn test_torrent_registration() {
        let mut coordinator = create_test_coordinator();
        let info_hash = create_test_info_hash();

        let result = coordinator
            .register_torrent(info_hash, "magnet:?xt=test".to_string())
            .await;

        assert!(result.is_ok());

        let content_info = coordinator
            .content_info(*info_hash.as_bytes())
            .await
            .unwrap();
        assert_eq!(content_info.name, "Sample Torrent");
        assert_eq!(content_info.total_size, 1_000_000_000);
    }

    #[tokio::test]
    async fn test_file_info_retrieval() {
        let mut coordinator = create_test_coordinator();
        let info_hash = create_test_info_hash();

        coordinator
            .register_torrent(info_hash, "magnet:?xt=test".to_string())
            .await
            .unwrap();

        let file_info = coordinator
            .file_info(*info_hash.as_bytes(), 0)
            .await
            .unwrap();

        assert_eq!(file_info.name, "sample_movie.mp4");
        assert!(file_info.is_streamable());
    }

    #[tokio::test]
    async fn test_range_reading() {
        let mut coordinator = create_test_coordinator();
        let info_hash = create_test_info_hash();

        coordinator
            .register_torrent(info_hash, "magnet:?xt=test".to_string())
            .await
            .unwrap();

        let read_data = coordinator
            .read_range(*info_hash.as_bytes(), 0, 1024)
            .await
            .unwrap();

        assert_eq!(read_data.len(), 1024);
    }

    #[tokio::test]
    async fn test_streaming_session_creation() {
        let info_hash = create_test_info_hash();
        let session = StreamingSession::new(info_hash, 1_000_000_000, 0);

        assert_eq!(session.info_hash, info_hash);
        assert_eq!(session.total_size, 1_000_000_000);
        assert_eq!(session.current_position, 0);
        assert_eq!(session.buffer_state.buffer_health, 0.0);
    }
}
