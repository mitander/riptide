# Riptide Style Guide

Write code that streams movies reliably, not code that impresses Rust evangelists.

## Core Philosophy

1. **Working > Clever** - Ship features users can use
2. **Measured > Assumed** - Prove performance improvements with benchmarks
3. **Simple > Flexible** - YAGNI until you need it
4. **Modular > Monolithic** - Clear crate boundaries enable focused development

## Workspace Architecture

### Crate Organization

Riptide uses a **multi-crate workspace** with clear separation of concerns:

```
riptide/
├── riptide-core/     → Core BitTorrent and streaming (no web dependencies)
├── riptide-web/      → Web UI and HTTP API (depends on core + search)
├── riptide-search/   → Media search and metadata (standalone)
└── riptide-cli/      → Command interface (orchestrates other crates)
```

**Design Rules:**
- **riptide-core** has zero web dependencies (pure protocol + streaming)
- **riptide-web** depends on core and search, handles all HTTP concerns
- **riptide-search** is standalone, can be used independently
- **riptide-cli** orchestrates other crates, provides unified interface

### Module Organization Within Crates

Deep modules for complex domains, shallow for simple ones:

```
riptide-core/src/
├── torrent/                 # Deep: Complex BitTorrent protocol
│   ├── mod.rs              # Public API only
│   ├── engine.rs           # Core orchestration
│   ├── enhanced_peer_manager/  # Peer management subsystem
│   │   ├── mod.rs
│   │   ├── connection.rs   # Wire protocol handling
│   │   ├── bandwidth.rs    # Rate limiting
│   │   └── metrics.rs      # Performance tracking
│   ├── piece_picker.rs     # Selection algorithms
│   ├── peer_connection.rs  # Individual peer handling
│   └── tracker.rs          # Tracker communication
├── streaming/              # Medium: HTTP streaming logic
│   ├── mod.rs
│   ├── http_server.rs      # Range request handling
│   ├── range_handler.rs    # Byte range logic
│   └── stream_coordinator.rs # Session management
├── storage/                # Medium: File management
│   ├── mod.rs
│   ├── file_storage.rs     # Copy-on-write operations
│   └── test_fixtures.rs    # Development utilities
└── config.rs               # Shallow: Configuration only

riptide-web/src/
├── handlers.rs             # Request handlers and API logic
├── server.rs               # Axum server setup and routing
├── templates.rs            # Server-side rendering engine
├── static_files.rs         # CSS, JS, image serving
└── lib.rs                  # Public API and error types

riptide-search/src/
├── service.rs              # Search coordination and demo data
└── lib.rs                  # Public API and error types

riptide-cli/src/
├── commands.rs             # CLI command implementations
└── main.rs                 # Command parsing and execution
```

**Size Limits:**
- **Files**: 500 lines max (split into subdirectory if larger)
- **Functions**: 50 lines max (exception: state machines)
- **Crates**: Related functionality only, clear boundaries

### Cross-Crate Dependencies

```rust
// riptide-core: Core functionality, no dependencies on other riptide crates
pub use config::RiptideConfig;
pub use torrent::TorrentEngine;
pub use streaming::DirectStreamingService;

// riptide-search: Standalone search functionality
pub use service::MediaSearchService;

// riptide-web: Depends on core and search
use riptide_core::{TorrentEngine, DirectStreamingService};
use riptide_search::MediaSearchService;

// riptide-cli: Orchestrates all other crates
use riptide_core::{TorrentEngine, RiptideConfig};
use riptide_web::{WebServer, WebHandlers};
use riptide_search::MediaSearchService;
```

## Error Handling Strategy

### Cross-Crate Error Conversion

Each crate defines its own error types, with explicit conversion between crates:

