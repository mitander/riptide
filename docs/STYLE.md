# Riptide Style Guide

Write code that streams movies, not code that impresses Rust evangelists.

## Core Philosophy

1. **Working > Clever** - Ship features users can use
2. **Measured > Assumed** - Prove performance improvements
3. **Simple > Flexible** - YAGNI until you need it

## Architecture

### Module Organization

Deep modules for complex domains, shallow for simple ones:

```
src/
├── torrent/                 # Deep: Complex protocol
│   ├── mod.rs              # Public API only
│   ├── engine.rs           # Core orchestration
│   ├── peer/               # Peer-related subsystem
│   │   ├── mod.rs
│   │   ├── connection.rs   # Wire protocol
│   │   ├── messages.rs     # Message types
│   │   └── handshake.rs    # Connection setup
│   ├── piece/              # Piece management subsystem
│   │   ├── mod.rs
│   │   ├── picker.rs       # Selection algorithms
│   │   ├── storage.rs      # Disk I/O
│   │   └── verification.rs # Hash checking
│   └── tracker/            # Tracker subsystem
│       ├── mod.rs
│       ├── http.rs         # HTTP tracker
│       └── udp.rs          # UDP tracker
├── streaming/              # Shallow: Simple HTTP
│   ├── mod.rs
│   ├── direct.rs           # Range requests
│   └── hls.rs              # HLS generation
└── config.rs               # Shallow: Just config
```

**Rule**: If a module exceeds 500 lines, it needs a subdirectory.

### Abstraction Boundaries

Only abstract when you have 2+ implementations:

```rust
// WRONG: Premature abstraction
trait Storage {
    async fn store(&mut self, data: &[u8]) -> Result<()>;
}
struct FileStorage;
impl Storage for FileStorage { ... }

// RIGHT: Concrete first
struct PieceStorage {
    base_path: PathBuf,
}
impl PieceStorage {
    pub async fn store(&mut self, piece: &Piece) -> Result<()> { ... }
}

// Later, when adding S3 storage, THEN make the trait
```

## Type Safety

### When to Newtype

Use newtypes to prevent catastrophic mix-ups, not for every integer:

```rust
// NECESSARY: Easy to swap parameters
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct InfoHash([u8; 20]);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct PieceIndex(u32);

// OVERKILL: Context makes it obvious
pub struct FileSize(u64);  // Just use u64
pub struct PortNumber(u16); // Just use u16
```

### Builder Pattern

Only for 4+ optional parameters:

```rust
// OVERKILL: Just use a function
TorrentBuilder::new()
    .info_hash(hash)
    .build()

// APPROPRIATE: Many optional configs
TorrentEngineBuilder::new()
    .download_dir("/media")
    .max_connections(50)
    .encryption_required(true)
    .dht_enabled(false)
    .piece_picker(Sequential)
    .build()
```

## Error Handling

### Error Strategy

```rust
// Application errors: thiserror with context
#[derive(Debug, thiserror::Error)]
pub enum TorrentError {
    #[error("Tracker {url} unreachable: {source}")]
    TrackerUnreachable {
        url: String,
        #[source]
        source: reqwest::Error,
    },

    #[error("Piece {index} verification failed")]
    PieceCorrupt { index: PieceIndex },
}

// Library boundaries: Specific enums
pub enum StreamingError {
    NotReady,
    InvalidRange { requested: Range<u64>, available: u64 },
}

// Internal helpers: Simple strings
fn validate_piece_size(size: u32) -> Result<(), &'static str> {
    if size.is_power_of_two() && size >= 16384 {
        Ok(())
    } else {
        Err("Piece size must be power of 2, minimum 16 KiB")
    }
}
```

### Assertions vs Errors

```rust
// Network input: Always Result
if packet.len() < 68 {
    return Err(TorrentError::PacketTooSmall);
}

// Internal invariants: debug_assert
debug_assert!(!self.pieces.is_empty(), "Torrent has no pieces");

// Safety-critical: document why assert is needed
// SAFETY: piece_index bounds-checked above, panic prevents memory corruption
assert!(piece_index < self.pieces.len());
```

## Memory Management

### Zero-Allocation Streaming

The streaming path must not allocate:

```rust
pub struct StreamingService {
    // Pre-allocated at startup
    segment_buffer: Box<[u8; SEGMENT_SIZE]>,
    header_buffer: Box<[u8; 1024]>,
}

// GOOD: Writes into provided buffer
pub async fn read_segment(&mut self, output: &mut [u8]) -> Result<usize>

// BAD: Allocates on every call
pub async fn read_segment(&mut self) -> Result<Vec<u8>>
```

### Buffer Reuse Pattern

