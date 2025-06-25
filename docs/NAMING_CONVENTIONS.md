# Riptide Naming Conventions

Clear names prevent bugs. Ambiguous names cause them.

## Core Principle: Domain Clarity Over Brevity

BitTorrent and media streaming have overlapping terminology. A "piece" in BitTorrent is not a "segment" in HLS. A "peer" is not a "client". Names must disambiguate context across workspace crates.

## Workspace-Specific Naming

### Crate Naming Pattern

```
riptide-{domain}
```

- **riptide-core** - Essential functionality (protocol + streaming)
- **riptide-web** - Web interface concerns
- **riptide-search** - Media discovery and metadata
- **riptide-cli** - Command-line interface

### Cross-Crate Type Disambiguation

When types might conflict across crates, use descriptive prefixes:

```rust
// riptide-core
pub struct TorrentEngine;        // Core BitTorrent engine
pub struct StreamingService;     // Direct streaming service

// riptide-web  
pub struct WebServer;            // HTTP server
pub struct WebHandlers;          // Request handlers
pub struct TemplateEngine;       // HTML rendering

// riptide-search
pub struct MediaSearchService;   // Search coordination
pub struct TorrentResult;        // Search result item

// riptide-cli
pub enum Commands;               // CLI commands
```

## Functions

### Action Patterns

```rust
// State mutations
start_download()        // Not download() - ambiguous with noun
pause_torrent()        // Not pause() - what are we pausing?
transcode_segment()    // Not process() - too vague

// Queries (pure, no side effects)
is_seeding()           // Boolean
has_piece()            // Boolean ownership check
can_stream_direct()    // Boolean capability check

// Calculations (pure, potentially expensive)
calculate_piece_hash()  // Not just hash() - hash what?
compute_bitfield()     // Explicit about computation cost

// Conversions
as_magnet_link()       // Borrowing conversion
into_streaming_response() // Consuming conversion
from_torrent_file()    // Constructor

// Fallible operations
parse_torrent_data()      // Returns Result - descriptive name is sufficient
parse_announce_response() // Returns Result - `try_` prefix unnecessary
decode_peer_message()     // Returns Result - operation name implies fallibility

// Use `try_` ONLY when there's a panic-based alternative
reserve_buffer()          // Panics on allocation failure
try_reserve_buffer()      // Returns Result on allocation failure

// Network operations (always async)
async fn connect_to_peer()     // Not connect()
async fn fetch_piece()         // Not get_piece()
async fn announce_to_tracker() // Not announce()
```

### Workspace Error Naming

Each crate defines specific error types with descriptive names:

```rust
// riptide-core/src/lib.rs
pub enum RiptideError {
    Torrent(TorrentError),
    Storage(StorageError), 
    Streaming(StreamingError),
    WebUI { reason: String },        // Cross-crate conversion
}

// riptide-web/src/lib.rs
pub enum WebUIError {
    TemplateError { reason: String },
    ServerStartFailed { address: SocketAddr, reason: String },
    HandlerError { reason: String },
}

// riptide-search/src/lib.rs  
pub enum MediaSearchError {
    SearchFailed { query: String, reason: String },
    NetworkError { reason: String },
    ParseError { reason: String },
}
```

### Cross-Crate Function Naming

Functions that work across crate boundaries should be explicit:

```rust
// riptide-cli/src/commands.rs
pub async fn start_server(host: String, port: u16, demo: bool) -> Result<()>
pub async fn add_torrent(source: String, output: Option<PathBuf>) -> Result<()>

// riptide-core/src/lib.rs - conversion helpers
impl RiptideError {
    pub fn from_web_ui_error(error: impl std::fmt::Display) -> Self
    pub fn from_search_error(error: impl std::fmt::Display) -> Self  
}
```

### The Specificity Rule

**Public APIs**: Maximum specificity
```rust
// BAD
pub fn get(&self, id: u64) -> Option<Data>
pub async fn process(&mut self, input: &[u8]) -> Result<()>

// GOOD
pub fn get_torrent(&self, info_hash: InfoHash) -> Option<&Torrent>
pub async fn handle_peer_message(&mut self, message: &PeerMessage) -> Result<()>
```

