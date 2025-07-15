# TODO: Progressive Remuxing Architecture Redesign

## Problem Statement

The current progressive streaming implementation is fundamentally broken for its intended purpose. It downloads and **remuxes the entire file** before serving any content, defeating the purpose of "progressive" streaming.

**Current (Broken) Flow:**

```
Torrent Download → FFmpeg (complete remux) → HTTP Response (after 100%)
     ↓                        ↓                      ↓
   100% file            Complete output         Start streaming
```

**Desired (Correct) Flow:**

```
Torrent Download → FFmpeg (real-time remux) → HTTP Response (immediate)
     ↓                        ↓                         ↓
   1-2MB head           Real-time output          Start streaming
```

## Current Implementation Issues

1. **`StreamPump::pump_to`** waits for entire file: `while offset < self.file_size`
2. **`is_partial_file_ready`** only serves after remuxing completes
3. **No real-time serving** of remuxed chunks as they're produced
4. **85%+ download required** before streaming starts
5. **Batch processing** instead of streaming pipeline

## Why Remuxing Over Transcoding

**Remuxing** (container format change only):

- Extremely fast (near real-time)
- Preserves original quality
- Low CPU usage
- Can start after 1-2MB (just container headers)
- Uses `ffmpeg -c:v copy -c:a copy`

**Transcoding** (re-encoding streams):

- Very CPU intensive
- Takes significant time
- Can degrade quality
- Should only be used when necessary

Most torrents have H.264 video and AAC audio that just need container conversion (AVI→MP4, MKV→MP4).

## Target Architecture

### Phase 1: Real-Time Remuxing Pipeline

**Components:**

1. **DownloadManager** - Downloads pieces on-demand for streaming
2. **RemuxStream** - FFmpeg process with real-time input/output
3. **ChunkServer** - Serves remuxed chunks as they're produced
4. **StreamCoordinator** - Orchestrates the pipeline

**Pipeline Flow:**

```
[TorrentEngine] → [DownloadManager] → [RemuxStream] → [ChunkServer] → [HTTP Response]
                       ↓                    ↓                  ↓
                   On-demand pieces    Real-time remux    Immediate serve
```

### Phase 2: Implementation Plan

#### Step 1: Refactor Progressive Remuxing Core

- **File:** `riptide-core/src/streaming/progressive.rs`
- **Goal:** Split monolithic `pump_to` into real-time pipeline
- **New Components:**
  - `RealtimeRemuxer` - FFmpeg with streaming I/O using `-c copy`
  - `ChunkBuffer` - Circular buffer for remuxed chunks
  - `RemuxingCoordinator` - Manages the pipeline

#### Step 2: Implement Chunk-Based Serving

- **File:** `riptide-core/src/streaming/remux/chunk_server.rs` (new)
- **Goal:** Serve remuxed data as it's produced
- **Features:**
  - Range request support for partial chunks
  - Buffering strategy for smooth playback
  - Progress tracking and client coordination

#### Step 3: Download-on-Demand Integration

- **File:** `riptide-core/src/streaming/download_manager.rs` (new)
- **Goal:** Request pieces based on streaming position
- **Features:**
  - Piece priority for streaming position + lookahead
  - Stall detection and recovery
  - Integration with torrent engine

#### Step 4: Update Session Management

- **File:** `riptide-core/src/streaming/remux/remuxer.rs`
- **Goal:** Support real-time pipeline instead of batch processing
- **Changes:**
  - Remove "wait for complete" logic
  - Add pipeline state management
  - Update readiness detection

## Technical Implementation Details

### Real-Time FFmpeg Integration

**Current Issue:**

```rust
// This waits for entire file
while offset < self.file_size {
    let data = self.read_chunk(offset, chunk_end)?;
    writer.write_all(&data)?;
    offset += data.len() as u64;
}
```

**Target Solution:**

```rust
// Start serving immediately after container headers
if container_headers_ready() {
    start_serving_remuxed_output();
}

// Continue feeding FFmpeg in background
tokio::spawn(async move {
    while let Some(chunk) = download_next_chunk().await {
        ffmpeg_stdin.write_all(&chunk).await?;
    }
});
```

### FFmpeg Command Changes

**Current (slow transcoding):**

```bash
ffmpeg -i pipe:0 -c:v libx264 -c:a aac -movflags frag_keyframe+empty_moov pipe:1
```

**Target (fast remuxing):**

```bash
ffmpeg -i pipe:0 -c:v copy -c:a copy -movflags frag_keyframe+empty_moov -f mp4 pipe:1
```

### Chunk Buffer Strategy

**Buffer Design:**

- Circular buffer with 10-30 seconds of remuxed content
- Much smaller than transcoding (since remuxing is faster)
- Client stall detection and recovery

**Implementation Location:**

- `riptide-core/src/streaming/buffer/`
- `ChunkBuffer` with ring buffer semantics
- `BufferStrategy` trait for different approaches