```rust
// riptide-core/src/lib.rs
#[derive(Debug, thiserror::Error)]
pub enum RiptideError {
    #[error("Torrent error: {0}")]
    Torrent(#[from] TorrentError),
    
    #[error("Storage error: {0}")]
    Storage(#[from] StorageError),
    
    #[error("Web UI error: {reason}")]
    WebUI { reason: String },
}

impl RiptideError {
    // Manual conversion for cross-crate errors
    pub fn from_web_ui_error(error: impl std::fmt::Display) -> Self {
        RiptideError::WebUI { reason: error.to_string() }
    }
}

// riptide-web/src/lib.rs  
#[derive(Debug, thiserror::Error)]
pub enum WebUIError {
    #[error("Template error: {reason}")]
    TemplateError { reason: String },
    
    #[error("Server failed to start on {address}: {reason}")]
    ServerStartFailed { address: std::net::SocketAddr, reason: String },
}

// Implement IntoResponse for Axum compatibility
impl IntoResponse for WebUIError {
    fn into_response(self) -> Response {
        let (status, message) = match self {
            WebUIError::TemplateError { reason } => (StatusCode::INTERNAL_SERVER_ERROR, reason),
            WebUIError::ServerStartFailed { reason, .. } => (StatusCode::INTERNAL_SERVER_ERROR, reason),
        };
        (status, format!("Error: {}", message)).into_response()
    }
}

// riptide-cli usage
web_server.start().await.map_err(RiptideError::from_web_ui_error)?;
```

### Error Documentation

All public functions returning `Result` **must** document their errors:

```rust
/// Starts downloading the specified torrent.
///
/// Connects to trackers, discovers peers, and begins piece acquisition using
/// the configured piece selection strategy.
///
/// # Errors
/// - `TorrentError::InvalidTorrentFile` - Failed to parse torrent data
/// - `TorrentError::TrackerConnectionFailed` - Could not reach tracker
/// - `TorrentError::InsufficientDiskSpace` - Not enough storage available
pub async fn start_download(&mut self, magnet_link: &str) -> Result<DownloadHandle, TorrentError>
```

## Testing Strategy

### Test Organization by Crate

```rust
// riptide-core/src/torrent/engine.rs
#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::RiptideConfig;
    
    #[tokio::test]
    async fn test_add_magnet_link() {
        let config = RiptideConfig::default();
        let mut engine = TorrentEngine::new(config);
        // Test core functionality
    }
}

// riptide-web/src/handlers.rs
#[cfg(test)]
mod tests {
    use super::*;
    use riptide_core::config::RiptideConfig;
    use riptide_core::torrent::TorrentEngine;
    
    #[tokio::test]
    async fn test_web_handlers_creation() {
        let config = RiptideConfig::default();
        let engine = Arc::new(RwLock::new(TorrentEngine::new(config)));
        // Test web layer
    }
}
```

### Integration Testing

```rust
// tests/integration_tests.rs (workspace root)
use riptide_core::{TorrentEngine, RiptideConfig};
use riptide_web::{WebServer, WebHandlers};
use riptide_search::MediaSearchService;

#[tokio::test]
async fn test_full_workflow() {
    // Test complete search -> download -> stream workflow
    let config = RiptideConfig::default();
    let engine = TorrentEngine::new(config.clone());
    let search = MediaSearchService::new_demo();
    
    // Integration test across all crates
}
```

### Mock Strategy

Mock at **crate boundaries**, not internal APIs:

```rust
// riptide-search/src/service.rs
#[async_trait]
pub trait TorrentSearchProvider: Send + Sync + std::fmt::Debug {
    async fn search_torrents(&self, query: &str, category: &str) 
        -> Result<Vec<MediaSearchResult>, MediaSearchError>;
}

// Mock for testing web layer
#[cfg(test)]
pub struct MockSearchProvider {
    results: Vec<MediaSearchResult>,
}

#[cfg(test)]
impl MockSearchProvider {
    pub fn with_results(results: Vec<MediaSearchResult>) -> Self {
        Self { results }
    }
}
```

## Performance Standards

### Measurement Requirements

Every optimization needs proof with specific benchmarks:

```rust
// BENCHMARK: torrent_piece_selection
// Before: 847ns per piece selection, 156 allocations
// After:  623ns per piece selection, 12 allocations  
// Improvement: 26.4% faster, 92% fewer allocations
// Justification: Hot path called per-piece during streaming
```