**Internal APIs**: Context-appropriate specificity
```rust
// In piece_picker.rs - context is clear
fn next_piece(&self) -> Option<PieceIndex>  // OK

// In main engine - needs specificity
fn next_piece_to_download(&self) -> Option<PieceIndex>  // Better
```

## Types

### Struct Naming

```rust
// Entities (things that exist)
Torrent              // Not TorrentInfo or TorrentData
Peer                 // Not PeerConnection (that's the connection TO a peer)
Piece                // Not PieceData
Movie                // Not MovieInfo or MediaItem

// Actors (things that do work)
TorrentEngine        // Not Engine or TorrentManager
PiecePicker          // Not Picker or PieceSelector
StreamingService     // Not Streamer or StreamHandler
SubtitleSynchronizer // Not SubtitleSync (noun/verb ambiguity)

// Connections/Sessions
PeerConnection       // The connection TO a peer
TrackerSession       // The session WITH a tracker
StreamingSession     // Active streaming state

// Strategies/Algorithms
SequentialPicker     // Implements PiecePicker trait
RarestFirstPicker    // Clear about algorithm
EndgamePicker        // Domain-specific term
```

### Enum Naming

```rust
// States (mutually exclusive)
enum TorrentState {
    Downloading { progress: f32 },
    Seeding { ratio: f32 },
    Paused,
    Error { reason: String },
}

// Results/Outcomes
enum PieceVerification {
    Valid,
    CorruptData { expected: Hash, actual: Hash },
    MissingData,
}

// Message types (use domain terminology)
enum PeerMessage {
    Choke,              // BitTorrent protocol names
    Unchoke,
    Interested,
    NotInterested,
    Have { piece_index: u32 },
    Bitfield(Bitfield),
    Request { index: u32, begin: u32, length: u32 },
    Piece { index: u32, begin: u32, data: Bytes },
    Cancel { index: u32, begin: u32, length: u32 },
}
```

### Type Parameters

```rust
// Use full words for public APIs
pub struct Cache<Key, Value> {
    entries: HashMap<Key, Value>,
}

// Single letters OK for well-established patterns
impl<T> From<T> for BoxedError where T: Error + Send + Sync + 'static

// Domain-specific types get descriptive names
pub struct PieceStore<Storage: BlockStorage> {
    backend: Storage,
}
```

## Variables

### The Context Rule

Variable name length should match lifetime and scope:

```rust
// WRONG
let t = get_torrent(info_hash);  // Long-lived object, short name
downloads.iter().for_each(|download_info_with_metadata| {  // Short-lived, long name

// RIGHT
let torrent = get_torrent(info_hash);
downloads.iter().for_each(|d| {
    d.update_progress();
});
```

### Iterator Variables

```rust
// Collections: Use singular form
for torrent in torrents { }
for peer in peers { }

// Index-based: i, j, k are fine for simple loops
for i in 0..pieces.len() { }

// Complex iterations: Be specific
for (piece_index, piece_data) in pieces.iter().enumerate() { }
```

### Domain-Specific Abbreviations

Only these abbreviations are allowed in any context:

```rust
// Protocol-defined (from BitTorrent spec)
info_hash   // Not ih
peer_id     // Not pid
piece_index // Not pi or idx

// Well-established in domain
tx          // Transmission/Transaction (context-dependent)
rx          // Reception

// NEVER abbreviate these
context     // Not ctx
sequence    // Not seq
manager     // Not mgr
handler     // Not hdlr
buffer      // Not buf
```

## Constants

### Naming Pattern

`[SCOPE_]CATEGORY_DESCRIPTION`

