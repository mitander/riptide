//! HTTP tracker client implementation with URL building and response parsing

use std::net::SocketAddr;

use super::types::{
    AnnounceEvent, AnnounceRequest, AnnounceResponse, PeerList, ScrapeRequest, ScrapeResponse,
    ScrapeStats,
};
use crate::config::NetworkConfig;
use crate::torrent::{InfoHash, TorrentError};

/// HTTP tracker client implementation
pub struct HttpTrackerClient {
    pub(super) announce_url: String,
    pub(super) scrape_url: Option<String>,
    pub(super) client: reqwest::Client,
}

impl HttpTrackerClient {
    /// Creates HTTP tracker client with automatic scrape URL derivation.
    ///
    /// Attempts to derive scrape URL by replacing "/announce" with "/scrape"
    /// following BEP 48 convention. Uses network configuration for timeout
    /// and user agent settings.
    pub fn new(announce_url: String, config: &NetworkConfig) -> Self {
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
                .timeout(config.tracker_timeout)
                .user_agent(config.user_agent)
                .redirect(reqwest::redirect::Policy::limited(3))
                .build()
                .expect("HTTP client creation should not fail"),
        }
    }

    /// Build announce URL with query parameters
    pub(super) fn build_announce_url(
        &self,
        request: &AnnounceRequest,
    ) -> Result<String, TorrentError> {
        // For BitTorrent trackers, we need to manually encode to avoid double-encoding
        let info_hash_encoded = Self::url_encode_bytes(request.info_hash.as_bytes());
        let peer_id_encoded = Self::url_encode_bytes(&request.peer_id);
        let event_str = Self::event_to_string(request.event);

        let query = format!(
            "info_hash={}&peer_id={}&port={}&uploaded={}&downloaded={}&left={}&compact=1&event={}",
            info_hash_encoded,
            peer_id_encoded,
            request.port,
            request.uploaded,
            request.downloaded,
            request.left,
            event_str
        );

        Ok(format!("{}?{}", self.announce_url, query))
    }

    /// Build scrape URL from announce URL
    ///
    /// # Errors
    /// - `TorrentError::TrackerConnectionFailed` - No scrape URL available
    /// - `TorrentError::UrlParsing` - Invalid URL format
    pub(super) fn build_scrape_url(&self, request: &ScrapeRequest) -> Result<String, TorrentError> {
        let scrape_url =
            self.scrape_url
                .as_ref()
                .ok_or_else(|| TorrentError::TrackerConnectionFailed {
                    url: "No scrape URL available".to_string(),
                })?;

        // Manually build query to avoid double-encoding
        let query_parts: Vec<String> = request
            .info_hashes
            .iter()
            .map(|info_hash| format!("info_hash={}", Self::url_encode_bytes(info_hash.as_bytes())))
            .collect();

        let query = query_parts.join("&");
        Ok(format!("{scrape_url}?{query}"))
    }

    /// URL encode bytes for tracker communication per RFC 3986.
    pub(crate) fn url_encode_bytes(bytes: &[u8]) -> String {
        bytes.iter().map(|&b| format!("%{b:02X}")).collect()
    }

    /// Convert announce event to tracker protocol string.
    pub(crate) fn event_to_string(event: AnnounceEvent) -> &'static str {
        match event {
            AnnounceEvent::Started => "started",
            AnnounceEvent::Stopped => "stopped",
            AnnounceEvent::Completed => "completed",
        }
    }

    /// Parse compact peer list from tracker response
    ///
    /// # Errors
    /// - `TorrentError::ProtocolError` - Invalid compact peer data length (not multiple of 6 bytes)
    pub(crate) fn parse_compact_peers(peer_bytes: &[u8]) -> PeerList {
        if !peer_bytes.len().is_multiple_of(6) {
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
    pub(super) fn parse_announce_response(
        &self,
        response_bytes: &[u8],
    ) -> Result<AnnounceResponse, TorrentError> {
        let parsed =
            bencode_rs::Value::parse(response_bytes).map_err(|e| TorrentError::ProtocolError {
                message: format!("Failed to parse tracker response: {e:?}"),
            })?;

        if parsed.is_empty() {
            return Err(TorrentError::ProtocolError {
                message: "Empty tracker response".to_string(),
            });
        }

        if let bencode_rs::Value::Dictionary(dict) = &parsed[0] {
            // Check for failure reason - treat as torrent not found if tracker explicitly rejects it
            if let Some(bencode_rs::Value::Bytes(failure_reason)) =
                dict.get(b"failure reason".as_slice())
            {
                let reason = String::from_utf8_lossy(failure_reason);
                return Err(TorrentError::TorrentNotFoundOnTracker {
                    url: format!("Tracker error: {reason}"),
                });
            }

            // Extract required fields
            let interval = match dict.get(b"interval".as_slice()) {
                Some(bencode_rs::Value::Integer(val)) => *val as u32,
                _ => {
                    return Err(TorrentError::ProtocolError {
                        message: "Missing interval in tracker response".to_string(),
                    });
                }
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
                Some(bencode_rs::Value::Bytes(peer_data)) => Self::parse_compact_peers(peer_data)?,
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
            Err(TorrentError::ProtocolError {
                message: "Invalid tracker response format".to_string(),
            })
        }
    }

    /// Parse tracker scrape response from bencode data
    pub(super) fn parse_scrape_response(
        &self,
        response_bytes: &[u8],
    ) -> Result<ScrapeResponse, TorrentError> {
        let parsed =
            bencode_rs::Value::parse(response_bytes).map_err(|e| TorrentError::ProtocolError {
                message: format!("Failed to parse scrape response: {e:?}"),
            })?;

        if parsed.is_empty() {
            return Err(TorrentError::ProtocolError {
                message: "Empty scrape response".to_string(),
            });
        }

        if let bencode_rs::Value::Dictionary(dict) = &parsed[0] {
            // Check for failure reason
            if let Some(bencode_rs::Value::Bytes(failure_reason)) =
                dict.get(b"failure reason".as_slice())
            {
                return Err(TorrentError::ProtocolError {
                    message: format!("Scrape error: {}", String::from_utf8_lossy(failure_reason)),
                });
            }

            // Parse files dictionary
            let mut files = std::collections::HashMap::new();

            if let Some(bencode_rs::Value::Dictionary(files_dict)) = dict.get(b"files".as_slice()) {
                for (info_hash_bytes, file_data) in files_dict {
                    if info_hash_bytes.len() == 20 {
                        let mut hash_array = [0u8; 20];
                        hash_array.copy_from_slice(info_hash_bytes);
                        let info_hash = InfoHash::new(hash_array);

                        if let bencode_rs::Value::Dictionary(file_dict) = file_data {
                            let complete = match file_dict.get(b"complete".as_slice()) {
                                Some(bencode_rs::Value::Integer(val)) => *val as u32,
                                _ => 0,
                            };

                            let downloaded = match file_dict.get(b"downloaded".as_slice()) {
                                Some(bencode_rs::Value::Integer(val)) => *val as u32,
                                _ => 0,
                            };

                            let incomplete = match file_dict.get(b"incomplete".as_slice()) {
                                Some(bencode_rs::Value::Integer(val)) => *val as u32,
                                _ => 0,
                            };

                            files.insert(
                                info_hash,
                                ScrapeStats {
                                    complete,
                                    downloaded,
                                    incomplete,
                                },
                            );
                        }
                    }
                }
            }

            Ok(ScrapeResponse { files })
        } else {
            Err(TorrentError::ProtocolError {
                message: "Invalid scrape response format".to_string(),
            })
        }
    }
}

use async_trait::async_trait;

use super::types::TrackerClient;

#[async_trait]
impl TrackerClient for HttpTrackerClient {
    /// Announces client presence to tracker and retrieves peer list.
    ///
    /// Sends torrent statistics and receives updated peer information.
    /// Handles HTTP request/response cycle with proper error handling.
    ///
    /// # Errors
    /// - `TorrentError::TrackerConnectionFailed` - Network or HTTP error
    /// - `TorrentError::ProtocolError` - Invalid tracker response format
    async fn announce(&self, request: AnnounceRequest) -> Result<AnnounceResponse, TorrentError> {
        let url = self.build_announce_url(&request)?;
        tracing::debug!("Announcing to tracker: {}", self.announce_url);

        let response = self.client.get(&url).send().await.map_err(|e| {
            tracing::warn!("HTTP request to {} failed: {}", self.announce_url, e);

            // Use specific error variants for better pattern matching
            if e.is_timeout() {
                TorrentError::TrackerTimeout {
                    url: self.announce_url.clone(),
                }
            } else {
                TorrentError::TrackerConnectionFailed {
                    url: self.announce_url.clone(),
                }
            }
        })?;

        let status = response.status();
        if !status.is_success() {
            tracing::warn!(
                "Tracker {} returned error status: {}",
                self.announce_url,
                status
            );
            return Err(match status.as_u16() {
                404 => TorrentError::TorrentNotFoundOnTracker {
                    url: self.announce_url.clone(),
                },
                500..=599 => TorrentError::TrackerServerError {
                    url: self.announce_url.clone(),
                    status: status.as_u16(),
                },
                _ => TorrentError::TrackerConnectionFailed {
                    url: self.announce_url.clone(),
                },
            });
        }

        let response_bytes = response.bytes().await.map_err(|e| {
            tracing::warn!(
                "Failed to read response body from {}: {}",
                self.announce_url,
                e
            );
            TorrentError::TrackerConnectionFailed {
                url: format!("Failed to read response body: {e}"),
            }
        })?;

        tracing::debug!(
            "Successfully announced to {}, received {} peers",
            self.announce_url,
            response_bytes.len() / 6
        );

        self.parse_announce_response(&response_bytes).map_err(|e| {
            tracing::warn!("Failed to parse response from {}: {}", self.announce_url, e);
            e
        })
    }

    /// Retrieves torrent statistics from tracker without announcing.
    ///
    /// Queries tracker for seeder/leecher counts without updating client state.
    /// Returns statistics indexed by info hash.
    ///
    /// # Errors
    /// - `TorrentError::TrackerConnectionFailed` - Network or HTTP error
    /// - `TorrentError::ProtocolError` - Invalid scrape response format
    async fn scrape(&self, request: ScrapeRequest) -> Result<ScrapeResponse, TorrentError> {
        self.scrape_url
            .as_ref()
            .ok_or_else(|| TorrentError::TrackerConnectionFailed {
                url: "Tracker does not support scrape operation".to_string(),
            })?;

        let url = self.build_scrape_url(&request)?;

        let response = self.client.get(&url).send().await.map_err(|e| {
            tracing::warn!("HTTP scrape request to {} failed: {}", self.announce_url, e);

            // Use specific error variants for better categorization
            if e.is_timeout() {
                TorrentError::TrackerTimeout {
                    url: self.announce_url.clone(),
                }
            } else {
                TorrentError::TrackerConnectionFailed {
                    url: self.announce_url.clone(),
                }
            }
        })?;

        let status = response.status();
        if !status.is_success() {
            tracing::warn!(
                "Tracker scrape {} returned error status: {}",
                self.announce_url,
                status
            );
            return Err(match status.as_u16() {
                404 => TorrentError::TorrentNotFoundOnTracker {
                    url: self.announce_url.clone(),
                },
                500..=599 => TorrentError::TrackerServerError {
                    url: self.announce_url.clone(),
                    status: status.as_u16(),
                },
                _ => TorrentError::TrackerConnectionFailed {
                    url: self.announce_url.clone(),
                },
            });
        }

        let response_bytes = response.bytes().await.map_err(|e| {
            tracing::warn!(
                "Failed to read scrape response body from {}: {}",
                self.announce_url,
                e
            );
            TorrentError::TrackerConnectionFailed {
                url: format!("Failed to read response body: {e}"),
            }
        })?;

        self.parse_scrape_response(&response_bytes)
    }

    /// Returns tracker URL for debugging and logging purposes.
    fn tracker_url(&self) -> &str {
        &self.announce_url
    }
}

#[cfg(test)]
mod tracker_client_tests {
    use std::time::Duration;

    use super::*;
    use crate::config::NetworkConfig;
    use crate::torrent::tracker::AnnounceEvent;
    use crate::torrent::{InfoHash, PeerId};

    fn create_test_network_config() -> NetworkConfig {
        NetworkConfig {
            tracker_timeout: Duration::from_secs(5),
            min_announce_interval: Duration::from_secs(60),
            default_announce_interval: Duration::from_secs(300),
            user_agent: "Riptide/test",
            max_peer_connections: 25,
            download_limit: None,
            upload_limit: None,
            peer_timeout: Duration::from_secs(60),
        }
    }

    #[test]
    fn test_http_tracker_client_new() {
        let config = create_test_network_config();

        // Test with standard announce URL
        let client =
            HttpTrackerClient::new("http://tracker.example.com/announce".to_string(), &config);
        assert_eq!(client.announce_url, "http://tracker.example.com/announce");
        assert_eq!(
            client.scrape_url,
            Some("http://tracker.example.com/scrape".to_string())
        );

        // Test with announce URL that doesn't end in /announce
        let client =
            HttpTrackerClient::new("http://tracker.example.com/tracker".to_string(), &config);
        assert_eq!(client.announce_url, "http://tracker.example.com/tracker");
        assert_eq!(client.scrape_url, None);
    }

    #[test]
    fn test_build_announce_url() {
        let config = create_test_network_config();
        let client =
            HttpTrackerClient::new("http://tracker.example.com/announce".to_string(), &config);

        let info_hash = InfoHash::new([0x11; 20]);
        let peer_id = PeerId::new([0x22; 20]);
        let request = AnnounceRequest {
            info_hash,
            peer_id: *peer_id.as_bytes(),
            port: 6881,
            uploaded: 1000,
            downloaded: 500,
            left: 2000,
            event: AnnounceEvent::Started,
        };

        let url = client.build_announce_url(&request).unwrap();
        assert!(
            url.contains("info_hash=%11%11%11%11%11%11%11%11%11%11%11%11%11%11%11%11%11%11%11%11")
        );
        assert!(
            url.contains("peer_id=%22%22%22%22%22%22%22%22%22%22%22%22%22%22%22%22%22%22%22%22")
        );
        assert!(url.contains("port=6881"));
        assert!(url.contains("uploaded=1000"));
        assert!(url.contains("downloaded=500"));
        assert!(url.contains("left=2000"));
        assert!(url.contains("compact=1"));
        assert!(url.contains("event=started"));
    }

    #[test]
    fn test_build_scrape_url() {
        let config = create_test_network_config();
        let client =
            HttpTrackerClient::new("http://tracker.example.com/announce".to_string(), &config);

        let info_hash1 = InfoHash::new([0xAA; 20]);
        let info_hash2 = InfoHash::new([0xBB; 20]);
        let request = ScrapeRequest {
            info_hashes: vec![info_hash1, info_hash2],
        };

        let url = client.build_scrape_url(&request).unwrap();
        assert!(url.contains("http://tracker.example.com/scrape"));
        assert!(
            url.contains("info_hash=%AA%AA%AA%AA%AA%AA%AA%AA%AA%AA%AA%AA%AA%AA%AA%AA%AA%AA%AA%AA")
        );
        assert!(
            url.contains("info_hash=%BB%BB%BB%BB%BB%BB%BB%BB%BB%BB%BB%BB%BB%BB%BB%BB%BB%BB%BB%BB")
        );
    }

    #[test]
    fn test_parse_compact_peers_success() {
        // 127.0.0.1:6881, 192.168.1.100:50000
        let peer_bytes = vec![
            127, 0, 0, 1, 26, 225, // 127.0.0.1:6881 (26*256+225=6881)
            192, 168, 1, 100, 195, 80, // 192.168.1.100:50000
        ];

        let peers = HttpTrackerClient::parse_compact_peers(&peer_bytes).unwrap();
        assert_eq!(peers.len(), 2);
        assert_eq!(peers[0].to_string(), "127.0.0.1:6881");
        assert_eq!(peers[1].to_string(), "192.168.1.100:50000");
    }

    #[test]
    fn test_parse_compact_peers_invalid_length() {
        let peer_bytes = vec![127, 0, 0, 1, 26]; // 5 bytes, not multiple of 6
        let result = HttpTrackerClient::parse_compact_peers(&peer_bytes);
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            TorrentError::ProtocolError { message } if message.contains("Invalid compact peer data length")
        ));
    }

    #[test]
    fn test_parse_announce_response_success() {
        let bencode_data =
            b"d8:intervali1800e8:completei10e10:incompletei5e5:peers6:\x7f\x00\x00\x01\x1a\x09e";
        let config = create_test_network_config();
        let client = HttpTrackerClient::new("http://example.com/announce".to_string(), &config);

        let response = client.parse_announce_response(bencode_data).unwrap();
        assert_eq!(response.interval, 1800);
        assert_eq!(response.complete, 10);
        assert_eq!(response.incomplete, 5);
        assert_eq!(response.peers.len(), 1);
        assert_eq!(response.peers[0].to_string(), "127.0.0.1:6665");
    }

    #[test]
    fn test_parse_announce_response_failure_reason() {
        let bencode_data = b"d14:failure reason5:errore";
        let config = create_test_network_config();
        let client = HttpTrackerClient::new("http://example.com/announce".to_string(), &config);

        let result = client.parse_announce_response(bencode_data);
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            TorrentError::TorrentNotFoundOnTracker { url } if url == "Tracker error: error"
        ));
    }

    #[test]
    fn test_parse_scrape_response_success() {
        let info_hash = InfoHash::new([0x11; 20]);
        // Create proper bencode data with binary info hash
        let mut bencode_data = Vec::new();
        bencode_data.extend_from_slice(b"d5:filesd20:");
        bencode_data.extend_from_slice(info_hash.as_bytes());
        bencode_data.extend_from_slice(b"d8:completei10e10:downloadedi20e10:incompletei5eeee");

        let config = create_test_network_config();
        let client = HttpTrackerClient::new("http://example.com/announce".to_string(), &config);

        let response = client.parse_scrape_response(&bencode_data).unwrap();
        assert_eq!(response.files.len(), 1);
        let stats = response.files.get(&info_hash).unwrap();
        assert_eq!(stats.complete, 10);
        assert_eq!(stats.downloaded, 20);
        assert_eq!(stats.incomplete, 5);
    }

    #[test]
    fn test_parse_scrape_response_failure_reason() {
        let bencode_data = b"d14:failure reason5:errore";
        let config = create_test_network_config();
        let client = HttpTrackerClient::new("http://example.com/announce".to_string(), &config);

        let result = client.parse_scrape_response(bencode_data);
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            TorrentError::ProtocolError { message } if message == "Scrape error: error"
        ));
    }
}
