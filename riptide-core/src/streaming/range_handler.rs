//! HTTP range request handling for media streaming

use serde::Serialize;

/// HTTP range request specification.
#[derive(Debug, Clone)]
pub struct RangeRequest {
    pub ranges: Vec<(u64, u64)>, // (start, end) byte ranges
}

/// Response to a range request.
#[derive(Debug, Clone)]
pub struct RangeResponse {
    pub data: Vec<u8>,
    pub start: u64,
    pub end: u64,
    pub total_size: u64,
    pub content_type: String,
}

/// Information about streamable content.
#[derive(Debug, Clone, Serialize)]
pub struct ContentInfo {
    pub name: String,
    pub total_size: u64,
    pub mime_type: Option<String>,
    pub files: Vec<FileInfo>,
    pub piece_size: u32,
    pub total_pieces: u32,
}

/// Information about a file within a torrent.
#[derive(Debug, Clone, Serialize)]
pub struct FileInfo {
    pub name: String,
    pub size: u64,
    pub offset: u64,
    pub mime_type: Option<String>,
    pub is_media: bool,
}

/// Handler for HTTP range requests with torrent-aware optimizations.
///
/// Provides range handling that understands BitTorrent piece boundaries
/// and can optimize downloading for streaming scenarios.
pub struct RangeHandler {
    piece_size: u32,
    total_pieces: u32,
    total_size: u64,
}

impl RangeHandler {
    /// Creates new range handler for torrent content.
    pub fn new(piece_size: u32, total_pieces: u32, total_size: u64) -> Self {
        Self {
            piece_size,
            total_pieces,
            total_size,
        }
    }

    /// Calculate which pieces are needed for a byte range.
    ///
    /// Returns a list of piece indices and the byte ranges within each piece
    /// that are needed to fulfill the request.
    pub fn calculate_required_pieces(&self, start: u64, length: u64) -> Vec<PieceRange> {
        let end = start + length - 1;
        let start_piece = (start / self.piece_size as u64) as u32;
        let end_piece = (end / self.piece_size as u64) as u32;

        let mut pieces = Vec::new();

        for piece_index in start_piece..=end_piece.min(self.total_pieces - 1) {
            let piece_start = piece_index as u64 * self.piece_size as u64;
            let piece_end = if piece_index == self.total_pieces - 1 {
                self.total_size - 1
            } else {
                piece_start + self.piece_size as u64 - 1
            };

            let range_start_in_piece = if piece_index == start_piece {
                start - piece_start
            } else {
                0
            };

            let range_end_in_piece = if piece_index == end_piece {
                end - piece_start
            } else {
                piece_end - piece_start
            };

            pieces.push(PieceRange {
                piece_index,
                start_offset: range_start_in_piece as u32,
                end_offset: range_end_in_piece as u32,
                priority: self.calculate_piece_priority(piece_index, start, length),
            });
        }

        pieces
    }

    /// Calculate streaming buffer requirements for smooth playback.
    ///
    /// Returns the pieces that should be prioritized for download to maintain
    /// smooth streaming without buffer underruns.
    pub fn calculate_streaming_buffer(
        &self,
        current_position: u64,
        bitrate_estimate: u64,
        buffer_duration: std::time::Duration,
    ) -> StreamingBuffer {
        let buffer_bytes = bitrate_estimate * buffer_duration.as_secs() / 8;
        let ahead_position = current_position + buffer_bytes;

        let current_piece = (current_position / self.piece_size as u64) as u32;
        let ahead_piece = (ahead_position / self.piece_size as u64) as u32;

        let critical_pieces = self.calculate_required_pieces(
            current_position,
            buffer_bytes.min(self.piece_size as u64 * 3),
        );

        let prefetch_pieces = if ahead_piece > current_piece {
            self.calculate_required_pieces(
                current_piece as u64 * self.piece_size as u64,
                (ahead_piece - current_piece) as u64 * self.piece_size as u64,
            )
        } else {
            Vec::new()
        };

        StreamingBuffer {
            current_piece,
            critical_pieces,
            prefetch_pieces,
            buffer_duration,
            estimated_bitrate: bitrate_estimate,
        }
    }

