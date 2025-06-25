//! Web request handlers for the Riptide UI

use std::sync::Arc;

use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;

use super::WebUIError;
use crate::media_search::{MediaSearchResult, MediaSearchService, TorrentResult};
use crate::streaming::DirectStreamingService;
use crate::torrent::TorrentEngine;

/// Web request handlers providing data for UI pages and API endpoints.
#[derive(Clone)]
pub struct WebHandlers {
    torrent_engine: Arc<RwLock<TorrentEngine>>,
    streaming_service: Arc<RwLock<DirectStreamingService>>,
    media_search_service: MediaSearchService,
}

impl WebHandlers {
    /// Creates new web handlers with engine references.
    pub fn new(
        torrent_engine: Arc<RwLock<TorrentEngine>>,
        streaming_service: Arc<RwLock<DirectStreamingService>>,
    ) -> Self {
        Self {
            torrent_engine,
            streaming_service,
            media_search_service: MediaSearchService::new(),
        }
    }

    /// Creates new web handlers with custom media search service.
    pub fn new_with_media_search(
        torrent_engine: Arc<RwLock<TorrentEngine>>,
        streaming_service: Arc<RwLock<DirectStreamingService>>,
        media_search_service: MediaSearchService,
    ) -> Self {
        Self {
            torrent_engine,
            streaming_service,
            media_search_service,
        }
    }

    /// Get server statistics for dashboard.
    pub async fn get_server_stats(&self) -> Result<ServerStats, WebUIError> {
        let engine = self.torrent_engine.read().await;
        let streaming = self.streaming_service.read().await;

        let engine_stats = engine.get_download_stats().await;
        let streaming_stats = streaming.get_stats().await;

        Ok(ServerStats {
            total_torrents: engine_stats.active_torrents,
            active_downloads: engine_stats.active_torrents,
            active_streams: streaming_stats.active_streams,
            total_bytes_downloaded: engine_stats.bytes_downloaded,
            total_bytes_streamed: streaming_stats.total_bytes_streamed,
            download_speed: 0, // TODO: Calculate from engine stats
            upload_speed: 0,   // TODO: Calculate from engine stats
            peer_connections: streaming_stats.peer_connections,
            healthy_peers: streaming_stats.healthy_peers,
            uptime: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
        })
    }

    /// Get recent server activity for dashboard.
    pub async fn get_recent_activity(&self) -> Result<Vec<ActivityItem>, WebUIError> {
        // For now, return mock activity data
        // In production, this would track actual events
        Ok(vec![
            ActivityItem {
                timestamp: chrono::Utc::now(),
                activity_type: ActivityType::TorrentAdded,
                description: "Added new torrent: Movie.2024.1080p.mp4".to_string(),
                details: Some("Downloaded from magnet link".to_string()),
            },
            ActivityItem {
                timestamp: chrono::Utc::now() - chrono::Duration::minutes(15),
                activity_type: ActivityType::StreamStarted,
                description: "Started streaming: Movie.2024.1080p.mp4".to_string(),
                details: Some("Client: 192.168.1.100".to_string()),
            },
            ActivityItem {
                timestamp: chrono::Utc::now() - chrono::Duration::hours(1),
                activity_type: ActivityType::DownloadCompleted,
                description: "Download completed: Series.S01E01.1080p.mp4".to_string(),
                details: Some("Size: 1.2 GB".to_string()),
            },
        ])
    }

    /// Get library items for library browsing.
    pub async fn get_library_items(&self) -> Result<Vec<LibraryItem>, WebUIError> {
        // For now, return mock library data
        // In production, this would scan the actual media library
        Ok(vec![
            LibraryItem {
                id: "movie_001".to_string(),
                title: "Big Buck Bunny".to_string(),
                media_type: MediaType::Movie,
                file_path: "/media/movies/Big.Buck.Bunny.2008.1080p.mp4".to_string(),
                size: 1_500_000_000,
                duration: Some(596), // 9:56 in seconds
                thumbnail_url: Some("/static/thumbnails/big_buck_bunny.jpg".to_string()),
                stream_url: "/stream/abc123def456".to_string(),
                added_at: chrono::Utc::now() - chrono::Duration::days(7),
            },
            LibraryItem {
                id: "series_001".to_string(),
                title: "Open Source Series S01E01".to_string(),
                media_type: MediaType::TvShow,
                file_path: "/media/tv/Open.Source.Series.S01E01.1080p.mp4".to_string(),
                size: 800_000_000,
                duration: Some(2700), // 45 minutes
                thumbnail_url: None,
                stream_url: "/stream/def456abc123".to_string(),
                added_at: chrono::Utc::now() - chrono::Duration::days(2),
            },
        ])
    }

