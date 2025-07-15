# COMPLETED: Progressive Remuxing Architecture Redesign - Phase 1

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

### Phase 1: Real-Time Remuxing Pipeline ✅ COMPLETED

**Components:**

1. **DownloadManager** ✅ - Downloads pieces on-demand for streaming
2. **RealtimeRemuxer** ✅ - FFmpeg process with real-time input/output
3. **ChunkServer** ✅ - Serves remuxed chunks as they're produced
4. **ChunkBuffer** ✅ - Buffering with LRU eviction and statistics

**Pipeline Flow:**

```
[TorrentEngine] → [DownloadManager] → [RemuxStream] → [ChunkServer] → [HTTP Response]
                       ↓                    ↓                  ↓
                   On-demand pieces    Real-time remux    Immediate serve
```

### Phase 2: Implementation Plan ✅ COMPLETED

#### Step 1: Refactor Progressive Remuxing Core ✅ COMPLETED

- **File:** `riptide-core/src/streaming/realtime_remuxer.rs` ✅ CREATED
- **Goal:** Split monolithic `pump_to` into real-time pipeline ✅ COMPLETED
- **New Components:**
  - `RealtimeRemuxer` ✅ - FFmpeg with streaming I/O using `-c copy`
  - `ChunkBuffer` ✅ - Circular buffer for remuxed chunks with LRU eviction
  - `RemuxHandle` ✅ - Manages the remuxing process lifecycle

#### Step 2: Implement Chunk-Based Serving ✅ COMPLETED

- **File:** `riptide-core/src/streaming/chunk_server.rs` ✅ CREATED
- **Goal:** Serve remuxed data as it's produced ✅ COMPLETED
- **Features:**
  - Range request support for partial chunks ✅ IMPLEMENTED
  - Buffering strategy for smooth playback ✅ IMPLEMENTED
  - Progress tracking and client coordination ✅ IMPLEMENTED

#### Step 3: Download-on-Demand Integration ✅ COMPLETED

- **File:** `riptide-core/src/streaming/download_manager.rs` ✅ CREATED
- **Goal:** Request pieces based on streaming position ✅ COMPLETED
- **Features:**
  - Piece priority for streaming position + lookahead ✅ IMPLEMENTED
  - Stall detection and recovery ✅ IMPLEMENTED
  - Integration with torrent engine ✅ FRAMEWORK READY

#### Step 4: Update Session Management 🔄 NEXT PHASE

- **File:** `riptide-core/src/streaming/remux/remuxer.rs`
- **Goal:** Support real-time pipeline instead of batch processing
- **Changes:**
  - Replace `is_partial_file_ready` broken logic
  - Integrate real-time remuxing components
  - Update readiness detection for streaming chunks

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

### Unit Tests (Per Component) ✅ COMPLETED

#### DownloadManager Tests ✅ COMPLETED

- **File:** `riptide-core/src/streaming/download_manager.rs` ✅ 5 TESTS PASSING
- **Tests:**
  - Piece request prioritization ✅ IMPLEMENTED
  - Stall detection and recovery ✅ IMPLEMENTED
  - Priority recalculation ✅ IMPLEMENTED
  - Download statistics tracking ✅ IMPLEMENTED
  - Streaming position updates ✅ IMPLEMENTED

#### RealtimeRemuxer Tests ✅ COMPLETED

- **File:** `riptide-core/src/streaming/realtime_remuxer.rs` ✅ 7 TESTS PASSING
- **Tests:**
  - FFmpeg configuration validation ✅ IMPLEMENTED
  - Command argument generation ✅ IMPLEMENTED
  - MP4 header detection ✅ IMPLEMENTED
  - Remux status transitions ✅ IMPLEMENTED
  - Configuration presets (AVI, low-latency) ✅ IMPLEMENTED

#### ChunkServer Tests ✅ COMPLETED

- **File:** `riptide-core/src/streaming/chunk_server.rs` ✅ 5 TESTS PASSING
- **Tests:**
  - Range request handling ✅ IMPLEMENTED
  - Buffer management with LRU eviction ✅ IMPLEMENTED
  - Client connection handling ✅ IMPLEMENTED
  - Chunk server lifecycle ✅ IMPLEMENTED
  - Playback readiness detection ✅ IMPLEMENTED

### Integration Tests 🔄 NEXT PHASE

#### Pipeline Integration 📝 TODO

- **File:** `riptide-tests/integration/progressive_remuxing_test.rs`
- **Tests:**
  - End-to-end pipeline with mock data
  - Streaming startup time (<2MB download)
  - Real-time remuxing performance
  - Error propagation through pipeline

#### Torrent Integration 📝 TODO

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

- `riptide-core/src/streaming/progressive.rs` - 🔄 NEEDS REPLACEMENT with real-time pipeline
- `riptide-core/src/streaming/remux/remuxer.rs` - 🔄 NEEDS UPDATE for session management
- `riptide-core/src/streaming/remux/state.rs` - 🔄 NEEDS UPDATE to add streaming states
- `riptide-core/src/streaming/mod.rs` - ✅ UPDATED with new modules

### New Files ✅ COMPLETED

- `riptide-core/src/streaming/download_manager.rs` ✅ CREATED
- `riptide-core/src/streaming/realtime_remuxer.rs` ✅ CREATED
- `riptide-core/src/streaming/chunk_server.rs` ✅ CREATED
- `riptide-core/src/streaming/remux/mod.rs` ✅ UPDATED with new exports

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

**Current Status:** ✅ Phase 1 complete - All real-time components implemented and tested

**Next Priority:** 🔄 Phase 2 - Integration with existing remux system
**Focus on:** Replace broken `is_partial_file_ready` logic in `remuxer.rs`
**Test approach:** Integration tests for complete pipeline functionality
**Architecture decision:** ✅ RESOLVED - Using FFmpeg fragmented MP4 (`-movflags +frag_keyframe+empty_moov`) with 256KB chunks

**Key integration tasks:**

1. Replace `progressive.rs` batch processing with real-time pipeline
2. Update `remuxer.rs` to use `ChunkServer` for readiness detection
3. Integrate `DownloadManager` with torrent engine
4. Add streaming state management for real-time chunks

**Performance goal:** ✅ ARCHITECTURE READY - Components designed for <2MB download requirement
