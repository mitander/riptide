//! In-memory piece storage for simulation environments
//!
//! Provides piece storage implementation using in-memory data structures
//! for deterministic testing and development scenarios.

use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use riptide_core::torrent::{InfoHash, PieceIndex, PieceStore, TorrentError, TorrentPiece};
use tokio::sync::RwLock;

/// In-memory piece storage for simulation environments
///
/// Stores actual piece data created from real files for deterministic
/// testing and development. Enables true content distribution simulation.
pub struct InMemoryPieceStore {
    /// Map from info_hash to torrent piece data
    torrents: Arc<RwLock<HashMap<InfoHash, HashMap<u32, TorrentPiece>>>>,
}

impl InMemoryPieceStore {
    /// Creates empty in-memory piece store
    pub fn new() -> Self {
        Self {
            torrents: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Adds torrent pieces to the store
    ///
    /// Stores pieces created by TorrentCreator for serving in simulation.
    /// Pieces are indexed by info_hash and piece_index for efficient lookup.
    ///
    /// # Errors
    /// - `TorrentError::InvalidTorrentFile` - Empty pieces or invalid data
    pub async fn add_torrent_pieces(
        &self,
        info_hash: InfoHash,
        pieces: Vec<TorrentPiece>,
    ) -> Result<(), TorrentError> {
        if pieces.is_empty() {
            return Err(TorrentError::InvalidTorrentFile {
                reason: "Cannot add torrent with no pieces".to_string(),
            });
        }

        let mut piece_map = HashMap::new();
        for piece in pieces {
            piece_map.insert(piece.index, piece);
        }

        let mut torrents = self.torrents.write().await;
        torrents.insert(info_hash, piece_map);

        Ok(())
    }

    /// Removes torrent from store
    pub async fn remove_torrent(&self, info_hash: InfoHash) {
        let mut torrents = self.torrents.write().await;
        torrents.remove(&info_hash);
    }

    /// Returns number of torrents stored
    pub async fn torrent_count(&self) -> usize {
        let torrents = self.torrents.read().await;
        torrents.len()
    }

    /// Returns total number of pieces across all torrents
    pub async fn total_piece_count(&self) -> usize {
        let torrents = self.torrents.read().await;
        torrents.values().map(|pieces| pieces.len()).sum()
    }
}

impl Default for InMemoryPieceStore {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl PieceStore for InMemoryPieceStore {
    async fn piece_data(
        &self,
        info_hash: InfoHash,
        piece_index: PieceIndex,
    ) -> Result<Vec<u8>, TorrentError> {
        let torrents = self.torrents.read().await;

        let pieces = torrents
            .get(&info_hash)
            .ok_or_else(|| TorrentError::TorrentNotFound { info_hash })?;

        let piece = pieces
            .get(&piece_index.as_u32())
            .ok_or_else(|| TorrentError::PieceHashMismatch { index: piece_index })?;

        Ok(piece.data.clone())
    }

    fn has_piece(&self, info_hash: InfoHash, piece_index: PieceIndex) -> bool {
        // Use try_read to avoid blocking in sync context
        if let Ok(torrents) = self.torrents.try_read() {
            if let Some(pieces) = torrents.get(&info_hash) {
                return pieces.contains_key(&piece_index.as_u32());
            }
        }
        false
    }

    fn piece_count(&self, info_hash: InfoHash) -> Result<u32, TorrentError> {
        // Use try_read for sync context
        if let Ok(torrents) = self.torrents.try_read() {
            if let Some(pieces) = torrents.get(&info_hash) {
                return Ok(pieces.len() as u32);
            }
        }
        Err(TorrentError::TorrentNotFound { info_hash })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_in_memory_store_add_retrieve_pieces() {
        let store = InMemoryPieceStore::new();
        let info_hash = InfoHash::new([1u8; 20]);

        // Create test pieces
        let pieces = vec![
            TorrentPiece {
                index: 0,
                hash: [0u8; 20],
                data: b"piece 0 data".to_vec(),
            },
            TorrentPiece {
                index: 1,
                hash: [1u8; 20],
                data: b"piece 1 data".to_vec(),
            },
        ];

        // Add pieces to store
        store.add_torrent_pieces(info_hash, pieces).await.unwrap();

        // Verify piece availability
        assert!(store.has_piece(info_hash, PieceIndex::new(0)));
        assert!(store.has_piece(info_hash, PieceIndex::new(1)));
        assert!(!store.has_piece(info_hash, PieceIndex::new(2)));

        // Retrieve piece data
        let piece_0_data = store
            .piece_data(info_hash, PieceIndex::new(0))
            .await
            .unwrap();
        assert_eq!(piece_0_data, b"piece 0 data");

        let piece_1_data = store
            .piece_data(info_hash, PieceIndex::new(1))
            .await
            .unwrap();
        assert_eq!(piece_1_data, b"piece 1 data");
    }

    #[tokio::test]
    async fn test_in_memory_store_piece_count() {
        let store = InMemoryPieceStore::new();
        let info_hash = InfoHash::new([2u8; 20]);

        let pieces = vec![TorrentPiece {
            index: 0,
            hash: [0u8; 20],
            data: b"test".to_vec(),
        }];

        store.add_torrent_pieces(info_hash, pieces).await.unwrap();

        assert_eq!(store.piece_count(info_hash).unwrap(), 1);
        assert_eq!(store.torrent_count().await, 1);
        assert_eq!(store.total_piece_count().await, 1);
    }

    #[tokio::test]
    async fn test_in_memory_store_unknown_torrent() {
        let store = InMemoryPieceStore::new();
        let unknown_hash = InfoHash::new([99u8; 20]);

        assert!(!store.has_piece(unknown_hash, PieceIndex::new(0)));

        let result = store.piece_data(unknown_hash, PieceIndex::new(0)).await;
        assert!(matches!(result, Err(TorrentError::TorrentNotFound { .. })));
    }

    #[tokio::test]
    async fn test_in_memory_store_remove_torrent() {
        let store = InMemoryPieceStore::new();
        let info_hash = InfoHash::new([3u8; 20]);

        let pieces = vec![TorrentPiece {
            index: 0,
            hash: [0u8; 20],
            data: b"test".to_vec(),
        }];

        store.add_torrent_pieces(info_hash, pieces).await.unwrap();
        assert_eq!(store.torrent_count().await, 1);

        store.remove_torrent(info_hash).await;
        assert_eq!(store.torrent_count().await, 0);
    }

    #[tokio::test]
    async fn test_in_memory_store_empty_pieces_error() {
        let store = InMemoryPieceStore::new();
        let info_hash = InfoHash::new([4u8; 20]);

        let result = store.add_torrent_pieces(info_hash, vec![]).await;
        assert!(matches!(
            result,
            Err(TorrentError::InvalidTorrentFile { .. })
        ));
    }
}
