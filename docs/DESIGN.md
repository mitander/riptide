# Riptide - Design Summary

## Architecture Overview

A Rust-based torrent media server focused on streaming performance and rapid iteration, organized as a multi-crate workspace for modular development and deployment.

### Workspace Structure

```
riptide/
├── riptide-core/     → Core BitTorrent and streaming functionality (foundational)
├── riptide-web/      → Web UI and HTTP API server (depends on core)
├── riptide-search/   → Media search and metadata services (depends on core)
├── riptide-cli/      → Command-line interface (depends on core)
└── riptide-sim/      → Deterministic simulation framework (depends on core)
```

**Dependency Graph**: Clean tree structure with no circular dependencies

```
riptide-core (foundational)
    ↑
    ├── riptide-sim (simulation & testing)
    ├── riptide-web (UI & API)
    ├── riptide-cli (command interface)
    └── riptide-search (media discovery)
```

### Core Components

```
riptide-core:
  - TorrentEngine         → BitTorrent protocol with trait-based peer/tracker abstractions
  - TcpPeerCoordinator    → Production TCP peer connections and message handling
  - TrackerCoordinator    → Real HTTP/UDP tracker communication
  - Storage Layer         → File organization, reflink/CoW support, piece verification
  - Streaming Pipeline    → Stateless streaming with deep module boundaries
  - PieceProvider trait   → File-like interface over torrent storage
  - StreamProducer trait  → HTTP response generator for media content
  - DirectStreamProducer  → Pass-through for MP4/WebM with range support
  - RemuxStreamProducer   → Real-time FFmpeg remuxing with input pump
  - HttpStreaming facade  → Format detection and producer selection
  - TorrentCreator        → File-to-torrent conversion with piece splitting and hashing

riptide-web:
  - Web Server            → Axum-based HTTP server with template rendering
  - API Handlers          → RESTful API for torrent management and streaming
  - Template Engine       → Server-side rendering with external template files
  - Static Files          → Asset serving for CSS, JavaScript, images
  - PieceProviderAdapter  → Bridge between DataSource and streaming pipeline

riptide-search:
  - Media Search      → Torrent discovery via search providers
  - Metadata Service  → IMDb integration, poster/artwork fetching
  - Demo Provider     → Rich demo data for UI development

riptide-cli:
  - Command Interface → Add/manage torrents, start web server
  - Simulation Mode   → Development environment with configurable parameters

riptide-sim:
  - InMemoryPeerManager     → Simulated peer connections for deterministic testing
  - ContentAwarePeerManager → Piece-serving simulation with real file data
  - InMemoryPieceStore      → In-memory storage of actual torrent pieces
  - PieceReconstructionService → Reassembly of downloaded pieces for streaming
  - SimulatedTrackerCoordinator → Mock tracker responses for offline development
  - DeterministicSimulation → Controlled time and event scheduling for tests
```

## Key Design Decisions

### 1. Workspace Architecture

**Choice**: Multi-crate workspace with clear separation of concerns.

**Structure**:

- **riptide-core**: BitTorrent protocol and streaming, trait abstractions for testability
- **riptide-web**: HTTP API and UI, depends only on core
- **riptide-search**: Media search and metadata, depends only on core
- **riptide-cli**: Command interface, depends only on core
- **riptide-sim**: Simulation implementations of core traits, depends only on core

**Benefits**:

- **No Circular Dependencies**: Clean tree structure with single foundational crate
- **Modular Development**: Independent testing and parallel development
- **Deployment Flexibility**: Core embeddable without UI dependencies
- **Simulation Isolation**: Test implementations separated from production code

**Architecture Patterns**:

- Idiomatic Rust module organization (no domain/infrastructure split)
- Trait-based abstractions for testability and simulation
- Error conversion between crates via explicit mapping
- Workspace-level dependency management for consistency
- Two-phase streaming strategy for format compatibility
- Intelligent caching to avoid redundant remuxing operations

### 2. Unified Trait-Based BitTorrent Architecture

**Choice**: Trait abstractions enabling both real and simulated implementations with identical interfaces.

**Architecture**:

```rust
// Core traits enabling production and simulation implementations
pub trait TrackerManagement: Send + Sync {
    async fn announce_to_trackers(&mut self, request: AnnounceRequest) -> Result<AnnounceResponse>;
    async fn scrape_from_trackers(&mut self, request: ScrapeRequest) -> Result<ScrapeResponse>;
}

pub trait PeerManager: Send + Sync {
    async fn connect_peer(&mut self, address: SocketAddr, info_hash: InfoHash) -> Result<()>;
    async fn send_message(&mut self, peer_address: SocketAddr, message: PeerMessage) -> Result<()>;
    async fn receive_message(&mut self) -> Result<PeerMessageEvent>;
    async fn connected_peers(&self) -> Vec<PeerInfo>;
    async fn shutdown(&mut self) -> Result<()>;
}

// Production implementations (riptide-core)
pub struct TcpPeerCoordinator { /* TCP peer connections */ }
pub struct TrackerCoordinator { /* HTTP/UDP tracker communication */ }

// Simulation implementations (riptide-sim)
pub struct InMemoryPeerManager { /* Deterministic peer simulation */ }
pub struct SimulatedTrackerCoordinator { /* Offline tracker responses */ }
pub struct ContentAwarePeerManager<P: PieceStore> { /* Real piece data serving */ }
// Engine uses dependency injection for swappable implementations
pub struct TorrentEngine<P: PeerManager, T: TrackerManagement> {
    peer_manager: Arc<RwLock<P>>,
    tracker_manager: Arc<RwLock<T>>,
    // ... other fields
}
```

