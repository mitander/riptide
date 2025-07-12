//! Test data creation for torrent testing.
//!
//! Provides standardized torrent metadata and piece data for consistent
//! testing across torrent-related modules.

use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use sha1::{Digest, Sha1};

use super::parsing::TorrentFile;
use super::{InfoHash, PieceIndex, TorrentError, TorrentMetadata};
use crate::storage::file_storage::FileStorage;
#[cfg(any(test, feature = "test-utils"))]
use crate::storage::test_fixtures::create_temp_storage_dirs;
use crate::torrent::{PieceStore, TorrentPiece};

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
            hasher.update(vec![i as u8; 32768]);
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
            hasher.update(vec![i as u8; 16384]);
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

/// Creates a mock piece store for testing with predefined test data.
///
/// Returns a piece store populated with deterministic test pieces that can be
/// used across different test scenarios for consistent behavior.
pub fn create_test_piece_store() -> Arc<TestPieceStore> {
    let mut store = TestPieceStore::new();
    let info_hash = InfoHash::new([1u8; 20]);

    // Add test pieces with deterministic data
    let pieces = vec![
        TorrentPiece {
            index: 0,
            data: vec![0u8; 1024],
            hash: [0u8; 20],
        },
        TorrentPiece {
            index: 1,
            data: vec![1u8; 1024],
            hash: [1u8; 20],
        },
    ];

    for piece in pieces {
        store.add_piece(info_hash, piece);
    }

    Arc::new(store)
}

/// Test piece store implementation for deterministic testing.
///
/// Provides in-memory piece storage with predictable behavior for testing
/// torrent operations without requiring real file I/O.
pub struct TestPieceStore {
    pieces: HashMap<(InfoHash, u32), TorrentPiece>,
    piece_counts: HashMap<InfoHash, u32>,
}

impl Default for TestPieceStore {
    fn default() -> Self {
        Self::new()
    }
}

impl TestPieceStore {
    /// Creates new empty test piece store.
    pub fn new() -> Self {
        Self {
            pieces: HashMap::new(),
            piece_counts: HashMap::new(),
        }
    }

    /// Adds a piece to the store.
    pub fn add_piece(&mut self, info_hash: InfoHash, piece: TorrentPiece) {
        self.pieces.insert((info_hash, piece.index), piece);
        *self.piece_counts.entry(info_hash).or_insert(0) += 1;
    }
}

#[async_trait]
impl PieceStore for TestPieceStore {
    async fn piece_data(
        &self,
        info_hash: InfoHash,
        piece_index: PieceIndex,
    ) -> Result<Vec<u8>, TorrentError> {
        self.pieces
            .get(&(info_hash, piece_index.as_u32()))
            .map(|piece| piece.data.clone())
            .ok_or(TorrentError::TorrentNotFound { info_hash })
    }

    fn has_piece(&self, info_hash: InfoHash, piece_index: PieceIndex) -> bool {
        self.pieces.contains_key(&(info_hash, piece_index.as_u32()))
    }

    fn piece_count(&self, info_hash: InfoHash) -> Result<u32, TorrentError> {
        Ok(self.piece_counts.get(&info_hash).copied().unwrap_or(0))
    }
}

/// Creates complete test environment with torrent metadata and storage.
///
/// Only available when tempfile is available (in tests or with test-utils feature).
#[cfg(any(test, feature = "test-utils"))]
pub fn create_test_environment() -> (TorrentMetadata, FileStorage, tempfile::TempDir) {
    let metadata = create_test_torrent_metadata();
    let (temp_dir, downloads_dir, library_dir) = create_temp_storage_dirs();
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
