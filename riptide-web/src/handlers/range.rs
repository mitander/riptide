//! HTTP Range request handling for video streaming
//!
//! Implements RFC 7233 HTTP Range Requests for efficient video streaming
//! with proper Content-Range and partial content responses.

use axum::body::Body;
use axum::http::{HeaderMap, StatusCode};
use axum::response::Response;

/// Parse HTTP Range header and return (start, end, content_length)
///
/// Handles standard "bytes=start-end" format with proper boundary checking.
/// Returns full range if header is missing or invalid.
///
/// # Examples
/// ```
/// use riptide_web::handlers::range::parse_range_header;
/// let (start, end, length) = parse_range_header("bytes=100-199", 1000);
/// assert_eq!((start, end, length), (100, 199, 100));
/// ```
pub fn parse_range_header(range: &str, total_size: u64) -> (u64, u64, u64) {
    if !range.starts_with("bytes=") {
        return (0, total_size.saturating_sub(1), total_size);
    }

    let range_spec = &range[6..]; // Remove "bytes="
    if let Some((start_str, end_str)) = range_spec.split_once('-') {
        let start = start_str.parse::<u64>().unwrap_or(0);
        let end = if end_str.is_empty() {
            total_size.saturating_sub(1)
        } else {
            end_str
                .parse::<u64>()
                .unwrap_or(total_size.saturating_sub(1))
        };
        let content_length = end.saturating_sub(start) + 1;
        (start, end, content_length)
    } else {
        (0, total_size.saturating_sub(1), total_size)
    }
}

/// Build HTTP response for video streaming with proper range headers
///
/// Creates appropriate response based on whether this is a range request,
/// including Content-Range headers for partial content responses.
///
/// # Errors
/// Returns StatusCode error if response building fails
#[allow(clippy::too_many_arguments)]
pub fn build_range_response(
    headers: &HeaderMap,
    video_data: Vec<u8>,
    content_type: &str,
    start: u64,
    end: u64,
    total_size: u64,
) -> Result<Response<Body>, StatusCode> {
    let has_range_header = headers.get("range").is_some();
    let data_length = video_data.len();

    let mut response = Response::builder()
        .header("Content-Type", content_type)
        .header("Accept-Ranges", "bytes")
        .header("Content-Length", data_length.to_string())
        .header("Cache-Control", "no-cache");

    // Add range response headers if this is a range request
    if has_range_header {
        response = response
            .status(StatusCode::PARTIAL_CONTENT)
            .header("Content-Range", format!("bytes {start}-{end}/{total_size}"));
    } else {
        response = response.status(StatusCode::OK);
    }

    response
        .body(Body::from(video_data))
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}

/// Validate range request bounds and return safe range
///
/// Ensures the requested range is valid and doesn't exceed available data.
/// Returns adjusted range that's safe to serve.
///
/// # Errors
/// Returns RANGE_NOT_SATISFIABLE if start position exceeds available size
pub fn validate_range_bounds(
    start: u64,
    end: u64,
    available_size: u64,
) -> Result<(u64, u64, u64), StatusCode> {
    if start > available_size {
        return Err(StatusCode::RANGE_NOT_SATISFIABLE);
    }

    let safe_end = end.min(available_size.saturating_sub(1));
    let safe_length = safe_end.saturating_sub(start) + 1;

    Ok((start, safe_end, safe_length))
}

/// Extract and parse Range header from HTTP headers
///
/// Returns None if no range header present or if header value is invalid.
/// Handles proper UTF-8 conversion and header value parsing.
pub fn extract_range_header(headers: &HeaderMap) -> Option<String> {
    headers
        .get("range")
        .and_then(|range| range.to_str().ok())
        .map(|s| s.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_range_header_valid() {
        let (start, end, length) = parse_range_header("bytes=100-199", 1000);
        assert_eq!((start, end, length), (100, 199, 100));
    }

    #[test]
    fn test_parse_range_header_open_end() {
        let (start, end, length) = parse_range_header("bytes=500-", 1000);
        assert_eq!((start, end, length), (500, 999, 500));
    }

    #[test]
    fn test_parse_range_header_invalid() {
        let (start, end, length) = parse_range_header("invalid", 1000);
        assert_eq!((start, end, length), (0, 999, 1000));
    }

    #[test]
    fn test_validate_range_bounds_valid() {
        let result = validate_range_bounds(100, 199, 1000);
        assert!(result.is_ok());
        let (start, end, length) = result.unwrap();
        assert_eq!((start, end, length), (100, 199, 100));
    }

    #[test]
    fn test_validate_range_bounds_exceeds_available() {
        let result = validate_range_bounds(500, 599, 400);
        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), StatusCode::RANGE_NOT_SATISFIABLE);
    }

    #[test]
    fn test_validate_range_bounds_clamps_end() {
        let result = validate_range_bounds(100, 999, 500);
        assert!(result.is_ok());
        let (start, end, length) = result.unwrap();
        assert_eq!((start, end, length), (100, 499, 400));
    }
}