    /// Order pieces for streaming.
    ///
    /// Returns pieces ordered by streaming priority, considering:
    /// - Current playback position
    /// - Buffer requirements
    /// - Piece availability from peers
    pub fn optimize_download_order(
        &self,
        required_pieces: Vec<PieceRange>,
        current_position: u64,
        available_pieces: &[u32],
    ) -> Vec<PieceRange> {
        let mut pieces = required_pieces;
        let current_piece = (current_position / self.piece_size as u64) as u32;

        // Sort by streaming priority
        pieces.sort_by(|a, b| {
            // Primary: distance from current position (closer = higher priority)
            let a_distance = if a.piece_index >= current_piece {
                a.piece_index - current_piece
            } else {
                u32::MAX // Pieces behind current position get lowest priority
            };

            let b_distance = if b.piece_index >= current_piece {
                b.piece_index - current_piece
            } else {
                u32::MAX
            };

            let distance_cmp = a_distance.cmp(&b_distance);
            if distance_cmp != std::cmp::Ordering::Equal {
                return distance_cmp;
            }

            // Secondary: availability (available pieces get higher priority)
            let a_available = available_pieces.contains(&a.piece_index);
            let b_available = available_pieces.contains(&b.piece_index);

            match (a_available, b_available) {
                (true, false) => std::cmp::Ordering::Less,
                (false, true) => std::cmp::Ordering::Greater,
                _ => std::cmp::Ordering::Equal,
            }
        });

        pieces
    }

    /// Calculate piece priority for downloading.
    fn calculate_piece_priority(
        &self,
        piece_index: u32,
        _start: u64,
        _length: u64,
    ) -> PiecePriority {
        // For now, use simple sequential priority
        // In production, this would consider:
        // - Distance from current playback position
        // - Buffer state
        // - Piece availability
        if piece_index < 10 {
            PiecePriority::Critical
        } else if piece_index < 50 {
            PiecePriority::High
        } else {
            PiecePriority::Normal
        }
    }

    /// Validate that a range request is satisfiable.
    ///
    /// # Errors
    /// - `RangeError::InvalidStart` - Start position exceeds total content size
    /// - `RangeError::InvalidLength` - Range extends beyond total content size
    pub fn validate_range(&self, start: u64, length: u64) -> Result<(), RangeError> {
        if start >= self.total_size {
            return Err(RangeError::InvalidStart {
                start,
                total_size: self.total_size,
            });
        }

        if start + length > self.total_size {
            return Err(RangeError::InvalidLength {
                start,
                length,
                total_size: self.total_size,
            });
        }

        Ok(())
    }
}

/// Range within a specific piece.
#[derive(Debug, Clone)]
pub struct PieceRange {
    pub piece_index: u32,
    pub start_offset: u32,
    pub end_offset: u32,
    pub priority: PiecePriority,
}

/// Priority level for piece downloading.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum PiecePriority {
    Critical, // Needed immediately for playback
    High,     // Needed soon for smooth streaming
    Normal,   // Prefetch for future playback
    Low,      // Background download
}

/// Streaming buffer management information.
#[derive(Debug, Clone)]
pub struct StreamingBuffer {
    pub current_piece: u32,
    pub critical_pieces: Vec<PieceRange>,
    pub prefetch_pieces: Vec<PieceRange>,
    pub buffer_duration: std::time::Duration,
    pub estimated_bitrate: u64,
}

/// Errors that can occur during range handling.
#[derive(Debug, thiserror::Error)]
pub enum RangeError {
    #[error("Invalid range start {start}, content size is {total_size}")]
    InvalidStart { start: u64, total_size: u64 },

    #[error("Invalid range length {length} starting at {start}, content size is {total_size}")]
    InvalidLength {
        start: u64,
        length: u64,
        total_size: u64,
    },

    #[error("Range not satisfiable")]
    NotSatisfiable,
}

/// Determine MIME type and media status from file extension.
/// Replaces mime_guess dependency with simple extension checking.
fn determine_file_type(filename: &str) -> (Option<String>, bool) {
    let extension = std::path::Path::new(filename)
        .extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| ext.to_lowercase());

    match extension.as_deref() {
        Some("mp4") => (Some("video/mp4".to_string()), true),
        Some("mkv") => (Some("video/x-matroska".to_string()), true),
        Some("avi") => (Some("video/x-msvideo".to_string()), true),
        Some("mov") => (Some("video/quicktime".to_string()), true),
        Some("wmv") => (Some("video/x-ms-wmv".to_string()), true),
        Some("flv") => (Some("video/x-flv".to_string()), true),
        Some("webm") => (Some("video/webm".to_string()), true),
        Some("m4v") => (Some("video/x-m4v".to_string()), true),
        Some("mp3") => (Some("audio/mpeg".to_string()), true),
        Some("flac") => (Some("audio/flac".to_string()), true),
        Some("wav") => (Some("audio/wav".to_string()), true),
        Some("aac") => (Some("audio/aac".to_string()), true),
        Some("ogg") => (Some("audio/ogg".to_string()), true),
        Some("m4a") => (Some("audio/mp4".to_string()), true),
        _ => (Some("application/octet-stream".to_string()), false),
    }
}

impl FileInfo {
    /// Create FileInfo from torrent file metadata.
    pub fn from_torrent_file(name: String, size: u64, offset: u64) -> Self {
        let (mime_type, is_media) = determine_file_type(&name);

        Self {
            name,
            size,
            offset,
            mime_type,
            is_media,
        }
    }

