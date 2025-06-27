//! Bencode parsing logic and info hash calculation

use sha1::{Digest, Sha1};

use super::types::{TorrentFile, TorrentMetadata};
use crate::torrent::{InfoHash, TorrentError};

// Type aliases for complex bencode types
pub(super) type BencodeDict<'a> = std::collections::HashMap<&'a [u8], bencode_rs::Value<'a>>;
pub(super) type ParseResult<T> = Result<T, TorrentError>;
pub(super) type BytesResult<'a> = Result<&'a [u8], TorrentError>;
pub(super) type FilesResult = ParseResult<(Vec<TorrentFile>, u64)>;

/// Bencode parsing utilities for torrent metadata extraction.
pub struct BencodeParser;

impl BencodeParser {
    /// Parse bencode data and extract torrent metadata
    ///
    /// # Errors
    /// - `TorrentError::InvalidTorrentFile` - Bencode parsing or metadata extraction failed
    pub fn parse_bencode_data(torrent_bytes: &[u8]) -> Result<TorrentMetadata, TorrentError> {
        let parsed = bencode_rs::Value::parse(torrent_bytes).map_err(|e| {
            TorrentError::InvalidTorrentFile {
                reason: format!("Bencode parsing failed: {e:?}"),
            }
        })?;

        if parsed.is_empty() {
            return Err(TorrentError::InvalidTorrentFile {
                reason: "Empty bencode data".to_string(),
            });
        }

        let root = &parsed[0];
        if let bencode_rs::Value::Dictionary(dict) = root {
            Self::extract_metadata_from_dict(dict, torrent_bytes)
        } else {
            Err(TorrentError::InvalidTorrentFile {
                reason: "Root element must be dictionary".to_string(),
            })
        }
    }

    /// Extract torrent metadata from bencode dictionary
    fn extract_metadata_from_dict(
        dict: &BencodeDict<'_>,
        original_data: &[u8],
    ) -> ParseResult<TorrentMetadata> {
        let info_dict =
            dict.get(b"info".as_slice())
                .ok_or_else(|| TorrentError::InvalidTorrentFile {
                    reason: "Missing 'info' field".to_string(),
                })?;

        let info_hash = Self::calculate_info_hash(info_dict, original_data)?;

        let bencode_rs::Value::Dictionary(info_dict_map) = info_dict else {
            return Err(TorrentError::InvalidTorrentFile {
                reason: "Info field must be dictionary".to_string(),
            });
        };

        let name = Self::extract_bytes_as_string(info_dict_map, b"name")?;
        let piece_length = Self::extract_integer(info_dict_map, b"piece length")? as u32;

        let pieces_bytes = Self::extract_bytes(info_dict_map, b"pieces")?;
        if pieces_bytes.len() % 20 != 0 {
            return Err(TorrentError::InvalidTorrentFile {
                reason: "Invalid pieces length".to_string(),
            });
        }

        let piece_hashes: Vec<[u8; 20]> = pieces_bytes
            .chunks(20)
            .map(|chunk| {
                let mut hash = [0u8; 20];
                hash.copy_from_slice(chunk);
                hash
            })
            .collect();

        let (files, total_length) =
            if let Ok(length) = Self::extract_integer(info_dict_map, b"length") {
                let files = vec![TorrentFile {
                    path: vec![name.clone()],
                    length: length as u64,
                }];
                (files, length as u64)
            } else if let Ok(bencode_rs::Value::List(files_list)) = info_dict_map
                .get(b"files".as_slice())
                .ok_or_else(|| TorrentError::InvalidTorrentFile {
                    reason: "Missing 'files' or 'length' field".to_string(),
                })
            {
                Self::extract_files_info(files_list)?
            } else {
                return Err(TorrentError::InvalidTorrentFile {
                    reason: "Invalid files structure".to_string(),
                });
            };

        let announce_urls = Self::extract_announce_urls(dict)?;

        Ok(TorrentMetadata {
            info_hash,
            name,
            piece_length,
            piece_hashes,
            total_length,
            files,
            announce_urls,
        })
    }

    /// Calculate SHA1 hash of the info dictionary
    fn calculate_info_hash(
        _info_dict: &bencode_rs::Value<'_>,
        original_data: &[u8],
    ) -> Result<InfoHash, TorrentError> {
        // Find the start of the info dictionary in the original data
        let info_start = original_data
            .windows(b"4:info".len())
            .position(|window| window == b"4:info")
            .ok_or_else(|| TorrentError::InvalidTorrentFile {
                reason: "Could not find info dictionary in data".to_string(),
            })?;

        // Skip "4:info" to get to the actual dictionary
        let info_data_start = info_start + 6;

        // Parse from the info dictionary start to find its end
        let info_dict_data = &original_data[info_data_start..];
        let info_dict_end = Self::find_bencode_dictionary_end(info_dict_data)?;

        // Extract the complete info dictionary bencode data
        let info_dict_bytes = &original_data[info_data_start..info_data_start + info_dict_end];

        // Calculate SHA-1 hash of the info dictionary
        let mut hasher = Sha1::new();
        hasher.update(info_dict_bytes);
        let hash_result = hasher.finalize();
        let mut hash = [0u8; 20];
        hash.copy_from_slice(&hash_result);

        Ok(InfoHash::new(hash))
    }

