# Progressive Streaming Architecture Refactor

## Problem Statement

The current streaming implementation defeats its own purpose. It waits for complete file download and remuxing before serving any content, requiring 85%+ download before streaming starts.

**Current (Broken) Flow:**

```
Torrent Download → FFmpeg (complete remux) → HTTP Response (after 100%)
```

**New (Correct) Flow:**

```
Torrent Download → FFmpeg (real-time pipe) → HTTP Response (immediate)
```

## New Architecture: Stateless Pipeline

Replacing the complex state machine with a simple, unidirectional pipeline:

```
PieceProvider → StreamProducer → HTTP Response
```

### Core Abstractions

1. **PieceProvider** - File-like interface over torrent storage
   - `async fn read_at(offset, length) -> Bytes`
   - Abstracts piece assembly into linear byte reads

2. **StreamProducer** - HTTP response generator
   - `async fn produce_stream(request) -> Response`
   - Two implementations: DirectStream (MP4) and RemuxStream (MKV/AVI)

## Implementation Progress

### ✅ Completed (Commit 1)

**Files Created:**

- `riptide-core/src/streaming/traits.rs` - Core abstractions
- `riptide-sim/src/streaming/mock_piece_provider.rs` - Test implementation

**Key Achievements:**

- Clean separation between torrenting and streaming
- Deterministic testing capability
- 6 comprehensive unit tests for MockPieceProvider

### 🔄 In Progress (Commit 2)

**Files Created:**

- `riptide-core/src/streaming/direct_stream.rs` - Pass-through for MP4/WebM

**Features:**

- HTTP range request support
- Zero-copy streaming from pieces to HTTP response
- Proper 206 Partial Content handling

### 📋 Next Steps

#### Commit 3: RemuxStreamProducer

- Real-time FFmpeg pipeline with pipe I/O
- Fragmented MP4 output (`-movflags frag_keyframe+empty_moov`)
- Async input pump feeding from PieceProvider
- Direct stdout → HTTP body streaming

#### Commit 4: Simulation Tests

- End-to-end test with real MKV → MP4 remuxing
- MP4 structure
  validation
- Edge case handling (failures, empty files)

#### Commit 5: Factory & Cleanup

- Media detection (`media_info.rs`)
- Factory function selecting appropriate producer
- **DELETE:** All obsolete modules
  - `remux/`, `progressive.rs`, `chunk_server.rs`, `buffer/`, `realtime_remuxer.rs`

#### Commit 6: Integration

- Update HTTP handlers to use new pipeline
- Fix existing range handling bugs
- Update documentation

## Key Implementation Details

### RemuxStream FFmpeg Command

```bash
ffmpeg -hide_banner -loglevel error \
       -i pipe:0 \
       -c:v copy -c:a copy \
       -movflags frag_keyframe+empty_moov+default_base_moof \
       -f mp4 \
       pipe:1
```

### Input Pump Pattern

```rust
tokio::spawn(async move {
    let mut offset = 0;
    while offset < file_size {
        match provider.read_at(offset, CHUNK_SIZE).await {
            Ok(bytes) => {
                stdin.write_all(&bytes).await?;
                offset += bytes.len() as u64;
            }
            Err(NotYetAvailable) => sleep(RETRY_DELAY).await,
            Err(e) => break,
        }
    }
});
```

### Zero-Buffer Output

```rust
let stdout = child.stdout.take().unwrap();
let stream = ReaderStream::new(stdout);
let body = Body::from_stream(stream);
```

## Success Metrics

- **Startup Time:** <2MB download to start streaming
- **Memory Usage:** No in-app buffering of media data
- **Code Reduction:** ~2000 lines deleted
- **Test Coverage:** 100% of new pipeline code
