//! Chunk server for real-time remuxed content serving
//!
//! This module provides components for serving remuxed media chunks as they are
//! produced by FFmpeg processes. It enables true progressive streaming by
//! buffering and serving chunks without waiting for complete file processing.

use std::collections::VecDeque;
use std::sync::Arc;
use std::time::Instant;

use bytes::Bytes;
use thiserror::Error;
use tokio::sync::{RwLock, watch};
use tracing::{debug, error, info, warn};

use crate::torrent::InfoHash;

/// Errors that can occur during chunk serving operations
#[derive(Debug, Error)]
pub enum ChunkServerError {
    /// Buffer capacity exceeded
    #[error("Buffer capacity exceeded: {current} >= {max}")]
    BufferFull {
        /// Current buffer size in bytes
        current: usize,
        /// Maximum allowed buffer size in bytes
        max: usize,
    },

    /// Requested chunk range is invalid
    #[error("Invalid chunk range: start={start}, end={end}, available={available}")]
    InvalidRange {
        /// Start offset of invalid range
        start: u64,
        /// End offset of invalid range
        end: u64,
        /// Currently available bytes
        available: u64,
    },

    /// Chunk not available yet
    #[error("Chunk not available: offset={offset}, available={available}")]
    ChunkNotAvailable {
        /// Requested byte offset
        offset: u64,
        /// Currently available bytes
        available: u64,
    },

    /// Stream has been terminated
    #[error("Stream terminated for {info_hash}")]
    StreamTerminated {
        /// InfoHash of terminated stream
        info_hash: InfoHash,
    },

    /// Buffer operation failed
    #[error("Buffer operation failed: {reason}")]
    BufferOperation {
        /// Reason for buffer operation failure
        reason: String,
    },
}

/// Request for chunk data with range support
#[derive(Debug, Clone)]
pub struct ChunkRequest {
    /// Starting byte offset
    pub start: u64,
    /// Ending byte offset (exclusive)
    pub end: Option<u64>,
    /// Whether this is a range request
    pub is_range_request: bool,
    /// Client identifier for tracking
    pub client_id: Option<String>,
}

/// Response containing chunk data and metadata
#[derive(Debug, Clone)]
pub struct ChunkResponse {
    /// Chunk data
    pub data: Bytes,
    /// Content range for HTTP responses
    pub content_range: Option<String>,
    /// Total content length if known
    pub total_length: Option<u64>,
    /// Whether this is a partial response
    pub is_partial: bool,
}

/// Individual chunk of remuxed data
#[derive(Debug, Clone)]
pub struct RemuxedChunk {
    /// Byte offset in the output stream
    pub offset: u64,
    /// Chunk data
    pub data: Bytes,
    /// Whether this chunk contains header information
    pub is_header: bool,
    /// Timestamp when chunk was produced
    pub timestamp: Instant,
}

/// Statistics for chunk buffer performance monitoring
#[derive(Debug, Clone)]
pub struct BufferStats {
    /// Total chunks stored
    pub total_chunks: usize,
    /// Total bytes buffered
    pub total_bytes: u64,
    /// Number of cache hits
    pub cache_hits: u64,
    /// Number of cache misses
    pub cache_misses: u64,
    /// Number of chunks evicted
    pub evicted_chunks: u64,
    /// Current buffer utilization (0.0 to 1.0)
    pub utilization: f64,
    /// Average chunk size
    pub avg_chunk_size: u64,
}

/// Buffer for storing remuxed chunks with LRU eviction
#[derive(Debug)]
pub struct ChunkBuffer {
    chunks: VecDeque<RemuxedChunk>,
    max_chunks: usize,
    max_bytes: u64,
    current_bytes: u64,
    stats: BufferStats,
    total_length: Option<u64>,
}

impl ChunkBuffer {
    /// Create new chunk buffer with specified limits
    ///
    /// # Arguments
    /// * `max_chunks` - Maximum number of chunks to buffer
    /// * `max_bytes` - Maximum bytes to buffer
    pub fn new(max_chunks: usize, max_bytes: u64) -> Self {
        Self {
            chunks: VecDeque::with_capacity(max_chunks),
            max_chunks,
            max_bytes,
            current_bytes: 0,
            stats: BufferStats {
                total_chunks: 0,
                total_bytes: 0,
                cache_hits: 0,
                cache_misses: 0,
                evicted_chunks: 0,
                utilization: 0.0,
                avg_chunk_size: 0,
            },
            total_length: None,
        }
    }

