//! Test data generation utilities for streaming tests.

use riptide_core::torrent::creation::TorrentPiece;

/// Creates realistic video data for streaming tests.
///
/// Generates mock video content with MP4 signature and varying data pattern
/// to simulate real video files for streaming and piece-based testing.
pub fn create_realistic_video_data(size: usize) -> Vec<u8> {
    let mut data = Vec::with_capacity(size);

    // Add MP4 file signature at start
    data.extend_from_slice(b"ftypmp41");

    // Fill with varying data to simulate real video content
    for i in 8..size {
        data.push(((i * 17) % 256) as u8); // Pseudo-random pattern
    }

    data
}

/// Creates simple mock video data with repeated pattern.
///
/// Generates predictable test data for cases where content doesn't matter,
/// only size and basic structure.
pub fn create_simple_mock_data(pattern: &[u8], size: usize) -> Vec<u8> {
    let mut data = Vec::with_capacity(size);
    let pattern_len = pattern.len();

    for i in 0..size {
        data.push(pattern[i % pattern_len]);
    }

    data
}

/// Splits data into torrent pieces for testing.
///
/// Simulates TorrentCreator behavior by splitting data into fixed-size pieces
/// with mock hashes for testing piece-based streaming functionality.
pub fn split_into_pieces(data: &[u8], piece_size: u32) -> Vec<TorrentPiece> {
    let mut pieces = Vec::new();

    for (i, chunk) in data.chunks(piece_size as usize).enumerate() {
        pieces.push(TorrentPiece {
            index: i as u32,
            hash: [((i * 7) % 256) as u8; 20], // Mock hash
            data: chunk.to_vec(),
        });
    }

    pieces
}

/// Creates pieces with deterministic hashes based on index.
///
/// Useful for tests that need predictable piece verification behavior.
pub fn create_sequential_pieces(count: u32, piece_size: usize) -> Vec<TorrentPiece> {
    let mut pieces = Vec::new();

    for i in 0..count {
        let mut data = vec![0u8; piece_size];
        // Fill with index-based pattern
        for (j, byte) in data.iter_mut().enumerate() {
            *byte = ((i + j as u32) % 256) as u8;
        }

        pieces.push(TorrentPiece {
            index: i,
            hash: [(i % 256) as u8; 20], // Simple deterministic hash
            data,
        });
    }

    pieces
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_realistic_video_data_has_mp4_signature() {
        let data = create_realistic_video_data(1000);
        assert_eq!(&data[0..8], b"ftypmp41");
        assert_eq!(data.len(), 1000);
    }

    #[test]
    fn test_simple_mock_data_repeats_pattern() {
        let data = create_simple_mock_data(b"TEST", 12);
        assert_eq!(data, b"TESTTESTTEST");
    }

    #[test]
    fn test_split_into_pieces_correct_count() {
        let data = vec![1, 2, 3, 4, 5, 6, 7, 8];
        let pieces = split_into_pieces(&data, 3);

        assert_eq!(pieces.len(), 3);
        assert_eq!(pieces[0].data, vec![1, 2, 3]);
        assert_eq!(pieces[1].data, vec![4, 5, 6]);
        assert_eq!(pieces[2].data, vec![7, 8]);
        assert_eq!(pieces[0].index, 0);
        assert_eq!(pieces[1].index, 1);
        assert_eq!(pieces[2].index, 2);
    }

    #[test]
    fn test_sequential_pieces_deterministic() {
        let pieces = create_sequential_pieces(3, 4);

        assert_eq!(pieces.len(), 3);
        assert_eq!(pieces[0].index, 0);
        assert_eq!(pieces[1].index, 1);
        assert_eq!(pieces[2].index, 2);
        assert_eq!(pieces[0].data.len(), 4);
        assert_eq!(pieces[0].hash[0], 0);
        assert_eq!(pieces[1].hash[0], 1);
    }
}