    /// Check if this file is suitable for streaming.
    pub fn is_streamable(&self) -> bool {
        self.is_media && self.size > 1_000_000 // At least 1MB for streaming
    }

    /// Get the byte range this file occupies in the torrent.
    pub fn byte_range(&self) -> (u64, u64) {
        (self.offset, self.offset + self.size - 1)
    }
}

impl ContentInfo {
    /// Create ContentInfo from torrent metadata.
    pub fn from_torrent_metadata(
        name: String,
        total_size: u64,
        piece_size: u32,
        total_pieces: u32,
        files: Vec<FileInfo>,
    ) -> Self {
        // Determine primary MIME type based on largest streamable file
        let mime_type = files
            .iter()
            .filter(|f| f.is_streamable())
            .max_by_key(|f| f.size)
            .and_then(|f| f.mime_type.clone());

        Self {
            name,
            total_size,
            mime_type,
            files,
            piece_size,
            total_pieces,
        }
    }

    /// Get all streamable files.
    pub fn streamable_files(&self) -> Vec<&FileInfo> {
        self.files.iter().filter(|f| f.is_streamable()).collect()
    }

    /// Find file containing a specific byte offset.
    pub fn file_at_offset(&self, offset: u64) -> Option<(usize, &FileInfo)> {
        self.files.iter().enumerate().find(|(_, file)| {
            let (start, end) = file.byte_range();
            offset >= start && offset <= end
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_calculate_required_pieces() {
        let handler = RangeHandler::new(16384, 10, 163840); // 10 pieces of 16KB

        // Request first 8KB (should need only first piece)
        let pieces = handler.calculate_required_pieces(0, 8192);
        assert_eq!(pieces.len(), 1);
        assert_eq!(pieces[0].piece_index, 0);
        assert_eq!(pieces[0].start_offset, 0);
        assert_eq!(pieces[0].end_offset, 8191);
    }

    #[test]
    fn test_calculate_required_pieces_spanning() {
        let handler = RangeHandler::new(16384, 10, 163840);

        // Request 20KB starting at 10KB (should span two pieces)
        let pieces = handler.calculate_required_pieces(10240, 20480);
        assert_eq!(pieces.len(), 2);

        // First piece: from offset 10240 to end of piece (16383)
        assert_eq!(pieces[0].piece_index, 0);
        assert_eq!(pieces[0].start_offset, 10240);
        assert_eq!(pieces[0].end_offset, 16383);

        // Second piece: from start to offset needed
        assert_eq!(pieces[1].piece_index, 1);
        assert_eq!(pieces[1].start_offset, 0);
    }

    #[test]
    fn test_validate_range() {
        let handler = RangeHandler::new(16384, 10, 163840);

        // Valid range
        assert!(handler.validate_range(0, 1024).is_ok());

        // Invalid start
        assert!(handler.validate_range(200000, 1024).is_err());

        // Invalid length
        assert!(handler.validate_range(163800, 1024).is_err());
    }

    #[test]
    fn test_file_info_from_torrent_file() {
        let file_info = FileInfo::from_torrent_file("movie.mp4".to_string(), 1_000_000_000, 0);

        assert!(file_info.is_media);
        assert!(file_info.is_streamable());
        assert_eq!(file_info.mime_type, Some("video/mp4".to_string()));
    }

    #[test]
    fn test_content_info_streamable_files() {
        let files = vec![
            FileInfo::from_torrent_file("movie.mp4".to_string(), 1_000_000_000, 0),
            FileInfo::from_torrent_file("readme.txt".to_string(), 1024, 1_000_000_000),
            FileInfo::from_torrent_file("audio.mp3".to_string(), 5_000_000, 1_000_001_024),
        ];

        let content_info = ContentInfo::from_torrent_metadata(
            "Test Torrent".to_string(),
            1_005_001_024,
            32768,
            30626,
            files,
        );

        let streamable = content_info.streamable_files();
        assert_eq!(streamable.len(), 2); // movie.mp4 and audio.mp3

        // Should pick video as primary MIME type (larger file)
        assert_eq!(content_info.mime_type, Some("video/mp4".to_string()));
    }

    #[test]
    fn test_streaming_buffer_calculation() {
        let handler = RangeHandler::new(32768, 100, 3276800); // 100 pieces of 32KB

        let buffer = handler.calculate_streaming_buffer(
            65536,                              // Current position at piece 2
            1_000_000,                          // 1 Mbps
            std::time::Duration::from_secs(10), // 10 second buffer
        );

        assert_eq!(buffer.current_piece, 2);
        assert!(!buffer.critical_pieces.is_empty());
        assert_eq!(buffer.estimated_bitrate, 1_000_000);
    }
}