    /// Get torrent list for torrent management.
    pub async fn get_torrent_list(&self) -> Result<Vec<TorrentInfo>, WebUIError> {
        // For now, return mock torrent data
        // In production, this would query the actual torrent engine
        Ok(vec![
            TorrentInfo {
                info_hash: "abc123def456789".to_string(),
                name: "Big.Buck.Bunny.2008.1080p.mp4".to_string(),
                status: TorrentStatus::Seeding,
                progress: 100.0,
                download_speed: 0,
                upload_speed: 512_000, // 512 KB/s
                size: 1_500_000_000,
                downloaded: 1_500_000_000,
                uploaded: 3_000_000_000,
                ratio: 2.0,
                peers: 5,
                seeds: 12,
                eta: None,
                added_at: chrono::Utc::now() - chrono::Duration::days(7),
            },
            TorrentInfo {
                info_hash: "def456abc123789".to_string(),
                name: "Open.Source.Series.S01E01.1080p.mp4".to_string(),
                status: TorrentStatus::Downloading,
                progress: 65.2,
                download_speed: 2_048_000, // 2 MB/s
                upload_speed: 256_000,     // 256 KB/s
                size: 800_000_000,
                downloaded: 521_600_000,
                uploaded: 104_320_000,
                ratio: 0.2,
                peers: 8,
                seeds: 3,
                eta: Some(std::time::Duration::from_secs(540)), // 9 minutes
                added_at: chrono::Utc::now() - chrono::Duration::hours(3),
            },
        ])
    }

    /// Add a new torrent by magnet link.
    pub async fn add_torrent(&self, magnet_link: &str) -> Result<AddTorrentResult, WebUIError> {
        let streaming = self.streaming_service.write().await;

        match streaming.add_torrent(magnet_link.to_string()).await {
            Ok(stream_url) => Ok(AddTorrentResult {
                success: true,
                message: "Torrent added successfully".to_string(),
                stream_url: Some(stream_url),
                info_hash: None, // Would extract from engine in production
            }),
            Err(e) => Ok(AddTorrentResult {
                success: false,
                message: format!("Failed to add torrent: {e}"),
                stream_url: None,
                info_hash: None,
            }),
        }
    }

    /// Get server settings for configuration page.
    pub async fn get_server_settings(&self) -> Result<ServerSettings, WebUIError> {
        Ok(ServerSettings {
            download_limit: None,
            upload_limit: None,
            max_connections: 50,
            streaming_port: 8080,
            web_ui_port: 3000,
            download_directory: "/media/downloads".to_string(),
            enable_upnp: true,
            enable_dht: true,
            enable_pex: true,
        })
    }

    /// Search for media using query string.
    ///
    /// # Errors
    /// - `WebUIError::InternalError` - Failed to perform search
    pub async fn search_media(&self, query: &str) -> Result<Vec<MediaSearchResult>, WebUIError> {
        self.media_search_service
            .search_all(query)
            .await
            .map_err(|e| WebUIError::InternalError {
                reason: format!("Media search failed: {e}"),
            })
    }

    /// Search for movies using query string.
    ///
    /// # Errors
    /// - `WebUIError::InternalError` - Failed to perform search
    pub async fn search_movies(&self, query: &str) -> Result<Vec<MediaSearchResult>, WebUIError> {
        self.media_search_service
            .search_movies(query)
            .await
            .map_err(|e| WebUIError::InternalError {
                reason: format!("Movie search failed: {e}"),
            })
    }

    /// Search for TV shows using query string.
    ///
    /// # Errors
    /// - `WebUIError::InternalError` - Failed to perform search
    pub async fn search_tv_shows(&self, query: &str) -> Result<Vec<MediaSearchResult>, WebUIError> {
        self.media_search_service
            .search_tv_shows(query)
            .await
            .map_err(|e| WebUIError::InternalError {
                reason: format!("TV search failed: {e}"),
            })
    }

    /// Get detailed torrent results for media.
    ///
    /// # Errors
    /// - `WebUIError::InternalError` - Failed to retrieve torrent details
    pub async fn get_media_torrents(
        &self,
        media_title: &str,
    ) -> Result<Vec<TorrentResult>, WebUIError> {
        self.media_search_service
            .get_media_torrents(media_title)
            .await
            .map_err(|e| WebUIError::InternalError {
                reason: format!("Failed to get torrents: {e}"),
            })
    }
}

/// Server statistics for dashboard.
#[derive(Debug, Clone, Serialize)]
pub struct ServerStats {
    pub total_torrents: usize,
    pub active_downloads: usize,
    pub active_streams: usize,
    pub total_bytes_downloaded: u64,
    pub total_bytes_streamed: u64,
    pub download_speed: u64,
    pub upload_speed: u64,
    pub peer_connections: usize,
    pub healthy_peers: usize,
    pub uptime: u64,
}

