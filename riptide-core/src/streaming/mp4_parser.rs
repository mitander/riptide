//! Media metadata parsing utilities for progressive streaming

use tracing;

/// Extract duration from various media formats
pub fn extract_duration_from_media(data: &[u8]) -> Option<f64> {
    // Try MP4 first
    if let Some(duration) = extract_duration_from_mp4(data) {
        tracing::debug!("Extracted MP4 duration: {} seconds", duration);
        return Some(duration);
    }

    // Try AVI
    if let Some(duration) = extract_duration_from_avi(data) {
        tracing::debug!("Extracted AVI duration: {} seconds", duration);
        return Some(duration);
    }

    tracing::debug!("Could not extract duration from media data");
    None
}

/// Extract duration from MP4 file by parsing moov atom
pub fn extract_duration_from_mp4(data: &[u8]) -> Option<f64> {
    // Find moov atom
    if let Some(moov_data) = find_atom(data, b"moov") {
        // Find mvhd atom within moov
        if let Some(mvhd_data) = find_atom(&moov_data, b"mvhd") {
            return parse_mvhd_duration(&mvhd_data);
        }
    }

    None
}

/// Extract duration from AVI file by parsing main header
pub fn extract_duration_from_avi(data: &[u8]) -> Option<f64> {
    // AVI files start with RIFF header
    if data.len() < 12 || &data[0..4] != b"RIFF" || &data[8..12] != b"AVI " {
        return None;
    }

    // Find the main AVI header (hdrl LIST)
    let mut pos = 12;
    while pos + 8 < data.len() {
        let chunk_id = &data[pos..pos + 4];
        let chunk_size =
            u32::from_le_bytes([data[pos + 4], data[pos + 5], data[pos + 6], data[pos + 7]])
                as usize;

        if chunk_id == b"LIST" && pos + 12 < data.len() && &data[pos + 8..pos + 12] == b"hdrl" {
            // Found hdrl LIST, look for avih chunk
            let mut hdrl_pos = pos + 12;
            while hdrl_pos + 8 < pos + chunk_size && hdrl_pos + 8 < data.len() {
                let sub_chunk_id = &data[hdrl_pos..hdrl_pos + 4];
                let sub_chunk_size = u32::from_le_bytes([
                    data[hdrl_pos + 4],
                    data[hdrl_pos + 5],
                    data[hdrl_pos + 6],
                    data[hdrl_pos + 7],
                ]) as usize;

                if sub_chunk_id == b"avih" && hdrl_pos + 8 + 32 <= data.len() {
                    // Found AVI main header
                    let header_start = hdrl_pos + 8;

                    // Parse microseconds per frame (offset 0)
                    let microseconds_per_frame = u32::from_le_bytes([
                        data[header_start],
                        data[header_start + 1],
                        data[header_start + 2],
                        data[header_start + 3],
                    ]);

                    // Parse total frames (offset 16)
                    let total_frames = u32::from_le_bytes([
                        data[header_start + 16],
                        data[header_start + 17],
                        data[header_start + 18],
                        data[header_start + 19],
                    ]);

                    if microseconds_per_frame > 0 && total_frames > 0 {
                        let duration =
                            (total_frames as f64 * microseconds_per_frame as f64) / 1_000_000.0;

                        // Sanity check - AVI duration should be reasonable (1 second to 24 hours)
                        if (1.0..=86400.0).contains(&duration) {
                            return Some(duration);
                        } else {
                            tracing::warn!(
                                "AVI duration {} seconds seems unreasonable, ignoring",
                                duration
                            );
                        }
                    }
                }

                hdrl_pos += 8 + sub_chunk_size;
                // Align to 2-byte boundary
                if hdrl_pos % 2 == 1 {
                    hdrl_pos += 1;
                }
            }
        }

        pos += 8 + chunk_size;
        // Align to 2-byte boundary
        if pos % 2 == 1 {
            pos += 1;
        }
    }

    None
}

/// Find an MP4 atom by its type
fn find_atom(data: &[u8], atom_type: &[u8; 4]) -> Option<Vec<u8>> {
    let mut pos = 0;

    while pos + 8 <= data.len() {
        // Read atom size (4 bytes, big endian)
        let size =
            u32::from_be_bytes([data[pos], data[pos + 1], data[pos + 2], data[pos + 3]]) as usize;

        // Read atom type (4 bytes)
        let atom_type_bytes = &data[pos + 4..pos + 8];

        if atom_type_bytes == atom_type {
            // Found the atom, return its data (excluding header)
            let atom_data = &data[pos + 8..pos + size];
            return Some(atom_data.to_vec());
        }

        // Move to next atom
        pos += size;

        // Safety check to prevent infinite loop
        if size == 0 {
            break;
        }
    }

    None
}

