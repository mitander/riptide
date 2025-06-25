# Riptide - Design Summary

## Architecture Overview

A Rust-based torrent media server focused on streaming performance and rapid iteration, organized as a multi-crate workspace for modular development and deployment.

### Workspace Structure

```
riptide/
├── riptide-core/     → Core BitTorrent and streaming functionality
├── riptide-web/      → Web UI and HTTP API server
├── riptide-search/   → Media search and metadata services
├── riptide-cli/      → Command-line interface
└── riptide-sim/      → Deterministic simulation framework
```

### Core Components

```
riptide-core:
  - Torrent Engine    → BitTorrent protocol, piece management, peer connections
  - Storage Layer     → File organization, reflink/CoW support
  - Streaming Service → Direct streaming, piece buffering, bandwidth control
  - Configuration     → Runtime simulation config, network settings

riptide-web:
  - Web Server        → Axum-based HTTP server with template rendering
  - API Handlers      → RESTful API for torrent management and streaming
  - Template Engine   → Server-side rendering with external template files
  - Static Files      → Asset serving for CSS, JavaScript, images

riptide-search:
  - Media Search      → Torrent discovery via search providers
  - Metadata Service  → IMDb integration, poster/artwork fetching
  - Demo Provider     → Rich demo data for UI development

riptide-cli:
  - Command Interface → Add/manage torrents, start web server
  - Simulation Mode   → Development environment with configurable parameters

riptide-sim:
  - Deterministic Clock    → Controlled time advancement for reproducible tests
  - Event Scheduler        → Priority-based event queue with deterministic ordering
  - Network Simulation     → Configurable latency, packet loss, bandwidth limits
  - Peer Simulation        → Mock peers with deterministic behavior patterns
  - Media Scenarios        → Real-world streaming patterns and edge cases
  - Invariant Checking     → Runtime validation of system properties
```

## Key Design Decisions

### 1. Workspace Architecture

**Choice**: Multi-crate workspace with clear separation of concerns.

**Structure**:

- **riptide-core**: Essential BitTorrent and streaming functionality, no web dependencies
- **riptide-web**: Web UI and HTTP API, depends on core and search crates
- **riptide-search**: Media search and metadata, standalone with optional integration
- **riptide-cli**: Command-line interface, orchestrates other crates
- **riptide-sim**: Deterministic simulation framework for testing and development

**Benefits**:

- **Modular Development**: Independent versioning and testing of components
- **Deployment Flexibility**: Core can be embedded without web UI overhead
- **Clear Boundaries**: Web concerns separated from protocol implementation
- **Runtime Configuration**: Simulation mode configurable at runtime vs compile-time

**Architecture Patterns**:

- Idiomatic Rust module organization (no domain/infrastructure split)
- Trait-based abstractions for testability and simulation
- Error conversion between crates via explicit mapping
- Workspace-level dependency management for consistency

### 2. Torrent Implementation

**Choice**: Trait abstractions with controlled external dependencies for non-streaming components.

**Components**:

- **Bencode Parsing**: Own `bencode-rs` crate for full control and streaming optimization
- **Magnet Links**: `magnet-url` crate (zero dependencies, ultra-fast)
- **Tracker Client**: Custom implementation using `reqwest` for HTTP, `tokio` for UDP
- **Peer Protocol**: Custom implementation optimized for streaming piece selection
- **Piece Selection**: Custom streaming-optimized algorithms with deadline-based prioritization

**Rationale**: Trait abstractions enable swappable implementations while maintaining full control over streaming-critical components. External dependencies used only for non-streaming functionality.

**Future Considerations**:

- `bencode-rs` may be enhanced and published to crates.io as a standalone library once streaming optimizations are proven. Current git dependency provides maximum development flexibility.
- Upgrade to nightly Rust to enable automatic import grouping in rustfmt for better code organization.

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

### Phase 4: Polish (Weeks 7-16)

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
4. **Deterministic simulation**: Reproducible scenarios with controlled time

```rust
// Using riptide-sim for deterministic testing
pub struct SimulationEnvironment {
    simulation: DeterministicSimulation,
    clock: DeterministicClock,
    network: NetworkSimulator,
}

// Pre-built scenarios for common test cases
SimulationScenarios::ideal_streaming(seed);
SimulationScenarios::peer_churn(seed);
SimulationScenarios::piece_failures(seed);
StreamingEdgeCases::bandwidth_collapse_scenario(seed);
```

The simulation framework enables:

- **Reproducible tests**: Same seed produces identical results
- **Time control**: Advance time deterministically without delays
- **Event scheduling**: Precise control over event ordering
- **Resource limits**: Test behavior under constrained resources
- **Invariant checking**: Validate system properties throughout simulation

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

## Implementation Considerations

- All async functions must be cancellation-safe
- Pre-allocate buffers in hot paths
- Feature flags for gradual rollout
- Comprehensive monitoring (download speed, buffer underruns, transcode queue)

## Philosophy

Build the simplest thing that streams movies reliably. Measure everything. Optimize based on real usage data, not assumptions.
