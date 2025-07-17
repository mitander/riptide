//! Direct streaming implementation for compatible media formats.
//!
//! This module provides a pass-through streaming strategy for media files
//! that are already in browser-compatible formats (MP4, WebM). It serves
//! byte ranges directly from the piece provider without any processing,
//! achieving optimal performance and minimal latency.

use std::sync::Arc;

use axum::body::Body;
use axum::http::{HeaderMap, HeaderValue, Request, StatusCode, header};
use axum::response::{IntoResponse, Response};
use bytes::Bytes;
use futures::{Stream, stream};

use crate::streaming::traits::{PieceProvider, PieceProviderError, StreamProducer};

/// Size of chunks to read from the piece provider.
///
/// This balances memory usage with streaming efficiency. Larger chunks
/// reduce overhead but increase memory usage and latency.
const CHUNK_SIZE: usize = 256 * 1024; // 256KB

/// Direct streaming producer for browser-compatible formats.
///
/// Serves media content directly from the piece provider without any
/// transcoding or remuxing. This is the most efficient streaming method
/// for formats that browsers can play natively (MP4, WebM).
pub struct DirectStreamProducer {
    /// The underlying piece provider for reading torrent data.
    provider: Arc<dyn PieceProvider>,
    /// MIME type of the media content.
    content_type: String,
}

impl DirectStreamProducer {
    /// Creates a new direct streaming producer.
    ///
    /// The content type should match the actual media format to ensure
    /// proper browser handling.
    pub fn new(provider: Arc<dyn PieceProvider>, content_type: String) -> Self {
        Self {
            provider,
            content_type,
        }
    }

    /// Parses an HTTP Range header to extract byte ranges.
    ///
    /// Supports the standard `bytes=start-end` format. Returns None if
    /// the header is missing or invalid.
    fn parse_range_header(headers: &HeaderMap) -> Option<(u64, Option<u64>)> {
        let range_header = headers.get(header::RANGE)?.to_str().ok()?;

        if !range_header.starts_with("bytes=") {
            return None;
        }

        let range_str = &range_header[6..]; // Skip "bytes="
        let parts: Vec<&str> = range_str.split('-').collect();

        if parts.len() != 2 {
            return None;
        }

        let start = parts[0].parse::<u64>().ok()?;
        let end = if parts[1].is_empty() {
            None
        } else {
            Some(parts[1].parse::<u64>().ok()?)
        };

        Some((start, end))
    }

    /// Creates a streaming body from a byte range.
    ///
    /// Returns a stream that yields chunks of data as they're read from
    /// the piece provider. This enables progressive playback without
    /// loading the entire file into memory.
    async fn create_range_stream(
        provider: Arc<dyn PieceProvider>,
        start: u64,
        length: u64,
    ) -> impl Stream<Item = Result<Bytes, std::io::Error>> {
        stream::unfold(
            (provider, start, start + length, 0u64),
            |(provider, start, end, offset)| async move {
                if offset >= end - start {
                    return None;
                }

                let chunk_start = start + offset;
                let chunk_size = std::cmp::min(CHUNK_SIZE as u64, end - chunk_start) as usize;

                match provider.read_at(chunk_start, chunk_size).await {
                    Ok(bytes) => {
                        let bytes_len = bytes.len() as u64;
                        Some((Ok(bytes), (provider, start, end, offset + bytes_len)))
                    }
                    Err(PieceProviderError::NotYetAvailable) => {
                        // For streaming, we convert this to an IO error
                        // that will cause the client to retry
                        Some((
                            Err(std::io::Error::new(
                                std::io::ErrorKind::WouldBlock,
                                "Data not yet available",
                            )),
                            (provider, start, end, offset),
                        ))
                    }
                    Err(e) => Some((
                        Err(std::io::Error::new(
                            std::io::ErrorKind::Other,
                            e.to_string(),
                        )),
                        (provider, start, end, offset),
                    )),
                }
            },
        )
    }
}

