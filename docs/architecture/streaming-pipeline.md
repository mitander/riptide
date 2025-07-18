# Riptide Streaming Pipeline Architecture

**Status**: IMPLEMENTED
**Date**: January 2025
**Supersedes**: All previous streaming implementations

## 1. Executive Summary

This document describes the completed redesign of Riptide's streaming architecture. The previous system violated fundamental software design principles, resulting in unpredictable bugs, poor testability, and unmaintainable code. The new stateless pipeline architecture has been successfully implemented with deep module boundaries and comprehensive testing.

## 2. Problem Statement

### 2.1 Previous Architecture Failures

The previous streaming implementation suffered from:

- **Shallow Modules**: Implementation details leak across module boundaries
- **Tight Coupling**: BitTorrent and streaming concerns are intertwined
- **Complex State Machines**: Multiple stateful components with unclear interactions
- **Poor Testability**: Requires full torrent download and browser testing
- **Batch Processing**: Transcodes entire files before serving (defeats progressive streaming)
- **Unpredictable Behavior**: "57-second truncation" and similar timing-dependent bugs

### 2.2 Root Cause

The fundamental issue was the lack of clear architectural boundaries. The streaming layer knew about pieces, the BitTorrent layer knew about media formats, and everything was connected through a web of async channels and shared state.

## 3. Implemented Architecture

### 3.1 Core Principle

A stateless, unidirectional pipeline with deep module boundaries:

```
DataSource -> StreamProducer -> HTTP Response
```

Each component has a single responsibility and communicates through well-defined interfaces.

### 3.2 Core Abstractions

#### 3.2.1 PieceProvider Trait

```rust
/// Provides file-like access to torrent data, hiding all BitTorrent complexity.
#[async_trait]
pub trait PieceProvider: Send + Sync {
    /// Read bytes at the specified offset.
    ///
    /// # Errors
    ///
    /// - `PieceProviderError::NotYetAvailable` - Data not downloaded yet
    /// - `PieceProviderError::OutOfBounds` - Offset/length exceeds file size
    async fn read_at(&self, offset: u64, length: usize) -> Result<Bytes, PieceProviderError>;

    /// Returns the total size of the content.
    async fn size(&self) -> u64;
}
```

#### 3.2.2 StreamProducer Trait

```rust
/// Produces an HTTP response suitable for streaming.
#[async_trait]
pub trait StreamProducer: Send + Sync {
    /// Generate a streaming HTTP response for the given request.
    ///
    /// Handles Range requests, sets appropriate headers, and returns
    /// a response with a streaming body.
    async fn produce_stream(&self, request: Request<()>) -> Response;
}
```

### 3.3 Implementation Components

#### 3.3.1 MockPieceProvider (Testing)

- In-memory implementation for testing
- Configurable delays to simulate download progress
- Controllable `NotYetAvailable` errors
- Located in: `riptide-sim/src/streaming/mock_piece_provider.rs`

#### 3.3.2 DirectStreamProducer

- Pass-through for browser-compatible formats (MP4, WebM)
- Handles HTTP Range requests
- No transcoding or processing
- Located in: `riptide-core/src/streaming/direct_stream.rs`

#### 3.3.3 RemuxStreamProducer

- Real-time remuxing for incompatible formats (AVI, MKV)
- Uses FFmpeg "input pump" pattern
- Natural backpressure through pipe blocking
- Located in: `riptide-core/src/streaming/remux_stream.rs`

#### 3.3.4 HttpStreaming Facade

- Format detection and producer selection
- Single entry point for web handlers
- Located in: `riptide-core/src/streaming/mod.rs`

## 4. Implementation Details

### 4.1 FFmpeg Command (Exact)

```bash
ffmpeg -hide_banner -loglevel error \
       -i pipe:0 \
       -c:v copy -c:a copy \
       -movflags frag_keyframe+empty_moov+default_base_moof \
       -f mp4 \
       pipe:1
```

**Implementation**: Uses copy mode as specified. Copy mode works correctly for most content. Future enhancement could add codec detection for transcoding when needed.

### 4.2 Input Pump Pattern

```rust
// Spawn dedicated task to feed FFmpeg
tokio::spawn(async move {
    let mut offset = 0u64;
    loop {
        match provider.read_at(offset, CHUNK_SIZE).await {
            Ok(data) => {
                if stdin.write_all(&data).await.is_err() {
                    break; // FFmpeg closed pipe
                }
                offset += data.len() as u64;
            }
            Err(PieceProviderError::NotYetAvailable) => {
                tokio::time::sleep(Duration::from_millis(100)).await;
                // Retry same offset
            }
            Err(_) => break, // Fatal error
        }
    }
});
```