    /// Find the end position of a bencode dictionary
    ///
    /// # Errors
    /// - `TorrentError::InvalidTorrentFile` - Invalid bencode dictionary format
    pub fn find_bencode_dictionary_end(data: &[u8]) -> Result<usize, TorrentError> {
        if data.is_empty() || data[0] != b'd' {
            return Err(TorrentError::InvalidTorrentFile {
                reason: "Expected dictionary start".to_string(),
            });
        }

        let mut pos = 1; // Skip initial 'd'
        let mut depth = 1;

        while pos < data.len() && depth > 0 {
            match data[pos] {
                b'd' | b'l' => {
                    depth += 1;
                    pos += 1;
                }
                b'e' => {
                    depth -= 1;
                    pos += 1;
                }
                b'i' => {
                    // Integer: find 'e'
                    pos += 1;
                    while pos < data.len() && data[pos] != b'e' {
                        pos += 1;
                    }
                    if pos < data.len() {
                        pos += 1; // Skip 'e'
                    }
                }
                b'0'..=b'9' => {
                    // String: read length
                    let start = pos;
                    while pos < data.len() && data[pos] != b':' {
                        pos += 1;
                    }
                    if pos >= data.len() {
                        return Err(TorrentError::InvalidTorrentFile {
                            reason: "Invalid string format".to_string(),
                        });
                    }

                    let length_str = std::str::from_utf8(&data[start..pos]).map_err(|_| {
                        TorrentError::InvalidTorrentFile {
                            reason: "Invalid string length".to_string(),
                        }
                    })?;
                    let length: usize =
                        length_str
                            .parse()
                            .map_err(|_| TorrentError::InvalidTorrentFile {
                                reason: "Invalid string length".to_string(),
                            })?;

                    pos += 1 + length; // Skip ':' and string content
                }
                _ => {
                    return Err(TorrentError::InvalidTorrentFile {
                        reason: "Invalid bencode character".to_string(),
                    });
                }
            }
        }

        if depth != 0 {
            return Err(TorrentError::InvalidTorrentFile {
                reason: "Incomplete bencode dictionary".to_string(),
            });
        }

        Ok(pos)
    }

    /// Extract string from bencode dictionary
    fn extract_bytes_as_string(dict: &BencodeDict<'_>, key: &[u8]) -> ParseResult<String> {
        let bytes = Self::extract_bytes(dict, key)?;
        String::from_utf8(bytes.to_vec()).map_err(|_| TorrentError::InvalidTorrentFile {
            reason: format!("Invalid UTF-8 in field: {:?}", String::from_utf8_lossy(key)),
        })
    }

    /// Extract bytes from bencode dictionary
    fn extract_bytes<'a>(dict: &'a BencodeDict<'_>, key: &[u8]) -> BytesResult<'a> {
        match dict.get(key) {
            Some(bencode_rs::Value::Bytes(bytes)) => Ok(bytes),
            _ => Err(TorrentError::InvalidTorrentFile {
                reason: format!(
                    "Missing or invalid field: {:?}",
                    String::from_utf8_lossy(key)
                ),
            }),
        }
    }

    /// Extract integer from bencode dictionary
    fn extract_integer(dict: &BencodeDict<'_>, key: &[u8]) -> ParseResult<i64> {
        match dict.get(key) {
            Some(bencode_rs::Value::Integer(value)) => Ok(*value),
            _ => Err(TorrentError::InvalidTorrentFile {
                reason: format!(
                    "Missing or invalid integer field: {:?}",
                    String::from_utf8_lossy(key)
                ),
            }),
        }
    }

    /// Extract files information from multi-file torrent
    fn extract_files_info(files_list: &[bencode_rs::Value<'_>]) -> FilesResult {
        let mut files = Vec::new();
        let mut total_length = 0u64;

        for file_value in files_list {
            if let bencode_rs::Value::Dictionary(file_dict) = file_value {
                let length = Self::extract_integer(file_dict, b"length")? as u64;
                total_length += length;

                let path_list = match file_dict.get(b"path".as_slice()) {
                    Some(bencode_rs::Value::List(path_list)) => path_list,
                    _ => {
                        return Err(TorrentError::InvalidTorrentFile {
                            reason: "Missing or invalid path in file".to_string(),
                        });
                    }
                };

                let mut path = Vec::new();
                for path_component in path_list {
                    if let bencode_rs::Value::Bytes(component) = path_component {
                        let component_str =
                            String::from_utf8(component.to_vec()).map_err(|_| {
                                TorrentError::InvalidTorrentFile {
                                    reason: "Invalid UTF-8 in file path".to_string(),
                                }
                            })?;
                        path.push(component_str);
                    } else {
                        return Err(TorrentError::InvalidTorrentFile {
                            reason: "Invalid path component type".to_string(),
                        });
                    }
                }

                files.push(TorrentFile { path, length });
            } else {
                return Err(TorrentError::InvalidTorrentFile {
                    reason: "Invalid file entry type".to_string(),
                });
            }
        }

        Ok((files, total_length))
    }

    /// Extract announce URLs from torrent dictionary
    fn extract_announce_urls(dict: &BencodeDict<'_>) -> ParseResult<Vec<String>> {
        let mut announce_urls = Vec::new();

        // Primary announce URL
        if let Ok(announce) = Self::extract_bytes_as_string(dict, b"announce") {
            announce_urls.push(announce);
        }

        // Announce list (optional)
        if let Some(bencode_rs::Value::List(announce_list)) = dict.get(b"announce-list".as_slice())
        {
            for tier in announce_list {
                if let bencode_rs::Value::List(tier_urls) = tier {
                    for url_value in tier_urls {
                        if let bencode_rs::Value::Bytes(url_bytes) = url_value
                            && let Ok(url) = String::from_utf8(url_bytes.to_vec())
                        {
                            announce_urls.push(url);
                        }
                    }
                }
            }
        }

        if announce_urls.is_empty() {
            return Err(TorrentError::InvalidTorrentFile {
                reason: "No announce URLs found".to_string(),
            });
        }

        Ok(announce_urls)
    }
}
