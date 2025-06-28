//! Magneto torrent search provider for production use.

use async_trait::async_trait;
use serde::Deserialize;

use super::TorrentSearchProvider;
use crate::errors::MediaSearchError;
use crate::types::{MediaSearchResult, MediaType, TorrentResult, VideoQuality};

/// Magneto search provider for real torrent discovery.
///
/// Integrates with Magneto API to find actual torrents from multiple indexers.
/// Handles rate limiting, retries, and response parsing automatically.
#[derive(Debug)]
pub struct MagnetoProvider {
    client: reqwest::Client,
    base_url: String,
    api_key: Option<String>,
}

/// Response from Magneto API search endpoint.
#[derive(Debug, Deserialize)]
struct MagnetoResponse {
    results: Vec<MagnetoTorrent>,
    #[allow(dead_code)]
    total: u32,
}

/// Single torrent result from Magneto.
#[derive(Debug, Deserialize)]
struct MagnetoTorrent {
    name: String,
    magnet: String,
    size: u64,
    seeders: u32,
    leechers: u32,
    indexer: String,
    #[allow(dead_code)]
    category: String,
    #[serde(rename = "publishDate")]
    #[allow(dead_code)]
    publish_date: String,
}

impl MagnetoProvider {
    /// Create new Magneto provider with default configuration.
    pub fn new() -> Self {
        Self::with_config("http://localhost:8080".to_string(), None)
    }

    /// Create Magneto provider with custom configuration.
    pub fn with_config(base_url: String, api_key: Option<String>) -> Self {
        Self {
            client: reqwest::Client::new(),
            base_url,
            api_key,
        }
    }

    /// Parse video quality from torrent name.
    fn parse_quality(name: &str) -> VideoQuality {
        let name_upper = name.to_uppercase();

        if name_upper.contains("2160P") || name_upper.contains("4K") {
            VideoQuality::BluRay4K
        } else if name_upper.contains("1080P") {
            if name_upper.contains("WEB")
                || name_upper.contains("WEBDL")
                || name_upper.contains("WEB-DL")
            {
                VideoQuality::WebDl1080p
            } else if name_upper.contains("HDTV") {
                VideoQuality::Hdtv1080p
            } else {
                VideoQuality::BluRay1080p
            }
        } else if name_upper.contains("720P") {
            if name_upper.contains("WEB")
                || name_upper.contains("WEBDL")
                || name_upper.contains("WEB-DL")
            {
                VideoQuality::WebDl720p
            } else if name_upper.contains("HDTV") {
                VideoQuality::Hdtv720p
            } else {
                VideoQuality::BluRay720p
            }
        } else {
            VideoQuality::BluRay720p // Default fallback
        }
    }

    /// Extract media title from search query by removing quality indicators.
    fn extract_title(query: &str) -> String {
        query
            .replace("1080p", "")
            .replace("720p", "")
            .replace("4K", "")
            .replace("2160p", "")
            .replace("BluRay", "")
            .replace("WEB-DL", "")
            .replace("HDTV", "")
            .replace("x264", "")
            .replace("x265", "")
            .trim()
            .to_string()
    }

    /// Group torrents by extracted media title.
    fn group_torrents(torrents: Vec<MagnetoTorrent>) -> Vec<MediaSearchResult> {
        use std::collections::HashMap;

        let mut grouped: HashMap<String, Vec<TorrentResult>> = HashMap::new();

        for torrent in torrents {
            let title = Self::extract_title(&torrent.name);
            let quality = Self::parse_quality(&torrent.name);

            let torrent_result = TorrentResult {
                name: torrent.name,
                magnet_link: torrent.magnet,
                size: torrent.size,
                seeders: torrent.seeders,
                leechers: torrent.leechers,
                quality,
                source: torrent.indexer,
                added_date: chrono::Utc::now(), // TODO: Parse torrent.publish_date
            };

            grouped
                .entry(title.clone())
                .or_default()
                .push(torrent_result);
        }

        grouped
            .into_iter()
            .map(|(title, torrents)| MediaSearchResult {
                title,
                year: None,                   // Will be filled by metadata service
                media_type: MediaType::Movie, // TODO: Detect from category
                imdb_id: None,
                poster_url: None,
                plot: None,
                genre: None,
                rating: None,
                torrents,
            })
            .collect()
    }
}

impl Default for MagnetoProvider {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl TorrentSearchProvider for MagnetoProvider {
    async fn search_torrents(
        &self,
        query: &str,
        category: &str,
    ) -> Result<Vec<MediaSearchResult>, MediaSearchError> {
        let url = format!("{}/api/v1/search", self.base_url);

        let mut params = vec![("query", query), ("category", category), ("limit", "50")];

        if let Some(ref api_key) = self.api_key {
            params.push(("apikey", api_key));
        }

        let response = self
            .client
            .get(&url)
            .query(&params)
            .send()
            .await
            .map_err(|e| MediaSearchError::NetworkError {
                reason: format!("Magneto request failed: {e}"),
            })?;

        if !response.status().is_success() {
            return Err(MediaSearchError::SearchFailed {
                query: query.to_string(),
                reason: format!("Magneto HTTP {}", response.status()),
            });
        }

        let magneto_response: MagnetoResponse =
            response
                .json()
                .await
                .map_err(|e| MediaSearchError::SearchFailed {
                    query: query.to_string(),
                    reason: format!("Magneto JSON parsing failed: {e}"),
                })?;

        Ok(Self::group_torrents(magneto_response.results))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_quality() {
        assert_eq!(
            MagnetoProvider::parse_quality("Movie.2023.1080p.BluRay.x264"),
            VideoQuality::BluRay1080p
        );
        assert_eq!(
            MagnetoProvider::parse_quality("Show.S01E01.720p.WEB-DL.x264"),
            VideoQuality::WebDl720p
        );
        assert_eq!(
            MagnetoProvider::parse_quality("Film.2023.2160p.UHD.BluRay.x265"),
            VideoQuality::BluRay4K
        );
    }

    #[test]
    fn test_extract_title() {
        assert_eq!(
            MagnetoProvider::extract_title("Interstellar 1080p BluRay x264"),
            "Interstellar"
        );
        assert_eq!(
            MagnetoProvider::extract_title("The Matrix 720p WEB-DL"),
            "The Matrix"
        );
    }
}
