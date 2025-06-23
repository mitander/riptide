//! Test data creation for torrent testing.
//!
//! Provides standardized torrent metadata and piece data for consistent
//! testing across torrent-related modules.

#![cfg(test)]

use super::parsing::TorrentFile;
use super::{InfoHash, PieceIndex, TorrentMetadata};
use crate::storage::file_storage::FileStorage;
use sha1::{Digest, Sha1};

/// Creates standard test torrent metadata with predictable hashes.
///
/// Generates a 3-piece torrent with 32 KiB pieces where piece data
/// is filled with the piece index value to create deterministic hashes.
pub fn create_test_torrent_metadata() -> TorrentMetadata {
    // Calculate correct hashes for test data
    // Piece 0: 32768 bytes of 0x00
    // Piece 1: 32768 bytes of 0x01
    // Piece 2: 32768 bytes of 0x02
    let piece_hashes = (0..3)
        .map(|i| {
            let mut hasher = Sha1::new();
            hasher.update(&vec![i as u8; 32768]);
            let result = hasher.finalize();
            let mut hash = [0u8; 20];
            hash.copy_from_slice(&result);
            hash
        })
        .collect();

    TorrentMetadata {
        info_hash: InfoHash::new([1u8; 20]),
        name: "test.txt".to_string(),
        piece_length: 32768,
        piece_hashes,
        total_length: 98304, // 3 * 32768
        files: vec![TorrentFile {
            path: vec!["test.txt".to_string()],
            length: 98304,
        }],
        announce_urls: vec!["http://tracker.example.com/announce".to_string()],
    }
}

/// Creates test piece data that matches the expected hash for the given piece index.
pub fn create_test_piece_data(piece_index: PieceIndex, piece_size: usize) -> Vec<u8> {
    vec![piece_index.as_u32() as u8; piece_size]
}

/// Creates a simple 2-piece torrent for basic testing.
pub fn create_simple_torrent_metadata() -> TorrentMetadata {
    let piece_hashes = (0..2)
        .map(|i| {
            let mut hasher = Sha1::new();
            hasher.update(&vec![i as u8; 16384]);
            let result = hasher.finalize();
            let mut hash = [0u8; 20];
            hash.copy_from_slice(&result);
            hash
        })
        .collect();

    TorrentMetadata {
        info_hash: InfoHash::new([2u8; 20]),
        name: "simple.txt".to_string(),
        piece_length: 16384,
        piece_hashes,
        total_length: 32768, // 2 * 16384
        files: vec![TorrentFile {
            path: vec!["simple.txt".to_string()],
            length: 32768,
        }],
        announce_urls: vec!["http://tracker.example.com/announce".to_string()],
    }
}

/// Creates a test InfoHash for peer manager testing.
pub fn create_test_info_hash() -> InfoHash {
    InfoHash::new([0x42u8; 20])
}

/// Creates complete test environment with torrent metadata and storage.
///
/// Composition pattern for tests that need both torrent and storage setup.
/// Avoids duplicating setup code across integration tests.
pub fn create_test_environment() -> (TorrentMetadata, FileStorage, tempfile::TempDir) {
    let metadata = create_test_torrent_metadata();
    let (temp_dir, downloads_dir, library_dir) =
        crate::storage::test_fixtures::create_temp_storage_dirs();
    let storage = FileStorage::new(downloads_dir, library_dir);

    (metadata, storage, temp_dir)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_test_torrent_metadata() {
        let metadata = create_test_torrent_metadata();

        assert_eq!(metadata.piece_hashes.len(), 3);
        assert_eq!(metadata.piece_length, 32768);
        assert_eq!(metadata.total_length, 98304);
        assert_eq!(metadata.name, "test.txt");
    }

    #[test]
    fn test_create_test_piece_data() {
        let piece_data = create_test_piece_data(PieceIndex::new(0), 1024);

        assert_eq!(piece_data.len(), 1024);
        assert!(piece_data.iter().all(|&b| b == 0));

        let piece_data = create_test_piece_data(PieceIndex::new(1), 512);
        assert!(piece_data.iter().all(|&b| b == 1));
    }
}
