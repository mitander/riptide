//! Magneto torrent search provider using built-in indexers.

use async_trait::async_trait;
use magneto::{Magneto, PirateBay, SearchRequest, Yts};

use super::TorrentSearchProvider;
use crate::errors::MediaSearchError;
use crate::types::{MediaSearchResult, MediaType, TorrentResult, VideoQuality};

/// Magneto search provider using built-in torrent indexers.
///
/// Uses Magneto library with built-in providers (Knaben, PirateBay, YTS)
/// to search for torrents across multiple indexers simultaneously.
/// No external server required - works completely offline.
pub struct MagnetoProvider {
    magneto: Magneto,
}

impl std::fmt::Debug for MagnetoProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MagnetoProvider")
            .field("magneto", &"<Magneto instance>")
            .finish()
    }
}

impl MagnetoProvider {
    /// Create new Magneto provider with working indexers.
    ///
    /// Uses PirateBay and YTS providers (excludes Knaben due to downtime).
    /// No configuration required - works out of the box.
    pub fn new() -> Self {
        let magneto =
            Magneto::with_providers(vec![Box::new(PirateBay::new()), Box::new(Yts::new())]);

        Self { magneto }
    }

    /// Create Magneto provider with custom configuration.
    ///
    /// For compatibility with existing interface.
    /// Ignores URL and API key since Magneto library handles provider connections.
    pub fn with_config(_base_url: String, _api_key: Option<String>) -> Self {
        Self::new()
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
    fn group_torrents(torrents: Vec<magneto::Torrent>) -> Vec<MediaSearchResult> {
        use std::collections::HashMap;

        let mut grouped: HashMap<String, Vec<TorrentResult>> = HashMap::new();

        for torrent in torrents {
            let title = Self::extract_title(&torrent.name);
            let quality = Self::parse_quality(&torrent.name);

            let torrent_result = TorrentResult {
                name: torrent.name,
                magnet_link: torrent.magnet_link,
                size: torrent.size_bytes,
                seeders: torrent.seeders,
                leechers: torrent.peers,
                quality,
                source: torrent.provider,
                added_date: chrono::Utc::now(),
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
        _category: &str,
    ) -> Result<Vec<MediaSearchResult>, MediaSearchError> {
        let request = SearchRequest::new(query);

        let torrents =
            self.magneto
                .search(request)
                .await
                .map_err(|e| MediaSearchError::NetworkError {
                    reason: format!("Magneto search failed: {e}"),
                })?;

        Ok(Self::group_torrents(torrents))
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