### State Machine Updates

**Current States:**

- `WaitingForHeadAndTail` → `Remuxing` → `Completed`

**New States:**

- `WaitingForHeaders` → `Streaming` → `Completed`
- Add `StreamingWithBuffer` for active real-time remuxing
- Add `StreamingStalled` for download issues

## Testing Strategy

### Unit Tests (Per Component)

#### DownloadManager Tests

- **File:** `riptide-core/src/streaming/download_manager.rs`
- **Tests:**
  - Piece request prioritization
  - Stall detection and recovery
  - Integration with mock torrent engine

#### RealtimeRemuxer Tests

- **File:** `riptide-core/src/streaming/realtime_remuxer.rs`
- **Tests:**
  - FFmpeg process lifecycle
  - Real-time input/output handling
  - Error handling and recovery

#### ChunkServer Tests

- **File:** `riptide-core/src/streaming/chunk_server.rs`
- **Tests:**
  - Range request handling
  - Buffer management
  - Client connection handling

### Integration Tests

#### Pipeline Integration

- **File:** `riptide-tests/integration/progressive_remuxing_test.rs`
- **Tests:**
  - End-to-end pipeline with mock data
  - Streaming startup time (<2MB download)
  - Real-time remuxing performance
  - Error propagation through pipeline

#### Torrent Integration

- **File:** `riptide-tests/integration/streaming_integration_test.rs`
- **Tests:**
  - Integration with real torrent engine
  - Piece availability and streaming coordination
  - Download priority and streaming position sync

### E2E Tests

#### Real-World Scenarios

- **File:** `riptide-tests/e2e/progressive_remuxing_e2e.rs`
- **Tests:**
  - Stream AVI file with 2MB download
  - Stream MKV file with variable bitrate
  - Handle network interruptions gracefully
  - Multiple concurrent streams

#### Performance Benchmarks

- **File:** `riptide-tests/benchmarks/remuxing_performance.rs`
- **Tests:**
  - Startup time benchmarks (target: <15s to start playback)
  - Memory usage during streaming
  - CPU usage for remuxing pipeline
  - Network efficiency (download vs stream position)

## Success Metrics

### Performance Targets

- **Startup Time:** <15 seconds from torrent start to playback
- **Download Requirement:** <2MB initial download for streaming
- **Buffer Health:** Maintain 10-30s ahead of playback position
- **CPU Usage:** <20% during active remuxing (vs 50%+ for transcoding)
- **Memory Usage:** <200MB per concurrent stream

### Quality Metrics

- **Playback Quality:** No buffering after initial startup
- **Error Recovery:** <3s to recover from network stalls
- **Concurrent Streams:** Support 5+ simultaneous streams
- **Format Support:** AVI, MKV, MP4 remuxing to fragmented MP4

## Files to Create/Modify

### Modified Files

- `riptide-core/src/streaming/progressive.rs` - Complete rewrite for real-time pipeline
- `riptide-core/src/streaming/remux/remuxer.rs` - Update session management
- `riptide-core/src/streaming/remux/state.rs` - Add streaming states
- `riptide-core/src/streaming/mod.rs` - Update main streaming logic

### New Files

- `riptide-core/src/streaming/download_manager.rs`
- `riptide-core/src/streaming/realtime_remuxer.rs`
- `riptide-core/src/streaming/chunk_server.rs`
- `riptide-core/src/streaming/buffer/mod.rs`
- `riptide-core/src/streaming/buffer/chunk_buffer.rs`

### Test Files

- `riptide-tests/integration/progressive_remuxing_test.rs`
- `riptide-tests/integration/streaming_integration_test.rs`
- `riptide-tests/e2e/progressive_remuxing_e2e.rs`
- `riptide-tests/benchmarks/remuxing_performance.rs`

## Migration Strategy

### Phase 1: Parallel Implementation

- Implement new architecture alongside existing
- Feature flag to switch between implementations
- Gradual rollout with A/B testing

### Phase 2: Testing and Validation

- Comprehensive test suite for new implementation
- Performance benchmarking against current system
- Edge case validation (network issues, corrupted data)

### Phase 3: Cutover

- Remove old implementation
- Update all references to new architecture
- Documentation updates

## Next Session Pickup

**Start with:** Review this document and begin Phase 1 implementation
**Focus on:** `RealtimeRemuxer` component first (smallest, most testable)
**Test approach:** Unit tests for FFmpeg remuxing integration before pipeline integration
**Architecture decision:** Confirm buffer strategy and chunk size (suggest 256KB-1MB chunks)

**Key question to resolve:** Should we use FFmpeg's built-in fragmented MP4 output (`-movflags +frag_keyframe+empty_moov`) or implement our own chunking strategy?

**Performance goal:** Get streaming working with <2MB download requirement (vs current 85%+ requirement).