/// Parse duration from mvhd atom
fn parse_mvhd_duration(mvhd_data: &[u8]) -> Option<f64> {
    if mvhd_data.len() < 20 {
        return None;
    }

    // mvhd version (1 byte)
    let version = mvhd_data[0];

    match version {
        0 => {
            // Version 0: 32-bit values
            if mvhd_data.len() < 20 {
                return None;
            }

            // Skip version, flags, creation_time, modification_time (16 bytes)
            let timescale =
                u32::from_be_bytes([mvhd_data[12], mvhd_data[13], mvhd_data[14], mvhd_data[15]]);

            let duration =
                u32::from_be_bytes([mvhd_data[16], mvhd_data[17], mvhd_data[18], mvhd_data[19]]);

            if timescale > 0 {
                Some(duration as f64 / timescale as f64)
            } else {
                None
            }
        }
        1 => {
            // Version 1: 64-bit values
            if mvhd_data.len() < 32 {
                return None;
            }

            // Skip version, flags, creation_time, modification_time (20 bytes)
            let timescale =
                u32::from_be_bytes([mvhd_data[20], mvhd_data[21], mvhd_data[22], mvhd_data[23]]);

            let duration = u64::from_be_bytes([
                mvhd_data[24],
                mvhd_data[25],
                mvhd_data[26],
                mvhd_data[27],
                mvhd_data[28],
                mvhd_data[29],
                mvhd_data[30],
                mvhd_data[31],
            ]);

            if timescale > 0 {
                Some(duration as f64 / timescale as f64)
            } else {
                None
            }
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_find_atom() {
        // Create a simple MP4 atom structure
        let mut data = Vec::new();

        // ftyp atom
        data.extend_from_slice(&16u32.to_be_bytes()); // size
        data.extend_from_slice(b"ftyp"); // type
        data.extend_from_slice(&[0u8; 8]); // data

        // moov atom
        data.extend_from_slice(&20u32.to_be_bytes()); // size
        data.extend_from_slice(b"moov"); // type
        data.extend_from_slice(&[1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12]); // data

        let moov_data = find_atom(&data, b"moov").unwrap();
        assert_eq!(moov_data, vec![1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12]);

        let missing = find_atom(&data, b"mdat");
        assert!(missing.is_none());
    }

    #[test]
    fn test_parse_mvhd_duration_version_0() {
        let mut mvhd_data = Vec::new();

        // Version 0, flags (4 bytes)
        mvhd_data.extend_from_slice(&[0, 0, 0, 0]);

        // Creation time (4 bytes)
        mvhd_data.extend_from_slice(&[0, 0, 0, 0]);

        // Modification time (4 bytes)
        mvhd_data.extend_from_slice(&[0, 0, 0, 0]);

        // Timescale: 1000 (4 bytes)
        mvhd_data.extend_from_slice(&1000u32.to_be_bytes());

        // Duration: 5000 (4 bytes) = 5 seconds
        mvhd_data.extend_from_slice(&5000u32.to_be_bytes());

        let duration = parse_mvhd_duration(&mvhd_data).unwrap();
        assert_eq!(duration, 5.0);
    }

    #[test]
    fn test_extract_duration_from_avi() {
        let mut avi_data = Vec::new();

        // RIFF header
        avi_data.extend_from_slice(b"RIFF");
        avi_data.extend_from_slice(&200u32.to_le_bytes()); // file size
        avi_data.extend_from_slice(b"AVI ");

        // hdrl LIST
        avi_data.extend_from_slice(b"LIST");
        avi_data.extend_from_slice(&100u32.to_le_bytes()); // chunk size
        avi_data.extend_from_slice(b"hdrl");

        // avih chunk
        avi_data.extend_from_slice(b"avih");
        avi_data.extend_from_slice(&56u32.to_le_bytes()); // avih size

        // AVI main header
        avi_data.extend_from_slice(&40000u32.to_le_bytes()); // 40ms per frame (25 fps)
        avi_data.extend_from_slice(&[0u8; 12]); // padding
        avi_data.extend_from_slice(&2500u32.to_le_bytes()); // 2500 frames
        avi_data.extend_from_slice(&[0u8; 32]); // rest of header

        let duration = extract_duration_from_avi(&avi_data).unwrap();
        assert_eq!(duration, 100.0); // 2500 frames * 40ms = 100 seconds
    }
}
