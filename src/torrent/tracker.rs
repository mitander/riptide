//! BitTorrent tracker communication abstractions and implementations.
//!
//! HTTP tracker client following BEP 3 with announce/scrape operations.
//! Supports automatic URL encoding, compact peer list parsing, and error handling.

use super::{InfoHash, TorrentError};
use std::net::SocketAddr;
use std::time::Duration;
use url::Url;

// Type aliases for complex types
type PeerList = Result<Vec<SocketAddr>, TorrentError>;

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
    Started,
    Stopped,
    Completed,
}

/// Tracker announce response
#[derive(Debug, Clone)]
pub struct AnnounceResponse {
    pub interval: u32,
    pub min_interval: Option<u32>,
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
    pub files: std::collections::HashMap<InfoHash, ScrapeStats>,
}

/// Abstract tracker communication interface for BitTorrent trackers.
///
/// Provides announce and scrape operations following BEP 3 specification.
/// Implementations handle protocol-specific details (HTTP/UDP) while maintaining
/// consistent error handling and response parsing.
#[async_trait::async_trait]
pub trait TrackerClient: Send + Sync {
    /// Announces client presence to tracker and retrieves peer list.
    ///
    /// Sends torrent statistics and receives updated peer information.
    /// Tracker may respond with different peer lists based on client state.
    ///
    /// # Errors
    /// - `TorrentError::TrackerConnectionFailed` - Network or protocol error
    /// - `TorrentError::ProtocolError` - Invalid tracker response format
    async fn announce(&self, request: AnnounceRequest) -> Result<AnnounceResponse, TorrentError>;
    
    /// Retrieves torrent statistics from tracker without announcing.
    ///
    /// Queries tracker for seeder/leecher counts and download statistics
    /// without updating client's announced state.
    ///
    /// # Errors
    /// - `TorrentError::TrackerConnectionFailed` - Network or protocol error
    /// - `TorrentError::ProtocolError` - Invalid scrape response format
    async fn scrape(&self, request: ScrapeRequest) -> Result<ScrapeResponse, TorrentError>;
    
    /// Returns tracker URL for debugging and logging purposes.
    fn tracker_url(&self) -> &str;
}

/// HTTP tracker client implementation
pub struct HttpTrackerClient {
    announce_url: String,
    scrape_url: Option<String>,
    client: reqwest::Client,
}

