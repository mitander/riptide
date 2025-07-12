//! In-memory piece storage for simulation environments.
//!
//! Provides piece storage implementation using in-memory data structures
//! for deterministic testing and development scenarios.

use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use riptide_core::torrent::{InfoHash, PieceIndex, PieceStore, TorrentError, TorrentPiece};
use tokio::sync::RwLock;

/// In-memory piece storage for simulation environments.
///
/// Stores actual piece data created from real files for deterministic
/// testing and development. Enables true content distribution simulation.
pub struct InMemoryPieceStore {
    /// Map from info_hash to torrent piece data
    torrents: Arc<RwLock<HashMap<InfoHash, HashMap<u32, TorrentPiece>>>>,
}

impl InMemoryPieceStore {
    /// Creates empty in-memory piece store.
    pub fn new() -> Self {
        Self {
            torrents: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Adds torrent pieces to the store.
    ///
    /// Stores all pieces for a torrent, making them available for serving
    /// to peers during simulation.
    pub async fn add_torrent_pieces(&self, info_hash: InfoHash, pieces: Vec<TorrentPiece>) {
        let mut torrents = self.torrents.write().await;
        let piece_map: HashMap<u32, TorrentPiece> = pieces
            .into_iter()
            .map(|piece| (piece.index, piece))
            .collect();

        let piece_count = piece_map.len();
        torrents.insert(info_hash, piece_map);

        tracing::debug!(
            "InMemoryPieceStore: Added torrent {} with {} pieces",
            info_hash,
            piece_count
        );
    }

    /// Adds a single piece to an existing torrent.
    ///
    /// If the torrent doesn't exist, creates a new entry for it.
    pub async fn add_piece(&self, info_hash: InfoHash, piece: TorrentPiece) {
        let mut torrents = self.torrents.write().await;
        let piece_map = torrents.entry(info_hash).or_insert_with(HashMap::new);
        piece_map.insert(piece.index, piece);
    }

    /// Returns the number of torrents stored.
    pub async fn torrent_count(&self) -> usize {
        self.torrents.read().await.len()
    }

    /// Returns the number of pieces for a specific torrent.
    pub async fn piece_count(&self, info_hash: InfoHash) -> usize {
        self.torrents
            .read()
            .await
            .get(&info_hash)
            .map(|pieces| pieces.len())
            .unwrap_or(0)
    }

    /// Removes all pieces for a torrent.
    pub async fn remove_torrent(&self, info_hash: InfoHash) {
        let mut torrents = self.torrents.write().await;
        torrents.remove(&info_hash);
    }

    /// Clears all stored torrents and pieces.
    pub async fn clear(&self) {
        let mut torrents = self.torrents.write().await;
        torrents.clear();
    }

    /// Returns list of all stored torrent info hashes.
    pub async fn list_torrents(&self) -> Vec<InfoHash> {
        self.torrents.read().await.keys().copied().collect()
    }

    /// Returns total storage size in bytes.
    pub async fn total_size(&self) -> u64 {
        let torrents = self.torrents.read().await;
        torrents
            .values()
            .flat_map(|pieces| pieces.values())
            .map(|piece| piece.data.len() as u64)
            .sum()
    }
}

impl Default for InMemoryPieceStore {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl PieceStore for InMemoryPieceStore {
    /// Retrieves piece data for specified torrent and piece index.
    ///
    /// # Errors
    /// - `TorrentError::TorrentNotFound` - Unknown info hash
    /// - `TorrentError::InvalidPieceIndex` - Invalid piece index
    async fn piece_data(
        &self,
        info_hash: InfoHash,
        piece_index: PieceIndex,
    ) -> Result<Vec<u8>, TorrentError> {
        let torrents = self.torrents.read().await;
        let pieces = torrents
            .get(&info_hash)
            .ok_or(TorrentError::TorrentNotFound { info_hash })?;

        let piece = pieces
            .get(&piece_index.as_u32())
            .ok_or(TorrentError::InvalidPieceIndex {
                index: piece_index.as_u32(),
                max_index: pieces.len() as u32 - 1,
            })?;

        Ok(piece.data.clone())
    }

    /// Checks if piece is available without retrieving data.
    fn has_piece(&self, info_hash: InfoHash, piece_index: PieceIndex) -> bool {
        // For async-to-sync conversion, we use try_read which is non-blocking
        if let Ok(torrents) = self.torrents.try_read()
            && let Some(pieces) = torrents.get(&info_hash)
        {
            return pieces.contains_key(&piece_index.as_u32());
        }
        false
    }

    /// Returns total number of pieces for a torrent.
    ///
    /// # Errors
    /// - `TorrentError::TorrentNotFound` - Unknown info hash
    fn piece_count(&self, info_hash: InfoHash) -> Result<u32, TorrentError> {
        // For sync method, use try_read which is non-blocking
        let torrents = self
            .torrents
            .try_read()
            .map_err(|_| TorrentError::TorrentNotFound { info_hash })?;

        let pieces = torrents
            .get(&info_hash)
            .ok_or(TorrentError::TorrentNotFound { info_hash })?;

        Ok(pieces.len() as u32)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_empty_store() {
        let store = InMemoryPieceStore::new();
        assert_eq!(store.torrent_count().await, 0);
        assert_eq!(store.total_size().await, 0);
    }

    #[tokio::test]
    async fn test_add_and_retrieve_piece() {
        let store = InMemoryPieceStore::new();
        let info_hash = InfoHash::new([1u8; 20]);
        let piece_data = vec![1, 2, 3, 4, 5];

        let piece = TorrentPiece {
            index: 0,
            data: piece_data.clone(),
            hash: [0u8; 20],
        };

        store.add_piece(info_hash, piece).await;

        assert_eq!(store.piece_count(info_hash).await, 1);
        assert!(store.has_piece(info_hash, PieceIndex::new(0)));

        let retrieved_data = store
            .piece_data(info_hash, PieceIndex::new(0))
            .await
            .unwrap();
        assert_eq!(retrieved_data, piece_data);
    }

    #[tokio::test]
    async fn test_add_torrent() {
        let store = InMemoryPieceStore::new();
        let info_hash = InfoHash::new([2u8; 20]);

        let pieces = vec![
            TorrentPiece {
                index: 0,
                data: vec![1, 2, 3],
                hash: [0u8; 20],
            },
            TorrentPiece {
                index: 1,
                data: vec![4, 5, 6],
                hash: [1u8; 20],
            },
        ];

        store.add_torrent_pieces(info_hash, pieces).await;

        assert_eq!(store.torrent_count().await, 1);
        assert_eq!(store.piece_count(info_hash).await, 2);
        assert_eq!(store.total_size().await, 6);
    }

    #[tokio::test]
    async fn test_missing_torrent() {
        let store = InMemoryPieceStore::new();
        let info_hash = InfoHash::new([3u8; 20]);

        let result = store.piece_data(info_hash, PieceIndex::new(0)).await;
        assert!(matches!(result, Err(TorrentError::TorrentNotFound { .. })));
        assert!(!store.has_piece(info_hash, PieceIndex::new(0)));
    }

    #[tokio::test]
    async fn test_missing_piece() {
        let store = InMemoryPieceStore::new();
        let info_hash = InfoHash::new([4u8; 20]);

        let piece = TorrentPiece {
            index: 0,
            data: vec![1, 2, 3],
            hash: [0u8; 20],
        };

        store.add_piece(info_hash, piece).await;

        let result = store.piece_data(info_hash, PieceIndex::new(1)).await;
        assert!(matches!(
            result,
            Err(TorrentError::InvalidPieceIndex { .. })
        ));
        assert!(!store.has_piece(info_hash, PieceIndex::new(1)));
    }

    #[tokio::test]
    async fn test_clear_and_remove() {
        let store = InMemoryPieceStore::new();
        let info_hash1 = InfoHash::new([5u8; 20]);
        let info_hash2 = InfoHash::new([6u8; 20]);

        let piece1 = TorrentPiece {
            index: 0,
            data: vec![1, 2, 3],
            hash: [0u8; 20],
        };
        let piece2 = TorrentPiece {
            index: 0,
            data: vec![4, 5, 6],
            hash: [1u8; 20],
        };

        store.add_piece(info_hash1, piece1).await;
        store.add_piece(info_hash2, piece2).await;
        assert_eq!(store.torrent_count().await, 2);

        store.remove_torrent(info_hash1).await;
        assert_eq!(store.torrent_count().await, 1);
        assert_eq!(store.piece_count(info_hash1).await, 0);
        assert_eq!(store.piece_count(info_hash2).await, 1);

        store.clear().await;
        assert_eq!(store.torrent_count().await, 0);
        assert_eq!(store.total_size().await, 0);
    }

    #[tokio::test]
    async fn test_list_torrents() {
        let store = InMemoryPieceStore::new();
        let info_hash1 = InfoHash::new([7u8; 20]);
        let info_hash2 = InfoHash::new([8u8; 20]);

        let piece = TorrentPiece {
            index: 0,
            data: vec![1, 2, 3],
            hash: [0u8; 20],
        };

        store.add_piece(info_hash1, piece.clone()).await;
        store.add_piece(info_hash2, piece).await;

        let mut torrents = store.list_torrents().await;
        torrents.sort();

        let mut expected = vec![info_hash1, info_hash2];
        expected.sort();

        assert_eq!(torrents, expected);
    }
}
