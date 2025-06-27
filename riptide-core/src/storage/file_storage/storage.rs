//! Storage trait implementation for FileStorage

use std::path::PathBuf;

use async_trait::async_trait;
use tokio::fs;

use super::types::FileStorage;
use crate::storage::{Storage, StorageError};
use crate::torrent::{InfoHash, PieceIndex};

#[async_trait]
impl Storage for FileStorage {
    async fn store_piece(
        &mut self,
        info_hash: InfoHash,
        index: PieceIndex,
        piece_bytes: &[u8],
    ) -> Result<(), StorageError> {
        let piece_path = self
            .download_dir
            .join(info_hash.to_string())
            .join(format!("piece_{}", index.as_u32()));

        if let Some(parent) = piece_path.parent() {
            fs::create_dir_all(parent).await?;
        }

        fs::write(&piece_path, piece_bytes).await?;
        Ok(())
    }

    async fn load_piece(
        &self,
        info_hash: InfoHash,
        index: PieceIndex,
    ) -> Result<Vec<u8>, StorageError> {
        let piece_path = self
            .download_dir
            .join(info_hash.to_string())
            .join(format!("piece_{}", index.as_u32()));

        match fs::read(&piece_path).await {
            Ok(piece_bytes) => Ok(piece_bytes),
            Err(_) => Err(StorageError::PieceNotFound { index }),
        }
    }

    async fn has_piece(
        &self,
        info_hash: InfoHash,
        index: PieceIndex,
    ) -> Result<bool, StorageError> {
        let piece_path = self
            .download_dir
            .join(info_hash.to_string())
            .join(format!("piece_{}", index.as_u32()));

        Ok(piece_path.exists())
    }

    async fn finalize_torrent(&mut self, info_hash: InfoHash) -> Result<PathBuf, StorageError> {
        let download_path = self.download_dir.join(info_hash.to_string());
        let library_path = self.library_dir.join(info_hash.to_string());

        fs::rename(download_path, &library_path).await?;
        Ok(library_path)
    }
}