### Zero-Allocation Streaming Paths

```rust
// riptide-core/src/streaming/
pub struct DirectStreamingService {
    // Pre-allocated at startup, reused for all streams
    segment_buffer: Box<[u8; SEGMENT_SIZE]>,
    header_buffer: Box<[u8; 1024]>,
    piece_cache: HashMap<PieceIndex, Vec<u8>>,
}

impl DirectStreamingService {
    // Write into provided buffer, never allocate
    pub async fn read_segment(&mut self, output: &mut [u8]) -> Result<usize> {
        // Reuse pre-allocated buffers
        self.segment_buffer.clear();
        // ... streaming logic
    }
}
```

### Performance Targets

- **Piece selection**: <1000ns per operation
- **HTTP response**: <100ms from request to first byte
- **Memory usage**: <50MB baseline, <1MB per active stream
- **Startup time**: <500ms for web server initialization

## Configuration Architecture

### Runtime vs Compile-time Configuration

**Use runtime configuration** for all behavior changes:

```rust
// riptide-core/src/config.rs
#[derive(Debug, Clone)]
pub struct RiptideConfig {
    pub network: NetworkConfig,
    pub storage: StorageConfig, 
    pub simulation: SimulationConfig,  // Runtime, not #[cfg(simulation)]
}

#[derive(Debug, Clone)]
pub struct SimulationConfig {
    pub enabled: bool,                    // Runtime flag
    pub deterministic_seed: Option<u64>,  // Deterministic testing
    pub max_simulated_peers: usize,       // Configurable simulation
    pub use_mock_data: bool,              // Development vs production
}

// Environment variable support
impl RiptideConfig {
    pub fn from_env() -> Self {
        let simulation_enabled = std::env::var("RIPTIDE_SIMULATION")
            .unwrap_or_default() == "true";
        // ... load from environment
    }
}
```

**Never use compile-time features** for behavior changes:

```rust
// WRONG: Compile-time simulation
#[cfg(feature = "simulation")]
fn create_peer_manager() -> PeerManager {
    SimulatedPeerManager::new()
}

// RIGHT: Runtime configuration  
fn create_peer_manager(config: &RiptideConfig) -> Box<dyn PeerManager> {
    if config.simulation.enabled {
        Box::new(SimulatedPeerManager::new(config.simulation.clone()))
    } else {
        Box::new(ProductionPeerManager::new())
    }
}
```

## Documentation Standards

### Public API Documentation

**Required for all public functions**:

```rust
/// Compresses RTP/UDP/IP headers into ROHC packet.
///
/// Analyzes headers and context to determine optimal packet type (IR, UO-0, etc.)
/// and generates the corresponding ROHC packet. Updates compressor context state.
/// Returns the number of bytes written to the output buffer.
///
/// # Errors
/// - `RohcError::ContextNotFound` - No context for the given CID
/// - `RohcError::UnsupportedProfile` - Headers incompatible with Profile 1
/// - `RohcError::BufferTooSmall` - Output buffer insufficient
///
/// # Examples
/// ```rust
/// let mut buffer = [0u8; 1024];
/// let compressed_size = compressor.compress(&headers, &mut buffer)?;
/// ```
pub fn compress(&mut self, headers: &Headers, buffer: &mut [u8]) -> Result<usize, RohcError>
```

### Internal Documentation

Brief docs for complex algorithms only:

```rust
/// RFC 3095 4.5.1: Calculate minimum k-bits for W-LSB encoding
fn calculate_k_bits(v_ref: u16, v: u16) -> u8

// Simple getter - no comment needed
fn get_sequence_number(&self) -> u16
```

### Cross-Crate Documentation

Document **why** crates are separated, not just **what** they do:

```rust
//! Riptide Core - Essential BitTorrent and streaming functionality
//!
//! This crate provides the fundamental building blocks for BitTorrent-based
//! media streaming: torrent protocol implementation, file storage, streaming
//! services, and configuration management.
//!
//! **Design Philosophy**: Core functionality with zero web dependencies.
//! This enables embedding in CLI tools, desktop applications, or alternative
//! web frameworks without pulling in HTTP server dependencies.

