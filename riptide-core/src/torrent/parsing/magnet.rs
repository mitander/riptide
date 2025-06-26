//! Magnet link parsing utilities

use super::types::MagnetLink;
use crate::torrent::{InfoHash, TorrentError};

/// Magnet link parsing utilities.
pub struct MagnetParser;

impl MagnetParser {
    /// Parses magnet link to extract torrent information.
    ///
    /// # Errors
    /// - `TorrentError::InvalidTorrentFile` - Malformed magnet URI
    pub fn parse_magnet_link(magnet_url: &str) -> Result<MagnetLink, TorrentError> {
        let magnet =
            magnet_url::Magnet::new(magnet_url).map_err(|e| TorrentError::InvalidTorrentFile {
                reason: format!("Invalid magnet link: {e}"),
            })?;

        // Extract info hash from exact topic (xt) parameter
        let info_hash = Self::extract_info_hash_from_magnet(&magnet)?;

        Ok(MagnetLink {
            info_hash,
            display_name: magnet.display_name().map(|s| s.to_string()),
            trackers: magnet.trackers().to_vec(),
        })
    }

    /// Extract info hash from magnet link
    fn extract_info_hash_from_magnet(
        magnet: &magnet_url::Magnet,
    ) -> Result<InfoHash, TorrentError> {
        // Simple approach: parse the URL string manually since API methods are unclear
        let url_str = magnet.to_string();

        // Look for xt parameter with btih hash
        for param in url_str.split('&') {
            if let Some(xt_value) = param.strip_prefix("xt=urn:btih:") {
                // Also handle the case where it appears after '?'
                return Self::parse_hash_from_string(xt_value);
            }
        }

        // Also check after the initial '?'
        for param in url_str.split('?').skip(1) {
            for sub_param in param.split('&') {
                if let Some(xt_value) = sub_param.strip_prefix("xt=urn:btih:") {
                    return Self::parse_hash_from_string(xt_value);
                }
            }
        }

        Err(TorrentError::InvalidTorrentFile {
            reason: format!("Missing or invalid info hash in magnet link: {url_str}"),
        })
    }

    /// Parse hex string to 20-byte hash
    fn parse_hash_from_string(hash_str: &str) -> Result<InfoHash, TorrentError> {
        if hash_str.len() == 40 {
            let mut hash = [0u8; 20];
            for (i, chunk) in hash_str.as_bytes().chunks(2).enumerate() {
                if i >= 20 {
                    break;
                }
                if let Ok(byte) = u8::from_str_radix(&String::from_utf8_lossy(chunk), 16) {
                    hash[i] = byte;
                } else {
                    return Err(TorrentError::InvalidTorrentFile {
                        reason: format!("Invalid hex character in hash: {hash_str}"),
                    });
                }
            }
            Ok(InfoHash::new(hash))
        } else {
            Err(TorrentError::InvalidTorrentFile {
                reason: format!("Invalid hash length: {} (expected 40)", hash_str.len()),
            })
        }
    }
}
