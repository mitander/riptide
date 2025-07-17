//! Adapter to bridge DataSource implementations to PieceProvider trait.
//!
//! This module provides an adapter that allows existing DataSource implementations
//! to be used with the new streaming pipeline that expects PieceProvider trait objects.

use std::sync::Arc;

use async_trait::async_trait;
use bytes::Bytes;

use super::traits::{PieceProvider, PieceProviderError};
use crate::storage::{DataError, DataSource};
use crate::torrent::InfoHash;

/// Adapter that implements PieceProvider using a DataSource backend.
///
/// This adapter bridges the gap between the legacy DataSource interface
/// and the new PieceProvider trait used by the streaming pipeline.
/// It associates a specific InfoHash with the DataSource to provide
/// a file-like interface over torrent piece data.
pub struct PieceProviderAdapter {
    /// The underlying data source that manages torrent pieces.
    data_source: Arc<dyn DataSource>,
    /// The info hash identifying which torrent this provider serves.
    info_hash: InfoHash,
}

impl PieceProviderAdapter {
    /// Creates a new adapter for the given data source and torrent.
    ///
    /// The adapter will serve data for the specified torrent through
    /// the PieceProvider interface.
    pub fn new(data_source: Arc<dyn DataSource>, info_hash: InfoHash) -> Self {
        Self {
            data_source,
            info_hash,
        }
    }

    /// Returns the info hash of the torrent this adapter serves.
    pub fn info_hash(&self) -> InfoHash {
        self.info_hash
    }

    /// Returns a reference to the underlying data source.
    pub fn data_source(&self) -> &Arc<dyn DataSource> {
        &self.data_source
    }
}

#[async_trait]
impl PieceProvider for PieceProviderAdapter {
    async fn read_at(&self, offset: u64, length: usize) -> Result<Bytes, PieceProviderError> {
        let end = offset + length as u64;
        let data = self
            .data_source
            .read_range(self.info_hash, offset..end)
            .await
            .map_err(convert_data_error)?;

        Ok(Bytes::from(data))
    }

    async fn size(&self) -> u64 {
        self.data_source
            .file_size(self.info_hash)
            .await
            .unwrap_or(0)
    }
}