#[async_trait::async_trait]
impl StreamProducer for DirectStreamProducer {
    async fn produce_stream(&self, request: Request<()>) -> Response {
        let file_size = self.provider.size().await;
        let headers = request.headers();

        // Parse range header if present
        let range = Self::parse_range_header(headers);

        match range {
            Some((start, end)) => {
                // Handle range request
                let end = end.unwrap_or(file_size - 1);

                // Validate range
                if start >= file_size || end >= file_size || start > end {
                    return (
                        StatusCode::RANGE_NOT_SATISFIABLE,
                        [(
                            header::CONTENT_RANGE,
                            HeaderValue::from_str(&format!("bytes */{}", file_size))
                                .unwrap_or_else(|_| HeaderValue::from_static("bytes */0")),
                        )],
                    )
                        .into_response();
                }

                let length = end - start + 1;

                // Create streaming body
                let stream = Self::create_range_stream(self.provider.clone(), start, length).await;

                let body = Body::from_stream(stream);

                // Build response with appropriate headers
                Response::builder()
                    .status(StatusCode::PARTIAL_CONTENT)
                    .header(header::CONTENT_TYPE, &self.content_type)
                    .header(header::CONTENT_LENGTH, length.to_string())
                    .header(
                        header::CONTENT_RANGE,
                        format!("bytes {}-{}/{}", start, end, file_size),
                    )
                    .header(header::ACCEPT_RANGES, "bytes")
                    .header(header::CACHE_CONTROL, "public, max-age=3600")
                    .body(body)
                    .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response())
            }
            None => {
                // Handle full file request
                let stream = Self::create_range_stream(self.provider.clone(), 0, file_size).await;

                let body = Body::from_stream(stream);

                Response::builder()
                    .status(StatusCode::OK)
                    .header(header::CONTENT_TYPE, &self.content_type)
                    .header(header::CONTENT_LENGTH, file_size.to_string())
                    .header(header::ACCEPT_RANGES, "bytes")
                    .header(header::CACHE_CONTROL, "public, max-age=3600")
                    .body(body)
                    .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response())
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Mock implementation for testing
    struct MockPieceProvider {
        data: bytes::Bytes,
    }

    impl MockPieceProvider {
        fn new(data: bytes::Bytes) -> Self {
            Self { data }
        }
    }

    #[async_trait::async_trait]
    impl PieceProvider for MockPieceProvider {
        async fn read_at(
            &self,
            offset: u64,
            length: usize,
        ) -> Result<bytes::Bytes, PieceProviderError> {
            let file_size = self.data.len() as u64;
            if offset + length as u64 > file_size {
                return Err(PieceProviderError::InvalidRange {
                    offset,
                    length,
                    file_size,
                });
            }

            let start = offset as usize;
            let end = start + length;
            Ok(self.data.slice(start..end))
        }

        async fn size(&self) -> u64 {
            self.data.len() as u64
        }
    }

    #[tokio::test]
    async fn test_direct_stream_full_file() {
        // Create test data
        let data = Bytes::from(vec![0u8; 1024]);
        let provider = Arc::new(MockPieceProvider::new(data.clone()));
        let producer = DirectStreamProducer::new(provider, "video/mp4".to_string());

        // Create request without range header
        let request = Request::builder().body(()).unwrap();

        // Produce stream
        let response = producer.produce_stream(request).await;

        // Verify response
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.headers().get(header::CONTENT_LENGTH).unwrap(),
            "1024"
        );
        assert_eq!(
            response.headers().get(header::CONTENT_TYPE).unwrap(),
            "video/mp4"
        );
        assert_eq!(
            response.headers().get(header::ACCEPT_RANGES).unwrap(),
            "bytes"
        );

        // Verify body content
        let body_bytes = axum::body::to_bytes(response.into_body(), 2048)
            .await
            .unwrap();
        assert_eq!(body_bytes, data);
    }

    #[tokio::test]
    async fn test_direct_stream_range_request() {
        // Create test data
        let data = Bytes::from((0..100u8).collect::<Vec<_>>());
        let provider = Arc::new(MockPieceProvider::new(data));
        let producer = DirectStreamProducer::new(provider, "video/mp4".to_string());

        // Create range request for bytes 10-19
        let request = Request::builder()
            .header(header::RANGE, "bytes=10-19")
            .body(())
            .unwrap();

        // Produce stream
        let response = producer.produce_stream(request).await;

        // Verify response
        assert_eq!(response.status(), StatusCode::PARTIAL_CONTENT);
        assert_eq!(
            response.headers().get(header::CONTENT_LENGTH).unwrap(),
            "10"
        );
        assert_eq!(
            response.headers().get(header::CONTENT_RANGE).unwrap(),
            "bytes 10-19/100"
        );

        // Verify body content
        let body_bytes = axum::body::to_bytes(response.into_body(), 1024)
            .await
            .unwrap();
        assert_eq!(body_bytes, vec![10, 11, 12, 13, 14, 15, 16, 17, 18, 19]);
    }

    #[tokio::test]
    async fn test_direct_stream_open_ended_range() {
        // Create test data
        let data = Bytes::from(vec![0u8; 100]);
        let provider = Arc::new(MockPieceProvider::new(data));
        let producer = DirectStreamProducer::new(provider, "video/mp4".to_string());

        // Create open-ended range request
        let request = Request::builder()
            .header(header::RANGE, "bytes=90-")
            .body(())
            .unwrap();

        // Produce stream
        let response = producer.produce_stream(request).await;

        // Verify response
        assert_eq!(response.status(), StatusCode::PARTIAL_CONTENT);
        assert_eq!(
            response.headers().get(header::CONTENT_LENGTH).unwrap(),
            "10"
        );
        assert_eq!(
            response.headers().get(header::CONTENT_RANGE).unwrap(),
            "bytes 90-99/100"
        );
    }

    #[tokio::test]
    async fn test_direct_stream_invalid_range() {
        // Create test data
        let data = Bytes::from(vec![0u8; 100]);
        let provider = Arc::new(MockPieceProvider::new(data));
        let producer = DirectStreamProducer::new(provider, "video/mp4".to_string());

        // Create invalid range request
        let request = Request::builder()
            .header(header::RANGE, "bytes=150-200")
            .body(())
            .unwrap();

        // Produce stream
        let response = producer.produce_stream(request).await;

        // Verify error response
        assert_eq!(response.status(), StatusCode::RANGE_NOT_SATISFIABLE);
        assert_eq!(
            response.headers().get(header::CONTENT_RANGE).unwrap(),
            "bytes */100"
        );
    }

    #[test]
    fn test_parse_range_header() {
        // Valid range
        let mut headers = HeaderMap::new();
        headers.insert(header::RANGE, HeaderValue::from_static("bytes=10-20"));
        assert_eq!(
            DirectStreamProducer::parse_range_header(&headers),
            Some((10, Some(20)))
        );

        // Open-ended range
        headers.clear();
        headers.insert(header::RANGE, HeaderValue::from_static("bytes=50-"));
        assert_eq!(
            DirectStreamProducer::parse_range_header(&headers),
            Some((50, None))
        );

        // Invalid format
        headers.clear();
        headers.insert(header::RANGE, HeaderValue::from_static("invalid"));
        assert_eq!(DirectStreamProducer::parse_range_header(&headers), None);

        // No range header
        headers.clear();
        assert_eq!(DirectStreamProducer::parse_range_header(&headers), None);
    }
}