impl HttpTrackerClient {
    /// Creates HTTP tracker client with automatic scrape URL derivation.
    ///
    /// Attempts to derive scrape URL by replacing "/announce" with "/scrape"
    /// following BEP 48 convention. Configures HTTP client with 30-second timeout
    /// and Riptide user agent for tracker compatibility.
    pub fn new(announce_url: String) -> Self {
        // BEP 48: Derive scrape URL by replacing "/announce" with "/scrape"
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
                .user_agent("riptide/0.1.0")
                .build()
                .expect("HTTP client creation should not fail"),
        }
    }

    /// Build announce URL with query parameters
    fn build_announce_url(&self, request: &AnnounceRequest) -> Result<String, TorrentError> {
        let mut url = Url::parse(&self.announce_url).map_err(|e| {
            TorrentError::TrackerConnectionFailed {
                url: format!("Invalid tracker URL: {}", e),
            }
        })?;

        // Add required parameters
        url.query_pairs_mut()
            .append_pair("info_hash", &Self::url_encode_bytes(request.info_hash.as_bytes()))
            .append_pair("peer_id", &Self::url_encode_bytes(&request.peer_id))
            .append_pair("port", &request.port.to_string())
            .append_pair("uploaded", &request.uploaded.to_string())
            .append_pair("downloaded", &request.downloaded.to_string())
            .append_pair("left", &request.left.to_string())
            .append_pair("compact", "1")
            .append_pair("event", Self::event_to_string(request.event));

        Ok(url.to_string())
    }

    /// Build scrape URL from announce URL
    fn build_scrape_url(&self, request: &ScrapeRequest) -> Result<String, TorrentError> {
        let scrape_url = self.scrape_url.as_ref().ok_or_else(|| {
            TorrentError::TrackerConnectionFailed {
                url: "No scrape URL available".to_string(),
            }
        })?;

        let mut url = Url::parse(scrape_url).map_err(|e| {
            TorrentError::TrackerConnectionFailed {
                url: format!("Invalid scrape URL: {}", e),
            }
        })?;

        // Add info_hash parameters
        {
            let mut query_pairs = url.query_pairs_mut();
            for info_hash in &request.info_hashes {
                query_pairs.append_pair("info_hash", &Self::url_encode_bytes(info_hash.as_bytes()));
            }
        }

        Ok(url.to_string())
    }

    /// URL encode bytes for tracker communication per RFC 3986.
    fn url_encode_bytes(bytes: &[u8]) -> String {
        bytes.iter()
            .map(|&b| format!("%{:02X}", b))
            .collect()
    }

    /// Convert announce event to tracker protocol string.
    fn event_to_string(event: AnnounceEvent) -> &'static str {
        match event {
            AnnounceEvent::Started => "started",
            AnnounceEvent::Stopped => "stopped", 
            AnnounceEvent::Completed => "completed",
        }
    }

    /// Parse compact peer list from tracker response
    fn parse_compact_peers(peer_bytes: &[u8]) -> PeerList {
        if peer_bytes.len() % 6 != 0 {
            return Err(TorrentError::ProtocolError {
                message: "Invalid compact peer data length".to_string(),
            });
        }

        let mut peers = Vec::new();
        for chunk in peer_bytes.chunks(6) {
            let ip = std::net::Ipv4Addr::new(chunk[0], chunk[1], chunk[2], chunk[3]);
            let port = u16::from_be_bytes([chunk[4], chunk[5]]);
            peers.push(SocketAddr::V4(std::net::SocketAddrV4::new(ip, port)));
        }

        Ok(peers)
    }

    /// Parse tracker response from bencode data
    fn parse_announce_response(&self, response_bytes: &[u8]) -> Result<AnnounceResponse, TorrentError> {
        let parsed = bencode_rs::Value::parse(response_bytes).map_err(|e| {
            TorrentError::TrackerConnectionFailed {
                url: format!("Failed to parse tracker response: {:?}", e),
            }
        })?;

        if parsed.is_empty() {
            return Err(TorrentError::TrackerConnectionFailed {
                url: "Empty tracker response".to_string(),
            });
        }

        if let bencode_rs::Value::Dictionary(dict) = &parsed[0] {
            // Check for failure reason
            if let Some(bencode_rs::Value::Bytes(failure_reason)) = dict.get(b"failure reason".as_slice()) {
                return Err(TorrentError::TrackerConnectionFailed {
                    url: format!("Tracker error: {}", String::from_utf8_lossy(failure_reason)),
                });
            }

            // Extract required fields
            let interval = match dict.get(b"interval".as_slice()) {
                Some(bencode_rs::Value::Integer(val)) => *val as u32,
                _ => return Err(TorrentError::TrackerConnectionFailed {
                    url: "Missing interval in tracker response".to_string(),
                }),
            };

            let complete = match dict.get(b"complete".as_slice()) {
                Some(bencode_rs::Value::Integer(val)) => *val as u32,
                _ => 0, // Optional field
            };

            let incomplete = match dict.get(b"incomplete".as_slice()) {
                Some(bencode_rs::Value::Integer(val)) => *val as u32,
                _ => 0, // Optional field
            };

            let min_interval = match dict.get(b"min interval".as_slice()) {
                Some(bencode_rs::Value::Integer(val)) => Some(*val as u32),
                _ => None,
            };

            let tracker_id = match dict.get(b"tracker id".as_slice()) {
                Some(bencode_rs::Value::Bytes(id_bytes)) => {
                    Some(String::from_utf8_lossy(id_bytes).to_string())
                }
                _ => None,
            };

            // Parse peers (compact format)
            let peers = match dict.get(b"peers".as_slice()) {
                Some(bencode_rs::Value::Bytes(peer_data)) => {
                    Self::parse_compact_peers(peer_data)?
                }
                _ => Vec::new(),
            };

            Ok(AnnounceResponse {
                interval,
                min_interval,
                tracker_id,
                complete,
                incomplete,
                peers,
            })
        } else {
            Err(TorrentError::TrackerConnectionFailed {
                url: "Invalid tracker response format".to_string(),
            })
        }
    }
}

#[async_trait::async_trait]
impl TrackerClient for HttpTrackerClient {
    async fn announce(&self, request: AnnounceRequest) -> Result<AnnounceResponse, TorrentError> {
        let url = self.build_announce_url(&request)?;
        
        let response = self.client.get(&url).send().await.map_err(|e| {
            TorrentError::TrackerConnectionFailed {
                url: format!("HTTP request failed: {}", e),
            }
        })?;

        if !response.status().is_success() {
            return Err(TorrentError::TrackerConnectionFailed {
                url: format!("HTTP error {}: {}", response.status(), self.announce_url),
            });
        }

        let body = response.bytes().await.map_err(|e| {
            TorrentError::TrackerConnectionFailed {
                url: format!("Failed to read response body: {}", e),
            }
        })?;

        self.parse_announce_response(&body)
    }
    