**Implementations**:

- **Production**: `TcpPeerCoordinator` + `TrackerCoordinator` for real BitTorrent operations
- **Simulation**: `InMemoryPeerManager` + `SimulatedTrackerCoordinator` for deterministic testing
- **Content-Aware**: `ContentAwarePeerManager<InMemoryPieceStore>` for end-to-end file simulation

**Benefits**:

- **Identical API**: Core engine logic works with real or simulated implementations
- **Deterministic Testing**: Complete reproducibility with simulation implementations
- **Comprehensive Coverage**: Test all code paths without network dependencies
- **Development Flexibility**: Mix real/simulated components for focused testing
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

### 3. Content Distribution Simulation

**Choice**: True content distribution simulation using real file data and piece reconstruction.

**Architecture**:

```rust
// File-to-torrent conversion with piece storage
pub struct SimulationTorrentCreator {
    creator: TorrentCreator,
    pieces: Vec<TorrentPiece>,  // Actual file data stored as pieces
}

// In-memory piece storage for simulation
pub struct InMemoryPieceStore {
    torrents: HashMap<InfoHash, HashMap<u32, TorrentPiece>>,
}

// Content-aware peer serving real piece data
pub struct ContentAwarePeerManager<P: PieceStore> {
    piece_store: Arc<P>,
    // Serves actual piece data in response to requests
}

// Piece reconstruction for streaming
pub struct PieceReconstructionService {
    verified_pieces: HashMap<InfoHash, BTreeMap<u32, VerifiedPiece>>,
    reconstructed_segments: HashMap<InfoHash, Vec<u8>>,
}
```

**Capabilities**:

- **Real File Conversion**: Split actual media files into torrent pieces with SHA-1 hashes
- **True Piece Serving**: Simulated peers serve actual file data, not mock responses
- **Content Reconstruction**: Downloaded pieces reassembled into streamable content
- **End-to-End Validation**: Complete pipeline from file → pieces → download → streaming

**Benefits**:

- **Realistic Testing**: Uses actual file data throughout the pipeline
- **Content Verification**: Ensures reconstructed content matches original files
- **Streaming Validation**: Test streaming with real media content
- **Performance Testing**: Real piece sizes and file formats

### 4. Storage Architecture

**Choice**: Simple directory structure with copy-on-write where available.

```
/media/library/{movie_id}/      # Completed movies
/media/downloads/{info_hash}/   # Active downloads
```

**Implementation**: Try reflink → hard link → move. No complex content-addressing.

### 5. Streaming Architecture

**Choice**: Stateless pipeline with deep module boundaries and real-time processing.

**Architecture**: `DataSource -> StreamProducer -> HTTP Response`

**Core Abstractions**:

```rust
// File-like interface over torrent storage
pub trait PieceProvider: Send + Sync {
    async fn read_at(&self, offset: u64, length: usize) -> Result<Bytes, PieceProviderError>;
    async fn size(&self) -> u64;
}

// HTTP response generator for media content
pub trait StreamProducer: Send + Sync {
    async fn produce_stream(&self, request: Request<()>) -> Response;
}
```

**Direct Streaming (MP4/WebM)**:

- `DirectStreamProducer` serves byte ranges directly from `PieceProvider`
- Full HTTP range request support for seeking
- Zero processing overhead

**Real-time Remuxing (MKV/AVI/MOV)**:

- `RemuxStreamProducer` uses FFmpeg input pump pattern
- Streams data to FFmpeg stdin as pieces become available
- Natural backpressure through pipe blocking
- Pre-buffers 64KB of MP4 output before sending HTTP response

**Benefits**:

- Complete decoupling between BitTorrent and streaming layers
- Testable design via `MockPieceProvider`
- Real-time processing without batch operations
- Duration preservation validated via ffprobe

### 6. Database Design

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

### Streaming Pipeline Architecture

```rust
// Stateless streaming facade
pub struct HttpStreaming {
    producer: Arc<dyn StreamProducer>,
}

// Format detection and producer selection
impl HttpStreaming {
    pub async fn new(provider: Arc<dyn PieceProvider>) -> StreamingResult<Self> {
        let format = detect_container_format(&header_data)?;
        let producer: Arc<dyn StreamProducer> = if requires_remuxing(&format) {
            Arc::new(RemuxStreamProducer::new(provider, extension(&format).to_string()))
        } else {
            Arc::new(DirectStreamProducer::new(provider, mime_type(&format).to_string()))
        };
        Ok(Self { producer })
    }
}
```

**Direct Streaming (MP4/WebM)**:

- `DirectStreamProducer` serves ranges directly from `PieceProvider`
- Full HTTP range request support, instant seeking

**Real-time Remuxing (MKV/AVI)**:

- `RemuxStreamProducer` uses FFmpeg input pump pattern
- Streams data as pieces become available
- No caching - compute on demand

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

### Phase 2: Streaming **COMPLETE**

- ✓ Stateless streaming pipeline with deep module boundaries
- ✓ PieceProvider and StreamProducer trait abstractions
- ✓ DirectStreamProducer for MP4/WebM with HTTP range support
- ✓ RemuxStreamProducer with FFmpeg input pump pattern
- ✓ HttpStreaming facade with automatic format detection
- ✓ PieceProviderAdapter bridging DataSource to streaming pipeline
- ✓ Real-time processing with natural backpressure
- ✓ Duration preservation validated via ffprobe testing

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
- Remuxing: Worker pool with CPU affinity

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
- Comprehensive monitoring (download speed, buffer underruns, remux queue)

## Philosophy

Build the simplest thing that streams movies reliably. Measure everything. Optimize based on real usage data, not assumptions.