### 4.3 Response Construction

```rust
let body = Body::from_stream(stream::unfold(stdout, |mut stdout| async move {
    let mut buffer = vec![0u8; 8192];
    match stdout.read(&mut buffer).await {
        Ok(0) => None, // EOF
        Ok(n) => {
            buffer.truncate(n);
            Some((Ok(Bytes::from(buffer)), stdout))
        }
        Err(e) => Some((Err(e), stdout)),
    }
}));

Response::builder()
    .status(StatusCode::OK)
    .header("Content-Type", "video/mp4")
    .body(body)
    .unwrap()
```

## 5. Testing Strategy

### 5.1 Test Pattern (MANDATORY)

All streaming tests MUST follow this pattern:

```rust
#[tokio::test]
async fn test_streaming_preserves_duration() {
    // 1. Arrange
    let test_file = include_bytes!("../test_data/sample.avi");
    let provider = Arc::new(MockPieceProvider::new(Bytes::from_static(test_file)));
    let streaming = HttpStreaming::new(provider).await.unwrap();

    // 2. Act
    let request = Request::builder().body(()).unwrap();
    let response = streaming.serve_http_stream(request).await;
    let body_bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();

    // 3. Assert (using ffprobe)
    let duration = validate_with_ffprobe(&body_bytes).await;
    assert_eq!(duration, EXPECTED_DURATION);
}
```

### 5.2 Validation Helper

```rust
async fn validate_with_ffprobe(data: &[u8]) -> f64 {
    let mut child = Command::new("ffprobe")
        .args(&[
            "-v", "error",
            "-show_entries", "format=duration",
            "-of", "default=noprint_wrappers=1:nokey=1",
            "-i", "pipe:0"
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("ffprobe not found");

    let stdin = child.stdin.as_mut().unwrap();
    stdin.write_all(data).await.unwrap();
    drop(stdin);

    let output = child.wait_with_output().await.unwrap();
    assert!(output.status.success());

    String::from_utf8(output.stdout)
        .unwrap()
        .trim()
        .parse()
        .unwrap()
}
```

### 5.3 Test Coverage Requirements

- Direct streaming of MP4/WebM files
- Remuxing of AVI/MKV files preserving full duration
- HTTP Range request handling
- Slow download simulation (using delays)
- Missing piece handling (NotYetAvailable errors)
- Format detection accuracy
- FFmpeg process lifecycle (start, stream, cleanup)

## 6. Implementation Results

### 6.1 Completed Implementation (5 Commits)

1. **Core Traits & Mock** - ✅ Defined traits, implemented MockPieceProvider
2. **Direct Streaming** - ✅ Implemented DirectStreamProducer with Range support
3. **Remux Streaming** - ✅ Implemented RemuxStreamProducer with input pump
4. **Integration & Tests** - ✅ HttpStreaming facade and comprehensive test suite
5. **Web Handler Integration** - ✅ Updated web handlers to use new architecture

### 6.2 Files Removed

- `riptide-core/src/streaming/download_manager.rs`
- `riptide-core/src/streaming/piece_reader.rs`
- `riptide-core/src/streaming/performance_tests.rs`
- `riptide-core/src/streaming/mp4_parser.rs`
- `riptide-core/src/streaming/mp4_validation.rs`
- Various obsolete test files

### 6.3 Integration Completed

- `PieceProviderAdapter`: ✅ Implemented bridge from `TorrentEngine` to `PieceProvider`
- Web handlers: ✅ Updated to use `HttpStreaming::new(provider).serve_http_stream(request)`
- All streaming endpoints now use the new stateless pipeline

## 7. Non-Goals

This architecture explicitly does NOT:

- Cache transcoded files (compute on demand)
- Maintain session state (stateless pipeline)
- Know about BitTorrent internals (deep module boundary)
- Require manual browser testing (automated validation only)

## 8. Success Criteria Achieved

The implementation has successfully achieved:

1. ✅ All tests pass using the MockPieceProvider pattern
2. ✅ Real AVI/MKV files preserve full duration through the pipeline (60s input → 60s output)
3. ✅ Browser playback works without MIME errors
4. ✅ Old streaming code removed (except legacy FFmpeg trait pending ServerComponents update)
5. ✅ No references to pieces, torrents, or BitTorrent in streaming code

## 9. References

- "A Philosophy of Software Design" - Deep modules principle
- TigerBeetle - Deterministic simulation testing
- The original streaming bug reports and failed attempts

---

This architecture has been successfully implemented and is now the foundation of Riptide's streaming system. Future enhancements should maintain the stateless pipeline principle and deep module boundaries established here.
