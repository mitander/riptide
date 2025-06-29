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

### 2. Unified Trait-Based BitTorrent Architecture

**Choice**: Trait abstractions enabling both real and simulated implementations with identical interfaces.

**Architecture**:

```rust
// Core traits for unified interface
pub trait TrackerManager: Send + Sync {
    async fn announce_to_trackers(&mut self, tracker_urls: &[String], request: AnnounceRequest) -> Result<AnnounceResponse>;
    async fn scrape_from_trackers(&mut self, tracker_urls: &[String], request: ScrapeRequest) -> Result<ScrapeResponse>;
}

pub trait PeerManager: Send + Sync {
    async fn connect_peer(&mut self, address: SocketAddr, info_hash: InfoHash, peer_id: PeerId) -> Result<()>;
    async fn send_message(&mut self, peer_address: SocketAddr, message: PeerMessage) -> Result<()>;
    async fn receive_message(&mut self) -> Result<PeerMessageEvent>;
    // ... additional methods
}

// Dependency injection for swappable implementations
pub struct TorrentEngine<P: PeerManager, T: TrackerManager> {
    peer_manager: Arc<RwLock<P>>,
    tracker_manager: Arc<RwLock<T>>,
    // ... other fields
}
```

**Implementations**:

- **Production**: `NetworkPeerManager` + `TrackerManager` for real BitTorrent operations with multi-tracker support.
- **Testing**: `SimulatedPeerManager` + `SimulatedTrackerManager` for deterministic testing.
- **Development**: Mix real/simulated components for focused testing.

**Benefits**:

- **99% Generic Logic**: Core engine logic works identically with real or simulated implementations.
- **Multi-Tracker Support**: The `TrackerManager` can handle torrents with multiple trackers, improving peer discovery.
- **Tracker Failover**: If one tracker is unavailable, the manager will automatically try the next one in the list.
- **Comprehensive Fuzzing**: Test all code paths with deterministic simulated components.
- **Gradual Integration**: Develop with simulation, deploy with real implementations.
- **Protocol Compliance**: Real implementation ensures BitTorrent specification adherence.

**Components**:

- **HTTP Tracker Client**: Real tracker communication with bencode parsing and BEP 23 support
- **TCP Peer Manager**: BitTorrent wire protocol with handshake and message serialization
- **Peer Protocol**: Complete message types (bitfield, request, piece, have, etc.)
- **Connection Management**: Async TCP with connection pooling and message routing

**Testing Strategy**:

The unified interface enables comprehensive testing:
- Unit tests with simulated components for speed and determinism
- Integration tests with real components for protocol validation
- Mixed scenarios testing specific edge cases
- Fuzzing with controlled inputs via simulation

### 3. Storage Architecture

**Choice**: Simple directory structure with copy-on-write where available.

```
/media/library/{movie_id}/      # Completed movies
/media/downloads/{info_hash}/   # Active downloads
```

**Implementation**: Try reflink → hard link → move. No complex content-addressing.

### 4. Streaming Strategy

**Choice**: Direct streaming by default, pre-transcode during download for incompatible formats.

**Rationale**: Eliminates buffering/stuttering. Most devices support H.264/H.265 directly.

### 5. Database Design

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

### Phase 1: Core **COMPLETE**

- Real BitTorrent tracker communication with HTTP announce/scrape
- TCP peer connections with BitTorrent wire protocol  
- Unified trait-based architecture for production and testing
- File storage with piece management
- CLI interface for torrent management
- Comprehensive test suite

### Phase 2: Streaming (In Progress)

- Basic streaming service architecture
- HTTP range request handling
- Stream coordinator with piece prioritization
- **Current**: Template/asset extraction and organization
- Direct streaming optimization
- Bandwidth management and buffering

### Phase 3: Web UI Enhancement

- Browse library with rich metadata
- Real-time download progress
- Search integration with providers
- Device detection and compatibility

### Phase 4: Production Polish

- Subtitles and multi-language support
- Apple TV app development
- Performance optimization and monitoring
- VPN integration and security features

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