```rust
pub struct TorrentEngine {
    // Reused across all operations
    piece_buffer: BytesMut,
    message_buffer: BytesMut,

    // Object pools for concurrent operations
    verification_pool: Pool<sha1::Sha1>,
}

impl TorrentEngine {
    async fn download_piece(&mut self, index: PieceIndex) -> Result<()> {
        self.piece_buffer.clear();
        self.piece_buffer.reserve(self.piece_size);
        // Use buffer...
    }
}
```

## Async Patterns

### Async vs Sync

**Async for I/O, sync for CPU:**

```rust
// GOOD: Network I/O: async
async fn connect_to_peer(addr: SocketAddr) -> Result<PeerConnection>

// GOOD: Disk I/O: async
async fn load_torrent_file(path: &Path) -> Result<Torrent>

// BAD: CPU-bound: Should be sync + spawn_blocking
async fn calculate_piece_hash(data: &[u8]) -> Hash  // WRONG!

// GOOD: CPU-bound: Correct approach
fn calculate_piece_hash(data: &[u8]) -> Hash {
    // Synchronous computation
}

// Called as:
let hash = tokio::task::spawn_blocking(move || {
    calculate_piece_hash(&data)
}).await?;
```

### Cancellation Safety

Every async function must be cancellation-safe:

```rust
// BAD: Partial write on cancellation
async fn save_piece(&mut self, piece: Piece) -> Result<()> {
    self.file.write_all(&piece.data).await?;
    self.mark_complete(piece.index);  // Never reached if cancelled!
}

// GOOD: Atomic operation
async fn save_piece(&mut self, piece: Piece) -> Result<()> {
    let temp_path = self.temp_path_for(piece.index);
    tokio::fs::write(&temp_path, &piece.data).await?;
    tokio::fs::rename(&temp_path, self.final_path_for(piece.index)).await?;
    self.mark_complete(piece.index);
}
```

## Testing

### Test Organization

```rust
// Unit tests: Same file, focused
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_piece_picker_rarest_first_selects_minimum() {
        // Test ONE specific behavior
    }
}

// Integration tests: tests/ directory
// tests/torrent_download.rs
#[tokio::test]
async fn test_download_ubuntu_iso() {
    // Real torrent, real tracker, verify completion
}

// Benchmarks: benches/ directory
// benches/piece_selection.rs
use criterion::{criterion_group, criterion_main, Criterion};
```

### Mock Strategy

Mock at protocol boundaries, not internal APIs:

```rust
// GOOD: Mock the wire protocol
pub trait TrackerProtocol: Send + Sync {
    async fn announce(&self, req: AnnounceRequest) -> Result<AnnounceResponse>;
}

#[cfg(test)]
pub struct MockTracker {
    responses: Vec<AnnounceResponse>,
}

// BAD: Mock internal components
trait PiecePickerTrait {  // Don't make traits just for mocking
    fn next_piece(&self) -> Option<PieceIndex>;
}
```

### Property Testing

Use proptest for protocol invariants:

```rust
proptest! {
    #[test]
    fn test_bitfield_never_exceeds_piece_count(
        piece_count: u32,
        set_pieces: Vec<u32>
    ) {
        let bitfield = Bitfield::new(piece_count);
        for &piece in &set_pieces {
            if piece < piece_count {
                bitfield.set(piece);
            }
        }
        prop_assert_eq!(bitfield.count_set(),
                       set_pieces.iter().filter(|&&p| p < piece_count).count());
    }
}
```

## Performance

### Measurement First

Every optimization needs proof:

```rust
// BENCHMARK: peer_message_parsing
// Before: 847ns per message
// After:  623ns per message
//
// Change: Reuse message buffer instead of allocating
// Worth it: 26% improvement × millions of messages = yes
```

### Common Patterns

```rust
// Pre-size collections when size is known
let mut peers = Vec::with_capacity(announce_response.peers.len());

// Use SmallVec for usually-small collections
use smallvec::SmallVec;
type PieceList = SmallVec<[PieceIndex; 8]>;  // Stack storage for ≤8 pieces

// Avoid allocating in loops
// BAD
for piece in pieces {
    let hash = piece.data.to_vec();  // Allocates every iteration
}

// GOOD
let mut hash_buffer = Vec::with_capacity(20);
for piece in pieces {
    hash_buffer.clear();
    hash_buffer.extend_from_slice(&piece.data);
}
```

## Documentation

### Module Documentation

```rust
//! Piece selection algorithms for optimal download performance.
//!
//! Provides multiple strategies:
//! - [`RarestFirst`]: Standard BitTorrent algorithm
//! - [`Sequential`]: For streaming use cases
//! - [`Priority`]: User-specified piece priority
//!
//! The piece picker maintains global piece availability and makes
//! decisions based on the configured strategy.
```

### Function Documentation