    /// Add chunk to buffer with LRU eviction
    ///
    /// # Errors
    /// - `ChunkServerError::BufferFull` - If chunk cannot fit even after eviction
    pub fn add_chunk(&mut self, chunk: RemuxedChunk) -> Result<(), ChunkServerError> {
        debug!(
            "Adding chunk at offset {} with {} bytes",
            chunk.offset,
            chunk.data.len()
        );

        // Evict chunks if necessary
        while (self.chunks.len() >= self.max_chunks)
            || (self.current_bytes + chunk.data.len() as u64 > self.max_bytes)
        {
            if let Some(evicted) = self.chunks.pop_front() {
                self.current_bytes -= evicted.data.len() as u64;
                self.stats.evicted_chunks += 1;
                debug!("Evicted chunk at offset {}", evicted.offset);
            } else {
                return Err(ChunkServerError::BufferFull {
                    current: chunk.data.len(),
                    max: self.max_bytes as usize,
                });
            }
        }

        // Add new chunk
        self.current_bytes += chunk.data.len() as u64;
        self.stats.total_chunks = self.chunks.len() + 1;
        self.stats.total_bytes += chunk.data.len() as u64;
        self.chunks.push_back(chunk);

        self.update_stats();
        Ok(())
    }

    /// Try to retrieve chunk data for specified range
    ///
    /// # Errors
    /// - `ChunkServerError::InvalidRange` - If range is malformed
    /// - `ChunkServerError::ChunkNotAvailable` - If data is not buffered
    pub fn try_get_range(
        &mut self,
        start: u64,
        end: Option<u64>,
    ) -> Result<Bytes, ChunkServerError> {
        let end = end.unwrap_or_else(|| self.available_bytes());

        if start >= end {
            return Err(ChunkServerError::InvalidRange {
                start,
                end,
                available: self.available_bytes(),
            });
        }

        // Find chunks that overlap with requested range
        let mut result_data = Vec::new();
        let mut current_offset = start;

        for chunk in &self.chunks {
            let chunk_start = chunk.offset;
            let chunk_end = chunk.offset + chunk.data.len() as u64;

            // Skip chunks before our range
            if chunk_end <= start {
                continue;
            }

            // Stop if chunk starts after our range
            if chunk_start >= end {
                break;
            }

            // Calculate overlap
            let overlap_start = chunk_start.max(start);
            let overlap_end = chunk_end.min(end);

            if overlap_start < overlap_end {
                let data_start = (overlap_start - chunk_start) as usize;
                let data_end = (overlap_end - chunk_start) as usize;

                // Check for gaps in data
                if current_offset < overlap_start {
                    self.stats.cache_misses += 1;
                    return Err(ChunkServerError::ChunkNotAvailable {
                        offset: current_offset,
                        available: self.available_bytes(),
                    });
                }

                result_data.extend_from_slice(&chunk.data[data_start..data_end]);
                current_offset = overlap_end;
            }
        }

        // Check if we have all requested data
        if current_offset < end {
            self.stats.cache_misses += 1;
            return Err(ChunkServerError::ChunkNotAvailable {
                offset: current_offset,
                available: self.available_bytes(),
            });
        }

        self.stats.cache_hits += 1;
        Ok(Bytes::from(result_data))
    }

    /// Get the total bytes available for reading
    pub fn available_bytes(&self) -> u64 {
        self.chunks
            .back()
            .map(|chunk| chunk.offset + chunk.data.len() as u64)
            .unwrap_or(0)
    }

    /// Check if buffer has header chunks
    pub fn has_headers(&self) -> bool {
        self.chunks.iter().any(|chunk| chunk.is_header)
    }

    /// Set total content length when known
    pub fn set_total_length(&mut self, length: u64) {
        self.total_length = Some(length);
    }

    /// Get total content length if known
    pub fn total_length(&self) -> Option<u64> {
        self.total_length
    }

    /// Get buffer statistics
    pub fn stats(&self) -> &BufferStats {
        &self.stats
    }

