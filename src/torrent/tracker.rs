//! Tracker communication abstractions and implementations

use super::{InfoHash, TorrentError};
use std::net::SocketAddr;
use std::time::Duration;

/// Tracker announce request
#[derive(Debug, Clone)]
pub struct AnnounceRequest {
    pub info_hash: InfoHash,
    pub peer_id: [u8; 20],
    pub port: u16,
    pub uploaded: u64,
    pub downloaded: u64,
    pub left: u64,
    pub event: AnnounceEvent,
}

/// BitTorrent announce events
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnnounceEvent {
    None,
    Started,
    Stopped,
    Completed,
}

/// Tracker announce response
#[derive(Debug, Clone)]
pub struct AnnounceResponse {
    pub interval: Duration,
    pub min_interval: Option<Duration>,
    pub tracker_id: Option<String>,
    pub complete: u32,
    pub incomplete: u32,
    pub peers: Vec<SocketAddr>,
}

/// Tracker scrape request
#[derive(Debug, Clone)]
pub struct ScrapeRequest {
    pub info_hashes: Vec<InfoHash>,
}

/// Individual torrent statistics from scrape
#[derive(Debug, Clone)]
pub struct ScrapeStats {
    pub complete: u32,
    pub downloaded: u32,
    pub incomplete: u32,
}

/// Tracker scrape response
#[derive(Debug, Clone)]
pub struct ScrapeResponse {
    pub torrents: std::collections::HashMap<InfoHash, ScrapeStats>,
}

/// Abstract tracker communication interface
#[async_trait::async_trait]
pub trait TrackerClient: Send + Sync {
    /// Announce to tracker and get peer list
    async fn announce(&self, request: AnnounceRequest) -> Result<AnnounceResponse, TorrentError>;
    
    /// Scrape tracker for torrent statistics
    async fn scrape(&self, request: ScrapeRequest) -> Result<ScrapeResponse, TorrentError>;
    
    /// Get tracker identifier for debugging
    fn tracker_url(&self) -> &str;
}

/// HTTP tracker client implementation
pub struct HttpTrackerClient {
    announce_url: String,
    scrape_url: Option<String>,
    client: reqwest::Client,
}

impl HttpTrackerClient {
    pub fn new(announce_url: String) -> Self {
        // Derive scrape URL from announce URL per BEP 48
        let scrape_url = announce_url.replace("/announce", "/scrape");
        let scrape_url = if scrape_url != announce_url {
            Some(scrape_url)
        } else {
            None
        };
        
        Self {
            announce_url,
            scrape_url,
            client: reqwest::Client::builder()
                .timeout(Duration::from_secs(30))
                .build()
                .expect("HTTP client creation should not fail"),
        }
    }
}

#[async_trait::async_trait]
impl TrackerClient for HttpTrackerClient {
    async fn announce(&self, _request: AnnounceRequest) -> Result<AnnounceResponse, TorrentError> {
        // TODO: Implement HTTP tracker announce protocol
        Err(TorrentError::TrackerConnectionFailed {
            url: self.announce_url.clone(),
        })
    }
    
    async fn scrape(&self, _request: ScrapeRequest) -> Result<ScrapeResponse, TorrentError> {
        // TODO: Implement HTTP tracker scrape protocol
        Err(TorrentError::TrackerConnectionFailed {
            url: self.scrape_url.as_ref().unwrap_or(&self.announce_url).clone(),
        })
    }
    
    fn tracker_url(&self) -> &str {
        &self.announce_url
    }
}

/// UDP tracker client implementation
pub struct UdpTrackerClient {
    tracker_url: String,
}

impl UdpTrackerClient {
    pub fn new(tracker_url: String) -> Self {
        Self { tracker_url }
    }
}

#[async_trait::async_trait]
impl TrackerClient for UdpTrackerClient {
    async fn announce(&self, _request: AnnounceRequest) -> Result<AnnounceResponse, TorrentError> {
        // TODO: Implement UDP tracker announce protocol per BEP 15
        Err(TorrentError::TrackerConnectionFailed {
            url: self.tracker_url.clone(),
        })
    }
    
    async fn scrape(&self, _request: ScrapeRequest) -> Result<ScrapeResponse, TorrentError> {
        // TODO: Implement UDP tracker scrape protocol
        Err(TorrentError::TrackerConnectionFailed {
            url: self.tracker_url.clone(),
        })
    }
    
    fn tracker_url(&self) -> &str {
        &self.tracker_url
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_announce_request_structure() {
        let request = AnnounceRequest {
            info_hash: InfoHash::new([0u8; 20]),
            peer_id: [1u8; 20],
            port: 6881,
            uploaded: 0,
            downloaded: 1024,
            left: 8192,
            event: AnnounceEvent::Started,
        };
        
        assert_eq!(request.port, 6881);
        assert_eq!(request.event, AnnounceEvent::Started);
    }
    
    #[tokio::test]
    async fn test_http_tracker_interface() {
        let tracker = HttpTrackerClient::new("http://tracker.example.com/announce".to_string());
        
        assert_eq!(tracker.tracker_url(), "http://tracker.example.com/announce");
        
        let request = AnnounceRequest {
            info_hash: InfoHash::new([0u8; 20]),
            peer_id: [1u8; 20],
            port: 6881,
            uploaded: 0,
            downloaded: 0,
            left: 1000,
            event: AnnounceEvent::Started,
        };
        
        // Should fail with stub implementation
        let result = tracker.announce(request).await;
        assert!(result.is_err());
    }
    
    #[test]
    fn test_scrape_url_derivation() {
        let tracker = HttpTrackerClient::new("http://tracker.example.com/announce".to_string());
        
        // Scrape URL should be derived correctly
        assert!(tracker.scrape_url.is_some());
        assert_eq!(tracker.scrape_url.unwrap(), "http://tracker.example.com/scrape");
    }
}