/// Converts DataError to PieceProviderError.
///
/// Maps storage-level errors to streaming-level errors with appropriate
/// error messages and context preservation.
fn convert_data_error(error: DataError) -> PieceProviderError {
    match error {
        DataError::InsufficientData { .. } => PieceProviderError::NotYetAvailable,
        DataError::Storage { reason } => PieceProviderError::StorageError(reason),
        DataError::Cache { reason } => PieceProviderError::StorageError(reason),
        DataError::RangeExceedsFile {
            start,
            end,
            file_size,
        } => PieceProviderError::InvalidRange {
            offset: start,
            length: (end - start) as usize,
            file_size,
        },
        DataError::InvalidRange { start, end } => {
            PieceProviderError::InvalidRange {
                offset: start,
                length: (end - start) as usize,
                file_size: 0, // We don't have file size in this context
            }
        }
        DataError::FileNotFound { .. } => {
            PieceProviderError::StorageError("File not found".to_string())
        }
        DataError::Torrent { source } => {
            PieceProviderError::StorageError(format!("Torrent error: {source}"))
        }
        DataError::Io { source } => PieceProviderError::StorageError(format!("IO error: {source}")),
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::ops::Range;

    use super::*;

    // Mock DataSource for testing
    struct MockDataSource {
        data: HashMap<InfoHash, Vec<u8>>,
    }

    impl MockDataSource {
        fn new() -> Self {
            Self {
                data: HashMap::new(),
            }
        }

        fn with_torrent(mut self, info_hash: InfoHash, data: Vec<u8>) -> Self {
            self.data.insert(info_hash, data);
            self
        }
    }

    #[async_trait]
    impl DataSource for MockDataSource {
        async fn read_range(
            &self,
            info_hash: InfoHash,
            range: Range<u64>,
        ) -> Result<Vec<u8>, DataError> {
            let data = self
                .data
                .get(&info_hash)
                .ok_or(DataError::FileNotFound { info_hash })?;

            let start = range.start as usize;
            let end = range.end.min(data.len() as u64) as usize;

            if start >= data.len() {
                return Ok(Vec::new());
            }

            Ok(data[start..end].to_vec())
        }

        async fn file_size(&self, info_hash: InfoHash) -> Result<u64, DataError> {
            self.data
                .get(&info_hash)
                .map(|data| data.len() as u64)
                .ok_or(DataError::FileNotFound { info_hash })
        }

        async fn check_range_availability(
            &self,
            _info_hash: InfoHash,
            _range: Range<u64>,
        ) -> Result<crate::storage::RangeAvailability, DataError> {
            Ok(crate::storage::RangeAvailability {
                available: true,
                missing_pieces: Vec::new(),
                cache_hit: false,
            })
        }

        async fn can_handle(&self, info_hash: InfoHash) -> bool {
            self.data.contains_key(&info_hash)
        }

        fn source_type(&self) -> &'static str {
            "mock_data_source"
        }
    }

    #[tokio::test]
    async fn test_adapter_basic_operations() {
        let info_hash = InfoHash::new([1u8; 20]);
        let test_data = b"Hello, World! This is test data for streaming.".to_vec();

        let data_source =
            Arc::new(MockDataSource::new().with_torrent(info_hash, test_data.clone()));

        let adapter = PieceProviderAdapter::new(data_source, info_hash);

        // Test size
        let size = adapter.size().await;
        assert_eq!(size, test_data.len() as u64);

        // Test read at beginning
        let data = adapter.read_at(0, 5).await.unwrap();
        assert_eq!(data.as_ref(), b"Hello");

        // Test read in middle
        let data = adapter.read_at(7, 5).await.unwrap();
        assert_eq!(data.as_ref(), b"World");

        // Test read at end
        let data = adapter.read_at(40, 7).await.unwrap();
        assert_eq!(data.as_ref(), b"aming.");
    }

    #[tokio::test]
    async fn test_adapter_range_handling() {
        let info_hash = InfoHash::new([2u8; 20]);
        let test_data = b"Short".to_vec();

        let data_source =
            Arc::new(MockDataSource::new().with_torrent(info_hash, test_data.clone()));

        let adapter = PieceProviderAdapter::new(data_source, info_hash);

        // Test reading past end returns truncated data
        let data = adapter.read_at(3, 10).await.unwrap();
        assert_eq!(data.as_ref(), b"rt");

        // Test reading completely past end returns empty
        let data = adapter.read_at(10, 5).await.unwrap();
        assert_eq!(data.len(), 0);
    }

    #[tokio::test]
    async fn test_adapter_info_hash_isolation() {
        let info_hash1 = InfoHash::new([1u8; 20]);
        let info_hash2 = InfoHash::new([2u8; 20]);

        let data_source = Arc::new(
            MockDataSource::new()
                .with_torrent(info_hash1, b"Data for torrent 1".to_vec())
                .with_torrent(info_hash2, b"Different data for torrent 2".to_vec()),
        );

        let adapter1 = PieceProviderAdapter::new(data_source.clone(), info_hash1);
        let adapter2 = PieceProviderAdapter::new(data_source, info_hash2);

        // Verify each adapter serves its own torrent's data
        let data1 = adapter1.read_at(0, 4).await.unwrap();
        let data2 = adapter2.read_at(0, 9).await.unwrap();

        assert_eq!(data1.as_ref(), b"Data");
        assert_eq!(data2.as_ref(), b"Different");

        assert_eq!(adapter1.size().await, 18);
        assert_eq!(adapter2.size().await, 28);
    }

    #[tokio::test]
    async fn test_adapter_accessor_methods() {
        let info_hash = InfoHash::new([3u8; 20]);
        let data_source = Arc::new(MockDataSource::new());

        let adapter = PieceProviderAdapter::new(data_source.clone(), info_hash);

        assert_eq!(adapter.info_hash(), info_hash);
        // Verify data source is correctly stored (can't use ptr_eq with trait objects)
        assert_eq!(adapter.data_source().source_type(), "mock_data_source");
    }
}