//! Riptide Web - Web UI and API server
//!
//! Provides HTTP-based interface for managing torrents and streaming media.
//! Built on Axum with server-side rendering and RESTful API endpoints.
//!
//! **Design Philosophy**: All web concerns isolated here. Template rendering,
//! static file serving, HTTP routing, and WebSocket connections. Depends on
//! riptide-core for business logic and riptide-search for media discovery.
```

## Commit Message Standards

### Workspace-Aware Commit Format

```
type(scope): concise description

- Key change explanation if multi-component
- Brief WHY if not obvious from changes

BENCHMARK: benchmark_name (only if performance changed)
Before: X
After: Y
```

### Scope Examples

```bash
feat(core): add deadline-based piece selection
fix(web): handle template rendering errors gracefully  
perf(search): cache torrent quality calculations
refactor(cli): extract command parsing to separate module
docs(workspace): update architecture documentation
test(integration): add full search-to-stream workflow test
```

### Cross-Crate Changes

```bash
feat(workspace): implement unified streaming interface

Add StreamSource enum in riptide-core and update web handlers
to support both torrent and local file streaming.

CHANGES:
- riptide-core: Add StreamSource enum and UnifiedStreamer
- riptide-web: Update handlers to use unified streaming API
- riptide-cli: Add local file streaming command support

BENCHMARK: stream_initialization
Before: 245ms (torrent-only)
After: 198ms (unified interface)
```

## Essential Commands

### Workspace Development

```bash
# Build entire workspace
cargo build --workspace

# Test all crates with output
cargo test --workspace -- --nocapture

# Check specific crate
cargo check -p riptide-core
cargo test -p riptide-web

# Run CLI from workspace
cargo run -p riptide-cli -- server --demo

# Format and lint
cargo fmt --all
cargo clippy --workspace -- -D warnings
```

### Performance Measurement

```bash
# Run benchmarks for specific crate
cargo bench -p riptide-core

# Profile streaming performance
cargo run --release -p riptide-cli -- server &
# Use profiling tools against running server
```

## Code Quality Standards

### Import Organization

```rust
// Standard library
use std::collections::HashMap;
use std::sync::Arc;

// External crates (alphabetical)
use axum::response::IntoResponse;
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;

// Internal workspace crates (dependency order)
use riptide_core::config::RiptideConfig;
use riptide_core::torrent::TorrentEngine;
use riptide_search::MediaSearchService;

// Local modules (relative imports)
use super::WebUIError;
use crate::templates::TemplateEngine;
```

### Error Handling

```rust
// Within same crate: Use #[from] for automatic conversion
#[derive(Debug, thiserror::Error)]
pub enum TorrentError {
    #[error("Storage error")]
    Storage(#[from] StorageError),
    
    #[error("Network error")]  
    Network(#[from] reqwest::Error),
}

// Cross-crate: Use explicit conversion
web_server.start().await.map_err(RiptideError::from_web_ui_error)?;
```

### Memory Management

```rust
// Pre-allocate in constructors
pub struct TorrentEngine {
    piece_buffer: Vec<u8>,           // Pre-allocated to max piece size
    peer_connections: Vec<PeerConnection>, // Pre-sized connection pool
}

impl TorrentEngine {
    pub fn new(config: RiptideConfig) -> Self {
        Self {
            piece_buffer: Vec::with_capacity(config.max_piece_size),
            peer_connections: Vec::with_capacity(config.max_peers),
        }
    }
    
    // Reuse allocated memory
    pub fn process_piece(&mut self, data: &[u8]) -> Result<()> {
        self.piece_buffer.clear();  // Reuse, don't reallocate
        self.piece_buffer.extend_from_slice(data);
        // Process...
    }
}
```

This style guide reflects the **current workspace architecture** and provides **concrete patterns** for maintaining code quality across all crates while enabling independent development and deployment.