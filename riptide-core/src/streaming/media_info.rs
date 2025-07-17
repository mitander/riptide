//! Media container format detection and analysis.
//!
//! This module provides functionality to identify media container formats
//! and determine the appropriate streaming strategy (direct vs remux).
//! It examines file headers to identify formats without requiring full parsing.

use thiserror::Error;

use crate::streaming::remux::types::ContainerFormat;

/// Errors that can occur during media format detection.
#[derive(Debug, Error)]
pub enum MediaInfoError {
    /// Not enough data to determine format.
    ///
    /// Format detection requires at least 12 bytes of file header.
    #[error("insufficient data for format detection: need at least 12 bytes, got {0}")]
    InsufficientData(usize),

    /// The file format could not be identified.
    #[error("unrecognized media format")]
    UnknownFormat,
}

/// Result type for media info operations.
pub type MediaInfoResult<T> = Result<T, MediaInfoError>;

/// Minimum number of bytes needed for reliable format detection.
pub const MIN_DETECTION_BYTES: usize = 12;

/// Detects the container format from file header bytes.
///
/// This function examines the first few bytes of a media file to identify
/// its container format. It uses magic byte sequences that are unique to
/// each format.
///
/// # Errors
///
/// - `MediaInfoError::InsufficientData` - Less than 12 bytes provided
pub fn detect_container_format(data: &[u8]) -> MediaInfoResult<ContainerFormat> {
    if data.len() < MIN_DETECTION_BYTES {
        return Err(MediaInfoError::InsufficientData(data.len()));
    }

    // MP4/M4V/MOV detection
    if data.len() >= 8 {
        let atom_type = &data[4..8];
        if matches!(
            atom_type,
            b"ftyp" | b"moov" | b"mdat" | b"free" | b"skip" | b"wide"
        ) {
            // Further distinguish between MP4 and MOV
            if data.len() >= 12 {
                let brand = &data[8..12];
                if brand.starts_with(b"qt") {
                    return Ok(ContainerFormat::Mov);
                }
            }
            return Ok(ContainerFormat::Mp4);
        }
    }

    // WebM detection (EBML header with webm doctype)
    if data.starts_with(&[0x1A, 0x45, 0xDF, 0xA3]) {
        // This is EBML, check if it's WebM specifically
        if data.len() >= 40 {
            // Look for "webm" doctype in the EBML header
            for window in data[24..40].windows(4) {
                if window == b"webm" {
                    return Ok(ContainerFormat::WebM);
                }
            }
        }
        // EBML but not WebM, likely Matroska
        return Ok(ContainerFormat::Mkv);
    }

    // AVI detection
    if data.starts_with(b"RIFF") && data.len() >= 12 && &data[8..12] == b"AVI " {
        return Ok(ContainerFormat::Avi);
    }

    Ok(ContainerFormat::Unknown)
}

/// Determines if a container format requires remuxing for browser streaming.
///
/// MP4 and WebM can be streamed directly to browsers, while other
/// formats need to be remuxed to fragmented MP4.
pub fn requires_remuxing(format: &ContainerFormat) -> bool {
    !matches!(format, ContainerFormat::Mp4 | ContainerFormat::WebM)
}

/// Returns the MIME type for a container format.
///
/// This is used for the Content-Type header in HTTP responses.
pub fn mime_type(format: &ContainerFormat) -> &'static str {
    match format {
        ContainerFormat::Mp4 => "video/mp4",
        ContainerFormat::WebM => "video/webm",
        ContainerFormat::Mkv => "video/x-matroska",
        ContainerFormat::Avi => "video/x-msvideo",
        ContainerFormat::Mov => "video/quicktime",
        ContainerFormat::Unknown => "application/octet-stream",
    }
}

/// Returns the file extension typically associated with a format.
pub fn extension(format: &ContainerFormat) -> &'static str {
    match format {
        ContainerFormat::Mp4 => "mp4",
        ContainerFormat::WebM => "webm",
        ContainerFormat::Mkv => "mkv",
        ContainerFormat::Avi => "avi",
        ContainerFormat::Mov => "mov",
        ContainerFormat::Unknown => "bin",
    }
}

/// Extracts basic media information from file data.
///
/// This is a convenience struct that combines format detection with
/// other media properties that might be useful for streaming decisions.
pub struct MediaInfo {
    /// The detected container format.
    pub format: ContainerFormat,
    /// Whether remuxing is required for browser compatibility.
    pub requires_remuxing: bool,
    /// Suggested MIME type for HTTP responses.
    pub mime_type: &'static str,
}

