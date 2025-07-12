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
    ///
    /// - `TorrentError::InvalidTorrentFile` - If bencode parsing or metadata extraction failed
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
        if !pieces_bytes.len().is_multiple_of(20) {
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
    ///
    /// - `TorrentError::InvalidTorrentFile` - If invalid bencode dictionary format
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

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;

    #[test]
    fn test_find_bencode_dictionary_end_simple() {
        // Simple dictionary: d3:keyi42ee
        let bencode_data = b"d3:keyi42ee";
        let end = BencodeParser::find_bencode_dictionary_end(bencode_data).unwrap();
        assert_eq!(end, bencode_data.len());
    }

    #[test]
    fn test_find_bencode_dictionary_end_nested() {
        // Nested dictionary: d3:keyd4:namei42eee
        let bencode_data = b"d3:keyd4:namei42eee";
        let end = BencodeParser::find_bencode_dictionary_end(bencode_data).unwrap();
        assert_eq!(end, bencode_data.len());
    }

    #[test]
    fn test_find_bencode_dictionary_end_with_strings() {
        // Dictionary with strings: d4:name4:test5:valuei123ee
        let bencode_data = b"d4:name4:test5:valuei123ee";
        let end = BencodeParser::find_bencode_dictionary_end(bencode_data).unwrap();
        assert_eq!(end, bencode_data.len());
    }

    #[test]
    fn test_find_bencode_dictionary_end_with_list() {
        // Dictionary with list: d4:listl4:testi42eee
        let bencode_data = b"d4:listl4:testi42eee";
        let end = BencodeParser::find_bencode_dictionary_end(bencode_data).unwrap();
        assert_eq!(end, bencode_data.len());
    }

    #[test]
    fn find_bencode_dictionary_end_invalid_start_test() {
        let bencode_data = b"l4:teste"; // List, not dictionary
        let result = BencodeParser::find_bencode_dictionary_end(bencode_data);
        assert!(result.is_err());
    }

    #[test]
    fn test_find_bencode_dictionary_end_incomplete() {
        let bencode_data = b"d3:key"; // Incomplete dictionary
        let result = BencodeParser::find_bencode_dictionary_end(bencode_data);
        assert!(result.is_err());
    }

    #[test]
    fn test_find_bencode_dictionary_end_invalid_string() {
        let bencode_data = b"d3:key999:"; // Invalid string length
        let result = BencodeParser::find_bencode_dictionary_end(bencode_data);
        assert!(result.is_err());
    }

    #[test]
    fn test_extract_bytes() {
        let mut bencode_dict = HashMap::new();
        bencode_dict.insert(b"test".as_slice(), bencode_rs::Value::Bytes(b"value"));

        let result = BencodeParser::extract_bytes(&bencode_dict, b"test").unwrap();
        assert_eq!(result, b"value");
    }

    #[test]
    fn test_extract_bytes_missing() {
        let bencode_dict = HashMap::new();
        let result = BencodeParser::extract_bytes(&bencode_dict, b"missing");
        assert!(result.is_err());
    }

    #[test]
    fn test_extract_integer() {
        let mut bencode_dict = HashMap::new();
        bencode_dict.insert(b"test".as_slice(), bencode_rs::Value::Integer(42));

        let result = BencodeParser::extract_integer(&bencode_dict, b"test").unwrap();
        assert_eq!(result, 42);
    }

    #[test]
    fn test_extract_integer_missing() {
        let bencode_dict = HashMap::new();
        let result = BencodeParser::extract_integer(&bencode_dict, b"missing");
        assert!(result.is_err());
    }

    #[test]
    fn test_extract_bytes_as_string() {
        let mut bencode_dict = HashMap::new();
        bencode_dict.insert(b"test".as_slice(), bencode_rs::Value::Bytes(b"hello"));

        let result = BencodeParser::extract_bytes_as_string(&bencode_dict, b"test").unwrap();
        assert_eq!(result, "hello");
    }

    #[test]
    fn test_extract_bytes_as_string_invalid_utf8() {
        let mut bencode_dict = HashMap::new();
        bencode_dict.insert(b"test".as_slice(), bencode_rs::Value::Bytes(&[0xFF, 0xFE]));

        let result = BencodeParser::extract_bytes_as_string(&bencode_dict, b"test");
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_bencode_data_empty() {
        let result = BencodeParser::parse_bencode_data(b"");
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_bencode_data_not_dictionary() {
        let bencode_list_data = b"l4:teste"; // List instead of dictionary
        let result = BencodeParser::parse_bencode_data(bencode_list_data);
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_bencode_data_missing_info() {
        let bencode_data = b"d8:announce9:test.com:ee"; // Dictionary without info
        let result = BencodeParser::parse_bencode_data(bencode_data);
        assert!(result.is_err());
    }

    fn create_minimal_torrent_data() -> Vec<u8> {
        // Create a minimal valid torrent structure
        // d8:announce9:test.com:4:infod4:name9:test.file12:piece lengthi32768e6:pieces20:xxxxxxxxxxxxxxxxxxxx6:lengthi1048576eee
        let torrent = "d8:announce9:test.com:4:infod4:name9:test.file12:piece lengthi32768e6:pieces20:\x01\x01\x01\x01\x01\x01\x01\x01\x01\x01\x01\x01\x01\x01\x01\x01\x01\x01\x01\x016:lengthi1048576eee";
        torrent.as_bytes().to_vec()
    }

    #[test]
    fn test_parse_bencode_data_minimal_valid() {
        let torrent_data = create_minimal_torrent_data();
        let result = BencodeParser::parse_bencode_data(&torrent_data);

        assert!(result.is_ok());
        let metadata = result.unwrap();
        assert_eq!(metadata.name, "test.file");
        assert_eq!(metadata.piece_length, 32768);
        assert_eq!(metadata.total_length, 1048576);
        assert_eq!(metadata.piece_hashes.len(), 1);
        assert_eq!(metadata.files.len(), 1);
        assert_eq!(metadata.announce_urls, vec!["test.com:"]);
    }

    #[test]
    fn test_parse_bencode_data_invalid_pieces_length() {
        // Create torrent with invalid pieces length (not multiple of 20)
        let torrent = "d8:announce9:test.com:4:infod4:name9:test.file12:piece lengthi32768e6:pieces19:\x01\x01\x01\x01\x01\x01\x01\x01\x01\x01\x01\x01\x01\x01\x01\x01\x01\x01\x016:lengthi1048576eee";
        let result = BencodeParser::parse_bencode_data(torrent.as_bytes());
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_bencode_data_multifile_simple() {
        // Test the multifile parsing logic without complex bencode construction
        // This test focuses on the extract_files_info function
        let file1_dict = {
            let mut file_dict = HashMap::new();
            file_dict.insert(b"length".as_slice(), bencode_rs::Value::Integer(524288));
            file_dict.insert(
                b"path".as_slice(),
                bencode_rs::Value::List(vec![bencode_rs::Value::Bytes(b"file1.txt")]),
            );
            bencode_rs::Value::Dictionary(file_dict)
        };

        let file2_dict = {
            let mut file_dict = HashMap::new();
            file_dict.insert(b"length".as_slice(), bencode_rs::Value::Integer(1048576));
            file_dict.insert(
                b"path".as_slice(),
                bencode_rs::Value::List(vec![bencode_rs::Value::Bytes(b"file2.dat")]),
            );
            bencode_rs::Value::Dictionary(file_dict)
        };

        let files_list = vec![file1_dict, file2_dict];
        let result = BencodeParser::extract_files_info(&files_list);

        assert!(result.is_ok());
        let (files, total_length) = result.unwrap();
        assert_eq!(total_length, 524288 + 1048576);
        assert_eq!(files.len(), 2);
        assert_eq!(files[0].path, vec!["file1.txt"]);
        assert_eq!(files[0].length, 524288);
        assert_eq!(files[1].path, vec!["file2.dat"]);
        assert_eq!(files[1].length, 1048576);
    }

    #[test]
    fn test_extract_files_info_invalid_file_entry() {
        let invalid_file = bencode_rs::Value::Integer(42); // Invalid type
        let files_list = vec![invalid_file];

        let result = BencodeParser::extract_files_info(&files_list);
        assert!(result.is_err());
    }

    #[test]
    fn test_extract_announce_urls_no_announce() {
        let bencode_dict = HashMap::new();
        let result = BencodeParser::extract_announce_urls(&bencode_dict);
        assert!(result.is_err());
    }

    fn create_torrent_with_announce_list() -> Vec<u8> {
        // Create torrent with announce-list
        let torrent = "d8:announce9:test.com:13:announce-listll9:test.com:el11:backup.com:ee4:infod4:name9:test.file12:piece lengthi32768e6:pieces20:\x01\x01\x01\x01\x01\x01\x01\x01\x01\x01\x01\x01\x01\x01\x01\x01\x01\x01\x01\x016:lengthi1048576eee";
        torrent.as_bytes().to_vec()
    }

    #[test]
    fn test_parse_bencode_data_with_announce_list() {
        let torrent_data = create_torrent_with_announce_list();
        let result = BencodeParser::parse_bencode_data(&torrent_data);

        assert!(result.is_ok());
        let metadata = result.unwrap();
        assert_eq!(metadata.announce_urls.len(), 3);
        assert!(metadata.announce_urls.contains(&"test.com:".to_string()));
        assert!(metadata.announce_urls.contains(&"backup.com:".to_string()));
    }

    #[test]
    fn test_calculate_info_hash_missing_info() {
        let bencode_data = b"d8:announce9:test.com:e"; // No info dictionary
        let result = BencodeParser::parse_bencode_data(bencode_data);
        assert!(result.is_err());
    }
}