```rust
// Protocol constants (from BitTorrent spec)
const BT_PROTOCOL_HEADER: &[u8] = b"\x19BitTorrent protocol";
const BT_PIECE_BLOCK_SIZE: u32 = 16384;  // 16 KiB
const BT_MAX_PEER_CONNECTIONS: usize = 50;

// Application constants
const DEFAULT_DOWNLOAD_DIR: &str = "/media/downloads";
const STREAMING_SEGMENT_DURATION: u64 = 6;  // seconds
const TRANSCODE_WORKER_THREADS: usize = 4;

// Module-specific (no prefix needed in module)
mod tracker {
    const ANNOUNCE_INTERVAL: u64 = 1800;  // 30 minutes
    const MIN_ANNOUNCE_INTERVAL: u64 = 300;  // 5 minutes
}
```

## Modules and Files

### Organization Rules

```rust
// Feature-based, not type-based
torrent/
    mod.rs              // Public API only
    engine.rs           // Core engine
    piece_picker.rs     // Piece selection algorithms
    peer_connection.rs  // Peer protocol handling
    tracker_client.rs   // Tracker communication

// NOT this
torrent/
    structs.rs         // BAD: Type-based
    utils.rs           // BAD: Grab bag
    helpers.rs         // BAD: Vague
    common.rs          // BAD: Unclear
```

### Module Naming

1. Use nouns for things: `tracker`, `peer`, `piece`
2. Use verbs for processes: `download`, `streaming`, `transcoding`
3. Never plural unless it's a collection module
4. No redundant prefixes: `piece_picker.rs` not `piece_picker_module.rs`

## Error Types

### Error Variants

Include context in the variant name:

```rust
pub enum TorrentError {
    // Include what failed and why
    TrackerConnectionFailed { url: String, reason: io::Error },
    PieceHashMismatch { piece_index: u32, expected: Hash, actual: Hash },
    InsufficientDiskSpace { required: u64, available: u64 },

    // Not just "Failed" or "Invalid"
    InvalidTorrentFile { reason: String },
    PeerProtocolViolation { peer_id: PeerId, violation: String },
}
```

## Test Naming

### Pattern: `test_unit_condition_outcome`

```rust
#[test]
fn test_piece_picker_sequential_returns_in_order() { }

#[test]
fn test_peer_connection_timeout_triggers_disconnect() { }

#[test]
fn test_torrent_pause_stops_peer_connections() { }

// Property tests
#[test]
fn prop_piece_download_never_exceeds_file_size() { }

// Benchmarks
#[bench]
fn bench_piece_hash_calculation() { }
```

## Comments and Documentation

### Documentation Comments

```rust
/// Starts downloading the specified torrent.
///
/// Connects to trackers, discovers peers, and begins piece acquisition using
/// the configured piece selection strategy. Returns handle to monitor and control
/// the download.
///
/// # Errors
/// - `TorrentError::InvalidTorrentFile` - Failed to parse magnet link
/// - `TorrentError::TrackerConnectionFailed` - Could not reach tracker
///
/// # Examples
/// ```
/// let handle = engine.start_download("magnet:?xt=...")?;
/// ```
pub async fn start_download(&mut self, magnet_link: &str) -> Result<DownloadHandle>
```

### Inline Comments

```rust
// Explain why, not what
// BAD
let piece_size = 16384;  // Set piece size to 16384

// GOOD
let piece_size = 16384;  // BitTorrent spec recommends 16 KiB for compatibility

// Domain knowledge
// Endgame mode: Request all remaining pieces from all peers
// to avoid waiting for a single slow peer
if pieces_remaining < 10 {
    self.enter_endgame_mode();
}
```

## Special Cases

### Builder Pattern

```rust
pub struct TorrentEngineBuilder {
    download_dir: Option<PathBuf>,
    max_connections: Option<usize>,
    // ...
}

impl TorrentEngineBuilder {
    pub fn download_dir(mut self, dir: PathBuf) -> Self {
        self.download_dir = Some(dir);
        self
    }

    // Not set_download_dir or with_download_dir
}
```

### Async Functions

Always prefix with verb indicating async operation:

```rust
async fn fetch_piece()        // Network fetch
async fn load_torrent()       // Disk I/O
async fn await_seeders()      // Waiting operation
async fn process_messages()   // Message loop
```

### Callback Types

```rust
type ProgressCallback = Box<dyn Fn(f32) + Send + Sync>;
type CompletionHandler = Box<dyn FnOnce(Result<()>) + Send>;