```rust
/// Selects the next piece to download based on strategy and availability.
///
/// Considers piece rarity, peer capabilities, and existing requests
/// to maximize download efficiency while avoiding duplicate requests.
///
/// # Returns
/// - `Some(PieceIndex)` - Next piece to request
/// - `None` - No pieces available (complete or all requested)
///
/// # Performance
/// O(log n) for rarest-first, O(1) for sequential.
pub fn select_piece(&mut self, peer: &Peer) -> Option<PieceIndex>
```

### Code Comments

```rust
// GOOD: Explains non-obvious decision
// Use 16 KiB blocks per BitTorrent spec for compatibility
const BLOCK_SIZE: u32 = 16384;

// GOOD: Documents protocol requirement
// Peers MUST send bitfield immediately after handshake
self.expect_bitfield = true;

// BAD: Restates code
let piece_count = torrent.piece_count();  // Get the piece count
```

## Patterns and Anti-Patterns

### Do This

```rust
// Early returns for clarity
pub fn validate_torrent(data: &[u8]) -> Result<Torrent> {
    if data.len() < 100 {
        return Err(TorrentError::TooSmall);
    }

    let dict = bencode::decode(data)?;
    let info = dict.get("info").ok_or(TorrentError::MissingInfo)?;
    // ...
}

// Explicit types for clarity
let peers: Vec<SocketAddr> = response.peers
    .into_iter()
    .filter_map(|p| p.parse().ok())
    .collect();

// Separate concerns
impl Torrent {
    pub fn info_hash(&self) -> InfoHash { ... }      // Pure computation
    pub async fn save(&self, path: &Path) -> Result<()> { ... }  // I/O operation
}
```

### Not This

```rust
// BAD: Deeply nested code
if let Some(torrent) = torrents.get(&info_hash) {
    if torrent.is_complete() {
        if let Some(peer) = torrent.fastest_peer() {
            // ... 5 more levels
        }
    }
}

// BAD: Boolean parameters
engine.start_download(info_hash, true, false);  // What do these mean?

// GOOD: Use enums or builder
engine.start_download(info_hash, DownloadMode::Sequential, Encryption::Optional);

// BAD: Stringly-typed APIs
tracker.set_event("started");

// GOOD: Use enums
tracker.set_event(TrackerEvent::Started);
```

## Commit Messages

### Format

```
type(scope): concise description

Longer explanation if needed. Focus on why, not what.

BENCHMARK: name_of_benchmark (only if performance changed)
Before: X
After: Y
```

### Examples

```bash
feat(torrent): add sequential piece selection

Needed for streaming - pieces must be downloaded in order
to start playback quickly.

fix(tracker): handle compact peer response

Some trackers send peers as 6-byte packed format instead
of dictionary. Now supporting both formats per BEP 23.

perf(streaming): reuse segment buffers

BENCHMARK: stream_segment
Before: 1.2ms per segment, 847 allocations
After: 0.3ms per segment, 2 allocations

Massive reduction in allocator pressure during streaming.
```

### Types

- `feat`: New functionality
- `fix`: Bug fix
- `perf`: Performance improvement
- `refactor`: Code restructuring
- `test`: Test additions/changes
- `docs`: Documentation only
- `chore`: Build/tooling/dependencies

## Code Review Checklist

Before merging, ensure:

- [ ] **Correctness**: Follows BitTorrent/HTTP/HLS specs
- [ ] **Performance**: No allocations in hot paths
- [ ] **Safety**: All unwraps justified or removed
- [ ] **Testing**: Unit tests for logic, integration for protocols
- [ ] **Documentation**: Public APIs fully documented
- [ ] **Naming**: Follows conventions, no ambiguity
- [ ] **Errors**: Proper context, helpful messages
- [ ] **Future-proof**: Won't break when adding features

## Tool Configuration

### Clippy Settings

```toml
# clippy.toml
cognitive-complexity-threshold = 20  # Lower than default
too-many-lines-threshold = 400      # Modules, not monoliths
too-many-arguments-threshold = 5    # Use builders instead

# Warn on these
warn = [
    "clippy::missing_errors_doc",
    "clippy::missing_panics_doc",
    "clippy::exhaustive_enums",
]

# Allow pragmatic code
allow = [
    "clippy::match_bool",           # Sometimes clearer
    "clippy::single_match_else",    # Often more readable
]
```

### Rustfmt Settings

```toml
# rustfmt.toml
max_width = 100                    # Not too wide
use_field_init_shorthand = true    # Clean struct init
imports_granularity = "Module"     # One import per module
group_imports = "StdExternalCrate" # Stdlib, external, crate
```

## Final Wisdom

**Remember**: You're building a media server, not entering an obfuscated code contest.

When making decisions:
1. Will this stream movies reliably?
2. Can someone debug this at 3 AM?
3. Does it actually make things faster? (prove it)
4. Are we solving real problems or theoretical ones?

> "There are two ways of constructing software: make it so simple that there are obviously no deficiencies, or make it so complicated that there are no obvious deficiencies." - C.A.R. Hoare

Choose simple. Every time.
