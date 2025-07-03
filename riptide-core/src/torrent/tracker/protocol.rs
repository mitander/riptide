//! BitTorrent tracker protocol utilities and constants

/// BitTorrent tracker protocol constants
pub mod constants {
    /// Default tracker announce interval in seconds
    pub const DEFAULT_ANNOUNCE_INTERVAL: u32 = 1800;

    /// Minimum announce interval in seconds
    pub const MIN_ANNOUNCE_INTERVAL: u32 = 60;

    /// Compact peer response format (6 bytes per peer)
    pub const COMPACT_PEER_SIZE: usize = 6;

    /// Standard BitTorrent port range
    pub const DEFAULT_PORT_RANGE: std::ops::Range<u16> = 6881..6889;
}

/// URL encoding utilities for tracker communication
pub mod encoding {
    /// Encode bytes for tracker URL parameters
    pub fn url_encode_bytes(bytes: &[u8]) -> String {
        bytes.iter().map(|&b| format!("%{b:02X}")).collect()
    }

    /// Decode URL-encoded bytes
    ///
    /// # Errors
    /// - Returns error for invalid URL encoding or malformed hex sequences
    pub fn url_decode_bytes(encoded: &str) -> Result<Vec<u8>, String> {
        let mut bytes = Vec::new();
        let mut chars = encoded.chars().peekable();

        while let Some(ch) = chars.next() {
            if ch == '%' {
                let hex_str: String = chars.by_ref().take(2).collect();
                if hex_str.len() != 2 {
                    return Err("Invalid URL encoding".to_string());
                }
                let byte = u8::from_str_radix(&hex_str, 16).map_err(|_| "Invalid hex digit")?;
                bytes.push(byte);
            } else {
                bytes.push(ch as u8);
            }
        }

        Ok(bytes)
    }
}

#[cfg(test)]
mod protocol_tests {
    use super::*;

    #[test]
    fn test_url_encoding() {
        let input = b"Hello World!";
        let encoded = encoding::url_encode_bytes(input);
        assert_eq!(encoded, "%48%65%6C%6C%6F%20%57%6F%72%6C%64%21");

        let decoded = encoding::url_decode_bytes(&encoded).unwrap();
        assert_eq!(decoded, input);
    }

    #[test]
    fn test_url_encoding_binary_data() {
        let input = [0x00, 0xFF, 0x7F, 0x80, 0x01];
        let encoded = encoding::url_encode_bytes(&input);
        assert_eq!(encoded, "%00%FF%7F%80%01");

        let decoded = encoding::url_decode_bytes(&encoded).unwrap();
        assert_eq!(decoded, input);
    }

    #[test]
    fn test_invalid_url_decoding() {
        assert!(encoding::url_decode_bytes("%G0").is_err());
        assert!(encoding::url_decode_bytes("%1").is_err());
        assert!(encoding::url_decode_bytes("%").is_err());
    }

    #[test]
    fn test_protocol_constants() {
        assert_eq!(constants::DEFAULT_ANNOUNCE_INTERVAL, 1800);
        assert_eq!(constants::MIN_ANNOUNCE_INTERVAL, 60);
        assert_eq!(constants::COMPACT_PEER_SIZE, 6);
        assert!(constants::DEFAULT_PORT_RANGE.contains(&6881));
        assert!(constants::DEFAULT_PORT_RANGE.contains(&6888));
        assert!(!constants::DEFAULT_PORT_RANGE.contains(&6889));
    }
}