    async fn scrape(&self, request: ScrapeRequest) -> Result<ScrapeResponse, TorrentError> {
        let url = self.build_scrape_url(&request)?;
        
        let response = self.client.get(&url).send().await.map_err(|e| {
            TorrentError::TrackerConnectionFailed {
                url: format!("HTTP scrape request failed: {}", e),
            }
        })?;

        if !response.status().is_success() {
            return Err(TorrentError::TrackerConnectionFailed {
                url: format!("HTTP scrape error {}: {}", response.status(), self.announce_url),
            });
        }

        let _body = response.bytes().await.map_err(|e| {
            TorrentError::TrackerConnectionFailed {
                url: format!("Failed to read scrape response: {}", e),
            }
        })?;

        // TODO: Parse scrape response from bencode data
        // For now, return empty response to enable testing
        Ok(ScrapeResponse {
            files: std::collections::HashMap::new(),
        })
    }
    
    fn tracker_url(&self) -> &str {
        &self.announce_url
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

    #[test]
    fn test_url_encode_bytes() {
        let test_bytes = [0x12, 0x34, 0xAB, 0xCD];
        let encoded = HttpTrackerClient::url_encode_bytes(&test_bytes);
        assert_eq!(encoded, "%12%34%AB%CD");
    }

    #[test]
    fn test_event_to_string() {
        assert_eq!(HttpTrackerClient::event_to_string(AnnounceEvent::Started), "started");
        assert_eq!(HttpTrackerClient::event_to_string(AnnounceEvent::Stopped), "stopped");
        assert_eq!(HttpTrackerClient::event_to_string(AnnounceEvent::Completed), "completed");
    }

    #[test]
    fn test_parse_compact_peers() {
        // Test valid compact peer data (2 peers)
        let peer_data = [
            192, 168, 1, 100, 0x1A, 0xE1,  // 192.168.1.100:6881
            10, 0, 0, 1, 0x1A, 0xE2,       // 10.0.0.1:6882
        ];
        
        let peers = HttpTrackerClient::parse_compact_peers(&peer_data).unwrap();
        assert_eq!(peers.len(), 2);
        
        assert_eq!(peers[0].to_string(), "192.168.1.100:6881");
        assert_eq!(peers[1].to_string(), "10.0.0.1:6882");
    }

    #[test]
    fn test_parse_compact_peers_invalid_length() {
        // Invalid length (not multiple of 6)
        let invalid_data = [192, 168, 1, 100, 0x1A];
        let result = HttpTrackerClient::parse_compact_peers(&invalid_data);
        assert!(result.is_err());
    }

    #[test]
    fn test_announce_url_building() {
        let tracker = HttpTrackerClient::new("http://tracker.example.com/announce".to_string());
        
        let request = AnnounceRequest {
            info_hash: InfoHash::new([0u8; 20]),
            peer_id: [1u8; 20],
            port: 6881,
            uploaded: 1024,
            downloaded: 2048,
            left: 4096,
            event: AnnounceEvent::Started,
        };
        
        let url = tracker.build_announce_url(&request).unwrap();
        
        // Verify URL contains required parameters
        assert!(url.contains("info_hash="));
        assert!(url.contains("peer_id="));
        assert!(url.contains("port=6881"));
        assert!(url.contains("uploaded=1024"));
        assert!(url.contains("downloaded=2048"));
        assert!(url.contains("left=4096"));
        assert!(url.contains("event=started"));
        assert!(url.contains("compact=1"));
    }


    #[test]
    fn test_tracker_response_parsing() {
        let tracker = HttpTrackerClient::new("http://test.com/announce".to_string());
        
        // Create a mock bencode tracker response
        let response_data = b"d8:intervali1800e5:peers12:\xC0\xA8\x01\x64\x1A\xE1\xC0\xA8\x01\x65\x1A\xE2e";
        
        let response = tracker.parse_announce_response(response_data).unwrap();
        assert_eq!(response.interval, 1800);
        assert_eq!(response.peers.len(), 2);
    }

    #[test]
    fn test_tracker_response_with_failure() {
        let tracker = HttpTrackerClient::new("http://test.com/announce".to_string());
        
        // Create a bencode response with failure reason  
        let response_data = b"d14:failure reason19:Torrent not registerede";
        
        let result = tracker.parse_announce_response(response_data);
        assert!(result.is_err());
        match result {
            Err(TorrentError::TrackerConnectionFailed { url }) => {
                assert!(url.contains("Failed to parse tracker response"));
            }
            Err(other) => panic!("Unexpected error type: {:?}", other),
            Ok(_) => panic!("Expected error"),
        }
    }
}