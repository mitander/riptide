# Riptide - Design Summary

## Architecture Overview

A Rust-based torrent media server focused on streaming performance and rapid iteration. Ships MVP in 2 weeks, production-ready in 16 weeks.

### Core Components

```
API Gateway      → HTTP interface, authentication, rate limiting
Torrent Engine   → BitTorrent protocol, piece management, peer connections
Storage Layer    → File organization, reflink/CoW support
Streaming Service → Direct streaming, pre-transcoding, HLS generation
Media Service    → Metadata, library management, search
Subtitle Service → OpenSubtitles integration, sync adjustment
```

## Key Design Decisions

### 1. Torrent Implementation

**Choice**: Build on `bittorrent-protocol` crate, extend for streaming.

**Rationale**: Pure Rust, modern async, no FFI issues. Implement streaming-specific piece selection algorithm for sequential playback.

### 2. Storage Architecture

**Choice**: Simple directory structure with copy-on-write where available.

```
/media/library/{movie_id}/      # Completed movies
/media/downloads/{info_hash}/   # Active downloads
```

**Implementation**: Try reflink → hard link → move. No complex content-addressing.

### 3. Streaming Strategy

**Choice**: Direct streaming by default, pre-transcode during download for incompatible formats.

**Rationale**: Eliminates buffering/stuttering. Most devices support H.264/H.265 directly.

### 4. Database Design

```sql
-- Optimized schema with denormalized hot fields
CREATE TABLE movies (
    id BIGSERIAL PRIMARY KEY,      -- Better index performance than UUID
    tmdb_id INTEGER UNIQUE NOT NULL,
    title TEXT NOT NULL,
    year SMALLINT NOT NULL,
    rating DECIMAL(3,1),
    -- Denormalized for query performance
    file_path TEXT NOT NULL,
    video_codec VARCHAR(20),
    audio_codec VARCHAR(20)
);
```

## Critical Components

### Streaming Piece Picker

```rust
pub struct StreamingPiecePicker {
    playback_position: AtomicU64,
    buffer_ahead: usize,
    deadline_heap: Mutex<BinaryHeap<PieceDeadline>>,
}
```

Prioritizes pieces near playback position to maintain buffer.

### Bandwidth Management

```rust
pub struct BandwidthScheduler {
    rules: Vec<TimeBasedRule>,     // ISP peak/off-peak
    current_limit: AtomicU64,
}
```

### VPN Integration

```rust
pub struct VpnDetector {
    interface_monitor: InterfaceMonitor,
    dns_leak_checker: DnsLeakChecker,
}
```

Kill switch if VPN disconnects during torrent activity.

## Development Approach

### Phase 1: Core (Weeks 1-2)
- Basic torrent downloading
- Simple file storage
- CLI interface
- **Ship to self for testing**

### Phase 2: Web UI (Week 3)
- Browse library
- Start/stop downloads
- View progress
- **Get family using it**

### Phase 3: Streaming (Weeks 4-6)
- Direct streaming first
- Device detection
- Pre-transcoding

### Phase 4: Production (Weeks 7-16)
- Subtitles
- Apple TV app
- Performance optimization
- Monitoring

## Performance Strategy

### Zero-Allocation Streaming Path

```rust
pub struct StreamingService {
    // Pre-allocated buffers
    segment_buffer: Box<[u8; SEGMENT_SIZE]>,
    header_buffer: Box<[u8; 1024]>,
}
```

### Measured Optimizations

Every performance claim requires benchmark proof:
- Piece selection: O(log n) for deadline-based
- Disk I/O: Batched writes, io_uring on Linux
- Transcoding: Worker pool with CPU affinity

## Testing Strategy

1. **Unit tests**: Algorithm correctness
2. **Integration tests**: Protocol compliance
3. **Property tests**: Invariant validation
4. **Mock environment**: Realistic network simulation

```rust
pub struct MockEnvironment {
    tracker: MockTracker,
    network: NetworkSimulator,  // Latency, packet loss, throttling
}
```

## Anti-Patterns Avoided

- No microservices (monolith until proven need)
- No premature abstractions (traits only with 2+ impls)
- No utils.rs modules (focused modules only)
- No blocking I/O in async contexts
- No unbounded buffers or queues

## Migration Strategy

```rust
pub struct StorageManifest {
    version: u32,
    layout: StorageLayout,
    migrations: Vec<Migration>,
}
```

Storage format versioning from day one prevents future migration pain.

## Production Considerations

- All async functions must be cancellation-safe
- Pre-allocate buffers in hot paths
- Feature flags for gradual rollout
- Comprehensive monitoring (download speed, buffer underruns, transcode queue)

## Philosophy

Build the simplest thing that streams movies reliably. Measure everything. Optimize based on real usage data, not assumptions.