/// Recent activity item for dashboard.
#[derive(Debug, Clone, Serialize)]
pub struct ActivityItem {
    pub timestamp: chrono::DateTime<chrono::Utc>,
    pub activity_type: ActivityType,
    pub description: String,
    pub details: Option<String>,
}

/// Types of server activities.
#[derive(Debug, Clone, Serialize)]
pub enum ActivityType {
    TorrentAdded,
    TorrentRemoved,
    DownloadStarted,
    DownloadCompleted,
    StreamStarted,
    StreamEnded,
    PeerConnected,
    PeerDisconnected,
}

/// Library item for media browsing.
#[derive(Debug, Clone, Serialize)]
pub struct LibraryItem {
    pub id: String,
    pub title: String,
    pub media_type: MediaType,
    pub file_path: String,
    pub size: u64,
    pub duration: Option<u32>, // Duration in seconds
    pub thumbnail_url: Option<String>,
    pub stream_url: String,
    pub added_at: chrono::DateTime<chrono::Utc>,
}

/// Media file types.
#[derive(Debug, Clone, Serialize)]
pub enum MediaType {
    Movie,
    TvShow,
    Music,
    Other,
}

/// Torrent information for management interface.
#[derive(Debug, Clone, Serialize)]
pub struct TorrentInfo {
    pub info_hash: String,
    pub name: String,
    pub status: TorrentStatus,
    pub progress: f64,
    pub download_speed: u64,
    pub upload_speed: u64,
    pub size: u64,
    pub downloaded: u64,
    pub uploaded: u64,
    pub ratio: f64,
    pub peers: u32,
    pub seeds: u32,
    pub eta: Option<std::time::Duration>,
    pub added_at: chrono::DateTime<chrono::Utc>,
}

/// Torrent download/seeding status.
#[derive(Debug, Clone, Serialize)]
pub enum TorrentStatus {
    Downloading,
    Seeding,
    Paused,
    Error,
    Checking,
}

/// Result of adding a torrent.
#[derive(Debug, Clone, Serialize)]
pub struct AddTorrentResult {
    pub success: bool,
    pub message: String,
    pub stream_url: Option<String>,
    pub info_hash: Option<String>,
}

/// Server configuration settings.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerSettings {
    pub download_limit: Option<u64>, // bytes per second
    pub upload_limit: Option<u64>,
    pub max_connections: u32,
    pub streaming_port: u16,
    pub web_ui_port: u16,
    pub download_directory: String,
    pub enable_upnp: bool,
    pub enable_dht: bool,
    pub enable_pex: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::RiptideConfig;

    #[tokio::test]
    async fn test_web_handlers_creation() {
        let config = RiptideConfig::default();
        let torrent_engine = Arc::new(RwLock::new(TorrentEngine::new(config.clone())));
        let streaming_service = Arc::new(RwLock::new(DirectStreamingService::new(config)));

        let handlers = WebHandlers::new(torrent_engine, streaming_service);

        let stats = handlers.get_server_stats().await.unwrap();
        assert_eq!(stats.total_torrents, 0);
    }

    #[tokio::test]
    async fn test_get_library_items() {
        let config = RiptideConfig::default();
        let torrent_engine = Arc::new(RwLock::new(TorrentEngine::new(config.clone())));
        let streaming_service = Arc::new(RwLock::new(DirectStreamingService::new(config)));

        let handlers = WebHandlers::new(torrent_engine, streaming_service);

        let items = handlers.get_library_items().await.unwrap();
        assert_eq!(items.len(), 2); // Mock data returns 2 items
        assert_eq!(items[0].title, "Big Buck Bunny");
    }

    #[tokio::test]
    async fn test_get_torrent_list() {
        let config = RiptideConfig::default();
        let torrent_engine = Arc::new(RwLock::new(TorrentEngine::new(config.clone())));
        let streaming_service = Arc::new(RwLock::new(DirectStreamingService::new(config)));

        let handlers = WebHandlers::new(torrent_engine, streaming_service);

        let torrents = handlers.get_torrent_list().await.unwrap();
        assert_eq!(torrents.len(), 2); // Mock data returns 2 torrents

        let seeding_torrent = &torrents[0];
        assert!(matches!(seeding_torrent.status, TorrentStatus::Seeding));
        assert_eq!(seeding_torrent.progress, 100.0);
    }

    #[tokio::test]
    async fn test_server_settings() {
        let config = RiptideConfig::default();
        let torrent_engine = Arc::new(RwLock::new(TorrentEngine::new(config.clone())));
        let streaming_service = Arc::new(RwLock::new(DirectStreamingService::new(config)));

        let handlers = WebHandlers::new(torrent_engine, streaming_service);

        let settings = handlers.get_server_settings().await.unwrap();
        assert_eq!(settings.streaming_port, 8080);
        assert_eq!(settings.web_ui_port, 3000);
        assert!(settings.enable_dht);
    }
}