    /// Update internal statistics
    fn update_stats(&mut self) {
        self.stats.utilization = if self.max_bytes > 0 {
            self.current_bytes as f64 / self.max_bytes as f64
        } else {
            0.0
        };

        self.stats.avg_chunk_size = if self.chunks.is_empty() {
            0
        } else {
            self.current_bytes / self.chunks.len() as u64
        };
    }
}

/// Server for handling chunk requests and managing buffers
#[derive(Debug)]
pub struct ChunkServer {
    info_hash: InfoHash,
    buffer: Arc<RwLock<ChunkBuffer>>,
    is_active: Arc<RwLock<bool>>,
    completion_notifier: watch::Sender<bool>,
    completion_receiver: watch::Receiver<bool>,
}

impl ChunkServer {
    /// Create new chunk server for specified stream
    ///
    /// # Arguments
    /// * `info_hash` - Stream identifier
    /// * `max_chunks` - Maximum chunks to buffer
    /// * `max_bytes` - Maximum bytes to buffer
    pub fn new(info_hash: InfoHash, max_chunks: usize, max_bytes: u64) -> Self {
        let buffer = Arc::new(RwLock::new(ChunkBuffer::new(max_chunks, max_bytes)));
        let (completion_notifier, completion_receiver) = watch::channel(false);

        Self {
            info_hash,
            buffer,
            is_active: Arc::new(RwLock::new(true)),
            completion_notifier,
            completion_receiver,
        }
    }

    /// Add remuxed chunk to buffer
    ///
    /// # Errors
    /// - `ChunkServerError::StreamTerminated` - If stream has been terminated
    /// - `ChunkServerError::BufferFull` - If buffer cannot accommodate chunk
    pub async fn add_chunk(&self, chunk: RemuxedChunk) -> Result<(), ChunkServerError> {
        let is_active = *self.is_active.read().await;
        if !is_active {
            return Err(ChunkServerError::StreamTerminated {
                info_hash: self.info_hash,
            });
        }

        let mut buffer = self.buffer.write().await;
        buffer.add_chunk(chunk)?;

        debug!(
            "Added chunk for {}, buffer stats: {} chunks, {} bytes",
            self.info_hash,
            buffer.stats().total_chunks,
            buffer.stats().total_bytes
        );

        Ok(())
    }

    /// Serve chunk request with range support
    ///
    /// # Errors
    /// - `ChunkServerError::StreamTerminated` - If stream has been terminated
    /// - `ChunkServerError::InvalidRange` - If range request is invalid
    /// - `ChunkServerError::ChunkNotAvailable` - If requested data is not available
    pub async fn serve_request(
        &self,
        request: ChunkRequest,
    ) -> Result<ChunkResponse, ChunkServerError> {
        let is_active = *self.is_active.read().await;
        if !is_active {
            return Err(ChunkServerError::StreamTerminated {
                info_hash: self.info_hash,
            });
        }

        debug!(
            "Serving request for {}: start={}, end={:?}",
            self.info_hash, request.start, request.end
        );

        let mut buffer = self.buffer.write().await;
        let available = buffer.available_bytes();

        let end = request.end.unwrap_or(available);
        let data = buffer.try_get_range(request.start, Some(end))?;

        let content_range = if request.is_range_request {
            let total = buffer.total_length().unwrap_or(available);
            Some(format!(
                "bytes {}-{}/{}",
                request.start,
                request.start + data.len() as u64 - 1,
                total
            ))
        } else {
            None
        };

        Ok(ChunkResponse {
            data,
            content_range,
            total_length: buffer.total_length(),
            is_partial: request.is_range_request,
        })
    }

    /// Check if stream has sufficient data for playback start
    pub async fn is_ready_for_playback(&self) -> bool {
        let buffer = self.buffer.read().await;
        buffer.has_headers() && buffer.available_bytes() >= 64 * 1024 // 64KB minimum
    }

    /// Get current buffer statistics
    pub async fn stats(&self) -> BufferStats {
        let buffer = self.buffer.read().await;
        buffer.stats().clone()
    }

    /// Mark stream as completed
    pub async fn mark_completed(&self, total_length: u64) {
        let mut buffer = self.buffer.write().await;
        buffer.set_total_length(total_length);

        let _ = self.completion_notifier.send(true);
        info!("Stream {} marked as completed", self.info_hash);
    }

    /// Terminate the chunk server
    pub async fn terminate(&self) {
        let mut is_active = self.is_active.write().await;
        *is_active = false;

        warn!("Chunk server for {} terminated", self.info_hash);
    }