impl MediaInfo {
    /// Creates media info from file header bytes.
    ///
    /// # Errors
    ///
    /// - `MediaInfoError::InsufficientData` - Less than 12 bytes provided
    pub fn from_bytes(data: &[u8]) -> MediaInfoResult<Self> {
        let format = detect_container_format(data)?;
        Ok(Self {
            format: format.clone(),
            requires_remuxing: requires_remuxing(&format),
            mime_type: mime_type(&format),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mp4_detection() {
        // Minimal MP4 header with ftyp box
        let mp4_data = [
            0x00, 0x00, 0x00, 0x20, // box size
            b'f', b't', b'y', b'p', // box type
            b'i', b's', b'o', b'm', // major brand
        ];

        let format = detect_container_format(&mp4_data).unwrap();
        assert_eq!(format, ContainerFormat::Mp4);
        assert!(!requires_remuxing(&format));
        assert_eq!(mime_type(&format), "video/mp4");
    }

    #[test]
    fn test_webm_detection() {
        // EBML header with WebM doctype
        let mut webm_data = vec![
            0x1A, 0x45, 0xDF, 0xA3, // EBML magic
        ];
        webm_data.extend_from_slice(&[0x00; 20]); // padding
        webm_data.extend_from_slice(b"webm");
        webm_data.extend_from_slice(&[0x00; 12]); // more padding

        let format = detect_container_format(&webm_data).unwrap();
        assert_eq!(format, ContainerFormat::WebM);
        assert!(!requires_remuxing(&format));
        assert_eq!(mime_type(&format), "video/webm");
    }

    #[test]
    fn test_mkv_detection() {
        // EBML header without WebM doctype
        let mkv_data = [
            0x1A, 0x45, 0xDF, 0xA3, // EBML magic
            0x93, 0x42, 0x86, 0x81, // EBML version
            0x01, 0x42, 0xF7, 0x81, // more EBML data
        ];

        let format = detect_container_format(&mkv_data).unwrap();
        assert_eq!(format, ContainerFormat::Mkv);
        assert!(requires_remuxing(&format));
        assert_eq!(mime_type(&format), "video/x-matroska");
    }

    #[test]
    fn test_avi_detection() {
        let avi_data = [
            b'R', b'I', b'F', b'F', // RIFF header
            0x00, 0x00, 0x00, 0x00, // file size
            b'A', b'V', b'I', b' ', // AVI type
        ];

        let format = detect_container_format(&avi_data).unwrap();
        assert_eq!(format, ContainerFormat::Avi);
        assert!(requires_remuxing(&format));
        assert_eq!(mime_type(&format), "video/x-msvideo");
    }

    #[test]
    fn test_mov_detection() {
        // MOV with QuickTime brand
        let mov_data = [
            0x00, 0x00, 0x00, 0x20, // box size
            b'f', b't', b'y', b'p', // box type
            b'q', b't', b' ', b' ', // QuickTime brand
        ];

        let format = detect_container_format(&mov_data).unwrap();
        assert_eq!(format, ContainerFormat::Mov);
        assert!(requires_remuxing(&format));
        assert_eq!(mime_type(&format), "video/quicktime");
    }

    #[test]
    fn test_unknown_format() {
        let unknown_data = [0xFF; 20];
        let format = detect_container_format(&unknown_data).unwrap();
        assert_eq!(format, ContainerFormat::Unknown);
        assert!(requires_remuxing(&format));
    }

    #[test]
    fn test_insufficient_data() {
        let short_data = [0x00; 8];
        let result = detect_container_format(&short_data);
        assert!(matches!(result, Err(MediaInfoError::InsufficientData(8))));
    }

    #[test]
    fn test_media_info_creation() {
        let mp4_data = [
            0x00, 0x00, 0x00, 0x20, b'f', b't', b'y', b'p', b'i', b's', b'o', b'm',
        ];

        let info = MediaInfo::from_bytes(&mp4_data).unwrap();
        assert_eq!(info.format, ContainerFormat::Mp4);
        assert!(!info.requires_remuxing);
        assert_eq!(info.mime_type, "video/mp4");
    }

    #[test]
    fn test_format_extensions() {
        assert_eq!(extension(&ContainerFormat::Mp4), "mp4");
        assert_eq!(extension(&ContainerFormat::WebM), "webm");
        assert_eq!(extension(&ContainerFormat::Mkv), "mkv");
        assert_eq!(extension(&ContainerFormat::Avi), "avi");
        assert_eq!(extension(&ContainerFormat::Mov), "mov");
        assert_eq!(extension(&ContainerFormat::Unknown), "bin");
    }
}
