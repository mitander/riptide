//! HTTP tracker client implementation with URL building and response parsing

use std::net::SocketAddr;

use url::Url;

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
                .build()
                .expect("HTTP client creation should not fail"),
        }
    }

    /// Build announce URL with query parameters
    pub(super) fn build_announce_url(
        &self,
        request: &AnnounceRequest,
    ) -> Result<String, TorrentError> {
        let mut url = Url::parse(&self.announce_url)?;

        // Add required parameters
        url.query_pairs_mut()
            .append_pair(
                "info_hash",
                &Self::url_encode_bytes(request.info_hash.as_bytes()),
            )
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

        let mut url = Url::parse(scrape_url)?;

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
    pub(crate) fn parse_compact_peers(peer_bytes: &[u8]) -> PeerList {
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
    pub(super) fn parse_announce_response(
        &self,
        response_bytes: &[u8],
    ) -> Result<AnnounceResponse, TorrentError> {
        let parsed = bencode_rs::Value::parse(response_bytes).map_err(|e| {
            TorrentError::TrackerConnectionFailed {
                url: format!("Failed to parse tracker response: {e:?}"),
            }
        })?;

        if parsed.is_empty() {
            return Err(TorrentError::TrackerConnectionFailed {
                url: "Empty tracker response".to_string(),
            });
        }

        if let bencode_rs::Value::Dictionary(dict) = &parsed[0] {
            // Check for failure reason
            if let Some(bencode_rs::Value::Bytes(failure_reason)) =
                dict.get(b"failure reason".as_slice())
            {
                return Err(TorrentError::TrackerConnectionFailed {
                    url: format!("Tracker error: {}", String::from_utf8_lossy(failure_reason)),
                });
            }

            // Extract required fields
            let interval = match dict.get(b"interval".as_slice()) {
                Some(bencode_rs::Value::Integer(val)) => *val as u32,
                _ => {
                    return Err(TorrentError::TrackerConnectionFailed {
                        url: "Missing interval in tracker response".to_string(),
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
            Err(TorrentError::TrackerConnectionFailed {
                url: "Invalid tracker response format".to_string(),
            })
        }
    }

    /// Parse tracker scrape response from bencode data
    pub(super) fn parse_scrape_response(
        &self,
        response_bytes: &[u8],
    ) -> Result<ScrapeResponse, TorrentError> {
        let parsed = bencode_rs::Value::parse(response_bytes).map_err(|e| {
            TorrentError::TrackerConnectionFailed {
                url: format!("Failed to parse scrape response: {e:?}"),
            }
        })?;

        if parsed.is_empty() {
            return Err(TorrentError::TrackerConnectionFailed {
                url: "Empty scrape response".to_string(),
            });
        }

        if let bencode_rs::Value::Dictionary(dict) = &parsed[0] {
            // Check for failure reason
            if let Some(bencode_rs::Value::Bytes(failure_reason)) =
                dict.get(b"failure reason".as_slice())
            {
                return Err(TorrentError::TrackerConnectionFailed {
                    url: format!("Scrape error: {}", String::from_utf8_lossy(failure_reason)),
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
            Err(TorrentError::TrackerConnectionFailed {
                url: "Invalid scrape response format".to_string(),
            })
        }
    }
}