    /// Wait for stream completion
    /// Wait for the chunk server to complete processing all chunks
    ///
    /// # Errors
    ///
    /// - `ChunkServerError::Timeout` - If completion takes too long
    /// - `ChunkServerError::InternalError` - If an internal error occurs during completion
    pub async fn wait_for_completion(&self) -> Result<(), ChunkServerError> {
        let mut receiver = self.completion_receiver.clone();

        loop {
            if receiver.changed().await.is_err() {
                return Err(ChunkServerError::StreamTerminated {
                    info_hash: self.info_hash,
                });
            }

            if *receiver.borrow() {
                return Ok(());
            }
        }
    }

    /// Serve data for a specific byte range
    ///
    /// # Errors
    /// - `ChunkServerError::InvalidRange` - If range is malformed
    /// - `ChunkServerError::ChunkNotAvailable` - If requested data is not available
    pub async fn serve_range(
        &self,
        range: std::ops::Range<u64>,
    ) -> Result<Vec<u8>, ChunkServerError> {
        let mut buffer = self.buffer.write().await;
        let end = if range.end == 0 {
            None
        } else {
            Some(range.end)
        };
        buffer
            .try_get_range(range.start, end)
            .map(|bytes| bytes.to_vec())
    }

    /// Check if the stream is complete
    pub async fn is_complete(&self) -> bool {
        let buffer = self.buffer.read().await;
        buffer.total_length().is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_chunk(offset: u64, size: usize, is_header: bool) -> RemuxedChunk {
        let data = vec![0u8; size];
        RemuxedChunk {
            offset,
            data: Bytes::from(data),
            is_header,
            timestamp: Instant::now(),
        }
    }

    #[test]
    fn test_chunk_buffer_add_and_retrieve() {
        let mut buffer = ChunkBuffer::new(10, 1024);

        let chunk = create_test_chunk(0, 100, true);
        buffer.add_chunk(chunk).unwrap();

        assert_eq!(buffer.available_bytes(), 100);
        assert!(buffer.has_headers());

        let data = buffer.try_get_range(0, Some(50)).unwrap();
        assert_eq!(data.len(), 50);
    }

    #[test]
    fn test_chunk_buffer_lru_eviction() {
        let mut buffer = ChunkBuffer::new(2, 1024);

        buffer.add_chunk(create_test_chunk(0, 100, false)).unwrap();
        buffer
            .add_chunk(create_test_chunk(100, 100, false))
            .unwrap();
        buffer
            .add_chunk(create_test_chunk(200, 100, false))
            .unwrap();

        // First chunk should be evicted
        assert_eq!(buffer.chunks.len(), 2);
        assert_eq!(buffer.stats.evicted_chunks, 1);
    }

    #[test]
    fn test_chunk_buffer_range_error() {
        let mut buffer = ChunkBuffer::new(10, 1024);
        buffer.add_chunk(create_test_chunk(0, 100, false)).unwrap();

        // Request beyond available data
        let result = buffer.try_get_range(150, Some(200));
        assert!(matches!(
            result,
            Err(ChunkServerError::ChunkNotAvailable { .. })
        ));
    }

    #[tokio::test]
    async fn test_chunk_server_basic_operations() {
        let info_hash = InfoHash::new([1u8; 20]);
        let server = ChunkServer::new(info_hash, 10, 128 * 1024);

        let chunk = create_test_chunk(0, 64 * 1024, true);
        server.add_chunk(chunk).await.unwrap();

        assert!(server.is_ready_for_playback().await);

        let request = ChunkRequest {
            start: 0,
            end: Some(50),
            is_range_request: true,
            client_id: Some("test_client".to_string()),
        };

        let response = server.serve_request(request).await.unwrap();
        assert_eq!(response.data.len(), 50);
        assert!(response.is_partial);
        assert!(response.content_range.is_some());
    }

    #[tokio::test]
    async fn test_chunk_server_termination() {
        let info_hash = InfoHash::new([1u8; 20]);
        let server = ChunkServer::new(info_hash, 10, 1024);

        server.terminate().await;

        let chunk = create_test_chunk(0, 100, false);
        let result = server.add_chunk(chunk).await;
        assert!(matches!(
            result,
            Err(ChunkServerError::StreamTerminated { .. })
        ));
    }
}
