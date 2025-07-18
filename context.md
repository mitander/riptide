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
# Riptide Style Guide

This document defines the coding standards for the Riptide project. All code must conform to these rules, which are enforced by built-in Rust tooling: `rustfmt`, `clippy`, and style enforcement tests.

## 1. Guiding Principles

1.  **Correctness > Performance > Features.** A clever optimization is worthless if it introduces subtle bugs.
2.  **Clarity at the Point of Use.** Code must be immediately understandable without tracing its implementation. No magic.
3.  **Deep Modules, Simple Interfaces.** Complexity is to be hidden within a module, not exposed through its public API. (See: "A Philosophy of Software Design").
4.  **Static over Dynamic.** Prefer static dispatch. Use `dyn Trait` only at major architectural boundaries for decoupling (e.g., swapping production vs. simulation components).
5.  **Measure, Don't Guess.** Performance optimizations must be backed by benchmarks.

## 2. Naming Conventions

### 2.1. Types (Structs, Enums, Traits)

Name what the type *is*, not its architectural pattern or role. Role-based suffixes like `Service`, `Manager`, or `Factory` are banned because they are verbose and add no semantic value.

| Correct           | Banned                       | Rationale                                   |
| :---------------- | :--------------------------- | :------------------------------------------ |
| `HttpStreaming`   | `HttpStreamingService`       | Redundant. The module context is sufficient.  |
| `FileLibrary`     | `FileLibraryManager`         | It *is* a library, not a manager *for* one.   |
| `new()`           | `PeerFactory::create()`      | `new()` is idiomatic. The Factory pattern is banned. |
| `TorrentEngine`   | `TorrentEngineController`    | Use direct, concrete names.                   |
| `Remuxer`         | `RemuxProcessor` or `RemuxHandler` | The name should describe the entity's essence. |

Standard suffixes for common patterns are required:

-   **Errors:** `*Error` (e.g., `TorrentError`)
-   **Configuration:** `*Config` (e.g., `RiptideConfig`)
-   **Handles:** `*Handle` (e.g., `TorrentEngineHandle`)
-   **Builders:** `*Builder` (e.g., `ConfigBuilder`)

### 2.2. Functions & Methods

Function names must be verbs that clearly state their action and effect.

| Convention                 | Example                           | Description                                     |
| :------------------------- | :-------------------------------- | :---------------------------------------------- |
| **Actions**                | `connect_peer()`, `parse_header()`  | `verb_noun` format. Clear and direct.           |
| **State Checkers**         | `is_complete()`, `has_piece()`    | Prefixed with `is_` or `has_`. Must return `bool`.|
| **Fallible Operations**    | `try_create_session()`            | Prefix with `try_`. Must return a `Result`.       |
| **Optional Getters**       | `output_path()`                   | Noun only. Must return `Option`.                  |
| **Simple Getters**         | `peer_id()`                       | Noun only. For fields that are always present.  |
| **Conversions (Borrow)**   | `as_bytes()`                      | Prefix with `as_`. Returns a reference.         |
| **Conversions (Consuming)**| `into_inner()`                    | Prefix with `into_`. Consumes `self`.           |

**Forbidden Prefixes:**

-   **`get_`**: REJECT. Use the noun directly. (`peer.id()` not `peer.get_id()`).
-   **`set_`**: REJECT. Use a descriptive verb. (`peer.update_speed(new_speed)` not `peer.set_speed(speed)`).
-   **`handle_`**: REJECT. Be specific. (`process_request()` not `handle_request()`).

### 2.3. Modules

Module names should describe their contents. Generic, meaningless names are banned.

| Correct          | Banned                      | Rationale                                   |
| :--------------- | :-------------------------- | :------------------------------------------ |
| `piece_picker`   | `utils`, `helpers`          | Describes the specific responsibility.        |
| `protocol`, `types` | `common`, `misc`, `stuff` | Use clear, domain-specific names.           |

## 3. Commit Message Convention

All commits **must** adhere to the [Conventional Commits](https://www.conventionalcommits.org/) specification. This is enforced by a git hook and enables automated changelogs and a clear, parsable project history.

**Format:** `<type>(scope)!: <description>`

-   **Types:** `feat`, `fix`, `docs`, `style`, `refactor`, `test`, `chore`, `build`, `ci`, `perf`, `revert`.
-   **Description:** Must start with a letter or digit and be 1-100 characters long.
-   **Breaking Changes:** Use `!` for breaking changes: `refactor(core)!: rename PeerManager trait`.

---

**Example: Single-line Commit**
```
docs: fix missing documentation warnings
```
---

**Example: Multi-line Commit with Body**
```
refactor(sim): unify peer manager implementations

- Consolidate DevPeerManager and SimPeerManager into organized peer_manager module.
- Create peer_manager/deterministic.rs for controlled simulation.
- Create peer_manager/development.rs for rapid iteration.
- Move piece store implementation to peer_manager/piece_store.rs.
- Update simulation_mode.rs to use the new unified peer manager structure.
- Remove redundant dev_peer_manager.rs and sim_peer_manager.rs files.

This change creates a cleaner, more maintainable peer management architecture with better separation between testing modes.
```

## 4. Documentation

All `pub` items **must** have complete documentation. This is enforced by `cargo clippy`. Non-compliance is a build failure.

-   **Summary:** Start with a concise one-line summary.
-   **Detail:** Follow with a paragraph explaining purpose, behavior, and any non-obvious details.
-   **`# Errors`**: Required for any function returning a `Result`. **Always** use a bulleted list.
-   **`# Panics`**: Required for any function that can panic. Use a sentence for a single condition; use a list for multiple.

**Example:**
```rust
/// Handles an HTTP streaming request.
///
/// This is the primary entry point for media streaming. It parses the `Range`
/// header, determines the required data from the underlying `DataSource`,
/// and constructs an appropriate `206 Partial Content` or `200 OK` response.
///
/// # Errors
///
/// - `StreamingError::DataSource` - If reading from the `DataSource` fails.
/// - `StreamingError::InvalidRange` - If the requested byte range is invalid.
///
/// # Panics
///
/// Panics if the `Content-Length` header cannot be constructed from the
/// response size, which should never happen for valid data.
pub async fn process_http_request(
    // ...
) -> Result<Response, StreamingError> {
    // ...
}
```

## 5. Error Handling

-   **Libraries (`riptide-core`, `riptide-sim`):** Use `thiserror` to create specific, well-defined error enums.
-   **Applications (`riptide-cli`):** Use `anyhow` for simple, context-rich error handling.
-   **Zero Tolerance for `.unwrap()` or `.expect()`:** These are forbidden outside of tests and will fail CI. The only exception is a "checked" `unwrap()` with a `// SAFETY:` comment explaining why it cannot fail.

## 6. Testing

-   **Deterministic Simulation:** Inspired by TigerBeetle, we use deterministic simulation for robust and reproducible testing of complex concurrency scenarios.
-   **Coverage:** All new features and bug fixes must be accompanied by tests. This is a non-negotiable part of a "Done" feature.
-   **Test Types:**
    -   **Unit Tests:** Located in the same file as the code they test.
    -   **Integration Tests:** In the `riptide-tests/integration` directory. Verify interactions between modules.
    -   **E2E Tests:** In the `riptide-tests/e2e` directory. Verify complete user workflows.