// Not just Callback or Handler
```

## Enforcement

### CI Enforcement (Automatic)
- Public API must use full words
- No utils.rs or helpers.rs modules
- Test names must start with test_ or prop_ or bench_
- Constants must be SCREAMING_SNAKE_CASE
- Public types must have documentation

### Code Review (Human Judgment)
- Internal naming clarity
- Appropriate abbreviation use
- Comment quality
- Domain term accuracy

## Quick Reference

### DO
- Use domain terms correctly (piece vs segment)
- Be specific in public APIs
- Include units in names when ambiguous (`timeout_seconds`)
- Use standard Rust patterns (`.into_iter()`, `as_ref()`)

### DON'T
- Abbreviate in public APIs (`msg` → `message`)
- Use generic names (`process`, `handle`, `manage`)
- Mix domain metaphors (torrent "segments", streaming "pieces")
- Create utils.rs modules

### THINK
- Will this name make sense in 6 months?
- Does it disambiguate from similar concepts?
- Is the abbreviation universally understood?
- Does the name indicate fallibility (try_) or asyncness?

## Workspace-Specific Guidelines

### Module Naming by Crate

```rust
// riptide-core/src/ - Technical focus
torrent/engine.rs               // Core BitTorrent protocol
streaming/http_server.rs        // HTTP range request handling  
storage/file_storage.rs         // Copy-on-write file operations
config.rs                       // Configuration management

// riptide-web/src/ - Web focus  
handlers.rs                     // HTTP request handlers
server.rs                       // Web server setup and routing
templates.rs                    // HTML template rendering
static_files.rs                 // CSS, JS, image serving

// riptide-search/src/ - Search focus
service.rs                      // Search coordination and providers
// Future: providers/magneto.rs, providers/imdb.rs

// riptide-cli/src/ - Command focus
commands.rs                     // CLI command implementations 
main.rs                         // Argument parsing and dispatch
```

### Import Naming Conventions

```rust
// Avoid naming conflicts by using crate-specific aliases when needed
use riptide_core::config::RiptideConfig;
use riptide_core::torrent::TorrentEngine as CoreEngine;
use riptide_web::server::WebServer;
use riptide_search::service::MediaSearchService;

// For common types, use fully qualified names
use riptide_core::{Result as CoreResult, RiptideError};
use riptide_web::{Result as WebResult, WebUIError};
use riptide_search::{Result as SearchResult, MediaSearchError};
```

### Test Naming by Crate

```rust
// riptide-core tests - Focus on protocol correctness
#[test]
fn test_torrent_engine_handles_invalid_info_hash() { }
#[test] 
fn test_piece_picker_streaming_prioritizes_sequential() { }
#[test]
fn test_storage_copy_on_write_preserves_data() { }

// riptide-web tests - Focus on HTTP behavior
#[test]
fn test_web_handlers_return_valid_json() { }
#[test]
fn test_template_engine_renders_without_errors() { }
#[test]
fn test_static_files_serve_correct_mime_types() { }

// riptide-search tests - Focus on search quality
#[test]
fn test_search_results_ranked_by_quality() { }
#[test]
fn test_demo_provider_returns_realistic_data() { }

// riptide-cli tests - Focus on command parsing
#[test]
fn test_add_command_validates_magnet_links() { }
#[test]
fn test_server_command_starts_with_correct_config() { }

// Integration tests - Cross-crate workflows
#[test]
fn test_search_to_stream_complete_workflow() { }
```

### Error Message Consistency

```rust
// Include crate context in error messages for debugging
impl fmt::Display for RiptideError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            RiptideError::WebUI { reason } => 
                write!(f, "Web UI error: {reason}"),
            RiptideError::Torrent(e) => 
                write!(f, "Core torrent error: {e}"),
            // Always prefix with component context
        }
    }
}
```

These workspace-specific guidelines ensure **consistent naming** across all crates while maintaining **clear separation of concerns** and enabling **efficient cross-crate development**.
