//! Utility functions for HTTP handling and data parsing

/// Parse HTTP Range header for video streaming
pub fn parse_range_header(range: &str, total_size: u64) -> (u64, u64, u64) {
    if !range.starts_with("bytes=") {
        return (0, total_size.saturating_sub(1), total_size);
    }

    let range_spec = &range[6..]; // Remove "bytes="
    if let Some((start_str, end_str)) = range_spec.split_once('-') {
        let start = start_str.parse::<u64>().unwrap_or(0);
        let end = if end_str.is_empty() {
            total_size.saturating_sub(1)
        } else {
            end_str
                .parse::<u64>()
                .unwrap_or(total_size.saturating_sub(1))
        };
        let _content_length = end.saturating_sub(start) + 1;
        (start, end, _content_length)
    } else {
        (0, total_size.saturating_sub(1), total_size)
    }
}

/// Create fake video data for demo streaming
pub fn create_fake_video_segment(start: u64, length: u64) -> Vec<u8> {
    // Create fake MP4-like data with proper headers for demo
    let mut data = Vec::new();

    // Simple pattern that browsers might recognize as video
    for i in 0..length {
        let byte = ((start + i) % 256) as u8;
        data.push(byte);
    }

    data
}

/// Parse hex string into InfoHash
pub fn parse_info_hash(hex_str: &str) -> Result<riptide_core::torrent::InfoHash, String> {
    if hex_str.len() != 40 {
        return Err("Invalid hash length".to_string());
    }

    let hash_bytes = decode_hex(hex_str)?;
    if hash_bytes.len() != 20 {
        return Err("Invalid hash bytes".to_string());
    }

    let mut hash_array = [0u8; 20];
    hash_array.copy_from_slice(&hash_bytes);
    Ok(riptide_core::torrent::InfoHash::new(hash_array))
}

/// Simple hex decoder
pub fn decode_hex(hex_str: &str) -> Result<Vec<u8>, String> {
    if hex_str.len() % 2 != 0 {
        return Err("Invalid hex string length".to_string());
    }

    let mut bytes = Vec::new();
    for chunk in hex_str.as_bytes().chunks(2) {
        let hex_byte = std::str::from_utf8(chunk).map_err(|_| "Invalid UTF-8")?;
        let byte = u8::from_str_radix(hex_byte, 16).map_err(|_| "Invalid hex digit")?;
        bytes.push(byte);
    }

    Ok(bytes)
}
