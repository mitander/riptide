# TODO: Progressive Streaming Architecture Redesign

## Problem Statement

The current progressive streaming implementation is fundamentally broken for its intended purpose. It downloads and transcodes the **entire file** before serving any content, defeating the purpose of "progressive" streaming.

**Current (Broken) Flow:**
```
Torrent Download → FFmpeg (complete transcode) → HTTP Response (after 100%)
     ↓                        ↓                      ↓
   100% file            Complete output         Start streaming
```

**Desired (Correct) Flow:**
```
Torrent Download → FFmpeg (real-time transcode) → HTTP Response (immediate)
     ↓                        ↓                         ↓
   5MB head              Real-time output          Start streaming
```

## Current Implementation Issues

1. **`StreamPump::pump_to`** waits for entire file: `while offset < self.file_size`
2. **`is_partial_file_ready`** only serves after transcoding completes
3. **No real-time serving** of transcoded chunks as they're produced
4. **85%+ download required** before streaming starts

## Target Architecture

### Phase 1: Real-Time Transcoding Pipeline

**Components:**
1. **DownloadManager** - Downloads pieces on-demand for streaming
2. **TranscodeStream** - FFmpeg process with real-time input/output
3. **ChunkServer** - Serves transcoded chunks as they're produced
4. **StreamCoordinator** - Orchestrates the pipeline

**Pipeline Flow:**
```
[TorrentEngine] → [DownloadManager] → [TranscodeStream] → [ChunkServer] → [HTTP Response]
                       ↓                    ↓                  ↓
                   On-demand pieces    Real-time transcode   Immediate serve
```

### Phase 2: Implementation Plan

#### Step 1: Refactor Progressive Streaming Core
- **File:** `riptide-core/src/streaming/progressive.rs`
- **Goal:** Split monolithic `pump_to` into real-time pipeline
- **New Components:**
  - `RealtimeTranscoder` - FFmpeg with streaming I/O
  - `ChunkBuffer` - Circular buffer for transcoded chunks
  - `StreamingCoordinator` - Manages the pipeline

#### Step 2: Implement Chunk-Based Serving
- **File:** `riptide-core/src/streaming/remux/chunk_server.rs` (new)
- **Goal:** Serve transcoded data as it's produced
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

## Testing Strategy

### Unit Tests (Per Component)

#### DownloadManager Tests
- **File:** `riptide-core/src/streaming/download_manager.rs`
- **Tests:**
  - Piece request prioritization
  - Stall detection and recovery
  - Integration with mock torrent engine

#### TranscodeStream Tests
- **File:** `riptide-core/src/streaming/transcoder.rs`
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
- **File:** `riptide-tests/integration/progressive_pipeline_test.rs`
- **Tests:**
  - End-to-end pipeline with mock data
  - Streaming startup time (<5MB download)
  - Real-time transcoding performance
  - Error propagation through pipeline

#### Torrent Integration
- **File:** `riptide-tests/integration/streaming_integration_test.rs`
- **Tests:**
  - Integration with real torrent engine
  - Piece availability and streaming coordination
  - Download priority and streaming position sync

### E2E Tests

#### Real-World Scenarios
- **File:** `riptide-tests/e2e/progressive_streaming_e2e.rs`
- **Tests:**
  - Stream AVI file with 5MB download
  - Stream MKV file with variable bitrate
  - Handle network interruptions gracefully
  - Multiple concurrent streams

#### Performance Benchmarks
- **File:** `riptide-tests/benchmarks/streaming_performance.rs`
- **Tests:**
  - Startup time benchmarks (target: <30s to start playback)
  - Memory usage during streaming
  - CPU usage for transcoding pipeline
  - Network efficiency (download vs stream position)

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
// Start serving immediately after header
if header_ready() {
    start_serving_transcoded_output();
}

// Continue feeding FFmpeg in background
tokio::spawn(async move {
    while let Some(chunk) = download_next_chunk().await {
        ffmpeg_stdin.write_all(&chunk).await?;
    }
});
```

### Chunk Buffer Strategy

**Buffer Design:**
- Circular buffer with 30-60 seconds of transcoded content
- Adaptive bitrate based on download speed
- Client stall detection and quality adjustment

**Implementation Location:**
- `riptide-core/src/streaming/buffer/`
- `ChunkBuffer` with ring buffer semantics
- `BufferStrategy` trait for different approaches

### State Machine Updates

**Current States:**
- `WaitingForHeadAndTail` → `Remuxing` → `Completed`

**New States:**
- `WaitingForHeader` → `Streaming` → `Completed`
- Add `StreamingWithBuffer` for active real-time streaming
- Add `StreamingStalled` for download issues

### Error Handling Strategy

**Graceful Degradation:**
1. **Download stalls** → Pause transcoding, buffer existing content
2. **FFmpeg errors** → Restart with different parameters
3. **Client disconnects** → Continue buffering for reconnection
4. **Piece unavailability** → Request from multiple peers

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

## Success Metrics

### Performance Targets
- **Startup Time:** <30 seconds from torrent start to playback
- **Download Requirement:** <10MB initial download for streaming
- **Buffer Health:** Maintain 30-60s ahead of playback position
- **CPU Usage:** <50% during active transcoding
- **Memory Usage:** <500MB per concurrent stream

### Quality Metrics
- **Playback Quality:** No buffering after initial startup
- **Error Recovery:** <5s to recover from network stalls
- **Concurrent Streams:** Support 3+ simultaneous streams
- **Format Support:** AVI, MKV, MP4 transcoding to MP4/WebM

## Files to Create/Modify

### New Files
- `riptide-core/src/streaming/download_manager.rs`
- `riptide-core/src/streaming/transcoder.rs`
- `riptide-core/src/streaming/chunk_server.rs`
- `riptide-core/src/streaming/buffer/mod.rs`
- `riptide-core/src/streaming/buffer/chunk_buffer.rs`
- `riptide-core/src/streaming/buffer/strategy.rs`

### Modified Files
- `riptide-core/src/streaming/progressive.rs` - Complete rewrite
- `riptide-core/src/streaming/remux/remuxer.rs` - Update session management
- `riptide-core/src/streaming/remux/state.rs` - Add streaming states
- `riptide-core/src/streaming/mod.rs` - Update main streaming logic

### Test Files
- `riptide-tests/integration/progressive_pipeline_test.rs`
- `riptide-tests/integration/streaming_integration_test.rs`
- `riptide-tests/e2e/progressive_streaming_e2e.rs`
- `riptide-tests/benchmarks/streaming_performance.rs`

## Next Session Pickup

**Start with:** Review this document and begin Phase 1 implementation
**Focus on:** `RealtimeTranscoder` component first (smallest, most testable)
**Test approach:** Unit tests for FFmpeg integration before pipeline integration
**Architecture decision:** Confirm buffer strategy and chunk size (suggest 1MB chunks)

**Key question to resolve:** Should we use FFmpeg's built-in fragmented MP4 output (`-movflags +frag_keyframe+empty_moov`) or implement our own chunking strategy?
