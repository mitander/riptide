//! Streaming strategy abstraction for handling different container formats

use std::ops::Range;

use async_trait::async_trait;

use crate::torrent::InfoHash;

/// Result type for streaming operations
pub type StreamingResult<T> = Result<T, StreamingError>;

/// Errors that can occur during streaming operations
#[derive(Debug, thiserror::Error)]
pub enum StreamingError {
    #[error("Container format not supported: {format}")]
    UnsupportedFormat { format: String },

    #[error("Failed to detect container format from headers")]
    FormatDetectionFailed,

    #[error("Container remuxing failed: {reason}")]
    RemuxingFailed { reason: String },

    #[error("Piece data unavailable: {info_hash}")]
    PieceDataUnavailable { info_hash: InfoHash },

    #[error("Range request invalid: {range:?}")]
    InvalidRange { range: Range<u64> },

    #[error("Missing pieces for reconstruction: {missing:?} of {total} total")]
    MissingPieces { missing: Vec<u32>, total: u32 },

    #[error("Piece storage error: {reason}")]
    PieceStorageError { reason: String },

    #[error("I/O error during {operation} on {path}: {source}")]
    IoError {
        operation: String,
        path: String,
        #[source]
        source: std::io::Error,
    },

    #[error("FFmpeg error: {reason}")]
    FfmpegError { reason: String },
}

/// Detected container format from file headers
#[derive(Debug, Clone, PartialEq)]
pub enum ContainerFormat {
    Mp4,
    WebM,
    Mkv,
    Avi,
    Mov,
    Unknown,
}

impl ContainerFormat {
    /// Returns whether this format can be streamed directly to browsers
    pub fn is_browser_compatible(&self) -> bool {
        matches!(self, ContainerFormat::Mp4 | ContainerFormat::WebM)
    }

    /// Returns the MIME type for HTTP responses
    pub fn mime_type(&self) -> &'static str {
        match self {
            ContainerFormat::Mp4 => "video/mp4",
            ContainerFormat::WebM => "video/webm",
            ContainerFormat::Mkv => "video/x-matroska",
            ContainerFormat::Avi => "video/x-msvideo",
            ContainerFormat::Mov => "video/quicktime",
            ContainerFormat::Unknown => "video/mp4", // Fallback
        }
    }
}

/// Streaming strategy for handling different container formats
#[async_trait]
pub trait StreamingStrategy: Send + Sync {
    /// Stream video data for the requested byte range
    async fn stream_range(
        &self,
        info_hash: InfoHash,
        range: Range<u64>,
    ) -> StreamingResult<Vec<u8>>;

    /// Get total file size for Content-Length headers
    async fn file_size(&self, info_hash: InfoHash) -> StreamingResult<u64>;

    /// Get container format for Content-Type headers
    async fn container_format(&self, info_hash: InfoHash) -> StreamingResult<ContainerFormat>;

    /// Returns whether this strategy can handle the given format
    fn supports_format(&self, format: &ContainerFormat) -> bool;
}

/// Detects container format from file headers
pub struct ContainerDetector;

impl ContainerDetector {
    /// Detect container format from the first few bytes of file data
    pub fn detect_format(header_bytes: &[u8]) -> ContainerFormat {
        if header_bytes.len() < 16 {
            return ContainerFormat::Unknown;
        }

        // MP4 signatures
        if header_bytes[4..8] == b"ftyp"[..] {
            // Check for MP4 brands
            if header_bytes[8..12] == b"mp41"[..]
                || header_bytes[8..12] == b"mp42"[..]
                || header_bytes[8..12] == b"isom"[..]
                || header_bytes[8..12] == b"avc1"[..]
            {
                return ContainerFormat::Mp4;
            }
        }

        // WebM (EBML + webm doctype)
        if header_bytes.starts_with(&[0x1A, 0x45, 0xDF, 0xA3]) {
            // This is EBML, check if it's WebM specifically
            if let Some(pos) = header_bytes.windows(4).position(|w| w == b"webm")
                && pos < 100
            {
                // Doctype should be near the beginning
                return ContainerFormat::WebM;
            }
            // If EBML but not WebM, it's likely MKV
            return ContainerFormat::Mkv;
        }

        // AVI signature
        if header_bytes.starts_with(b"RIFF") && header_bytes[8..12] == b"AVI "[..] {
            return ContainerFormat::Avi;
        }

        // QuickTime MOV
        if header_bytes[4..8] == b"ftyp"[..] && header_bytes[8..12] == b"qt  "[..] {
            return ContainerFormat::Mov;
        }

        ContainerFormat::Unknown
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_container_format_browser_compatibility() {
        assert!(ContainerFormat::Mp4.is_browser_compatible());
        assert!(ContainerFormat::WebM.is_browser_compatible());
        assert!(!ContainerFormat::Mkv.is_browser_compatible());
        assert!(!ContainerFormat::Avi.is_browser_compatible());
    }

    #[test]
    fn test_container_format_mime_types() {
        assert_eq!(ContainerFormat::Mp4.mime_type(), "video/mp4");
        assert_eq!(ContainerFormat::WebM.mime_type(), "video/webm");
        assert_eq!(ContainerFormat::Mkv.mime_type(), "video/x-matroska");
    }

    #[test]
    fn test_container_detection_mp4() {
        // MP4 with ftyp box
        let mp4_header = [
            0x00, 0x00, 0x00, 0x20, // box size
            b'f', b't', b'y', b'p', // box type 'ftyp'
            b'm', b'p', b'4', b'1', // brand 'mp41'
            0x00, 0x00, 0x00, 0x00, // minor version
        ];

        assert_eq!(
            ContainerDetector::detect_format(&mp4_header),
            ContainerFormat::Mp4
        );
    }

    #[test]
    fn test_container_detection_webm() {
        // WebM EBML header with doctype
        let mut webm_header = vec![
            0x1A, 0x45, 0xDF, 0xA3, // EBML signature
            0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x1F,
        ];
        webm_header.extend_from_slice(b"webm"); // doctype
        webm_header.resize(50, 0); // pad to minimum size

        assert_eq!(
            ContainerDetector::detect_format(&webm_header),
            ContainerFormat::WebM
        );
    }

    #[test]
    fn test_container_detection_mkv() {
        // MKV EBML header without webm doctype
        let mkv_header = [
            0x1A, 0x45, 0xDF, 0xA3, // EBML signature
            0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x1F, 0x42, 0x86, // DocType element
            0x81, 0x01, // size 1
            b'm', // start of "matroska"
            0x00, 0x00,
        ];

        assert_eq!(
            ContainerDetector::detect_format(&mkv_header),
            ContainerFormat::Mkv
        );
    }

    #[test]
    fn test_container_detection_avi() {
        let avi_header = [
            b'R', b'I', b'F', b'F', // RIFF signature
            0x24, 0x00, 0x00, 0x00, // file size
            b'A', b'V', b'I', b' ', // AVI signature
            0x4C, 0x49, 0x53, 0x54, // LIST
        ];

        assert_eq!(
            ContainerDetector::detect_format(&avi_header),
            ContainerFormat::Avi
        );
    }

    #[test]
    fn test_container_detection_unknown() {
        let unknown_header = [0x00; 16];
        assert_eq!(
            ContainerDetector::detect_format(&unknown_header),
            ContainerFormat::Unknown
        );

        // Too short header
        let short_header = [0x00; 8];
        assert_eq!(
            ContainerDetector::detect_format(&short_header),
            ContainerFormat::Unknown
        );
    }
}
