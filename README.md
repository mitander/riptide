# Riptide

[![LICENSE](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)

**BitTorrent media server optimized for streaming.**

> [!WARNING]
> Early development stage. Core BitTorrent engine and streaming components are in active development.

## Overview

Riptide downloads torrents and streams media content with a focus on zero-buffering playback. Built with Rust for performance and reliability, featuring a simulation framework for offline development.

**Current Implementation:**
- Type-safe domain modeling (InfoHash, PieceIndex, TorrentError)
- Async storage abstraction with file-based backend
- Complete BitTorrent simulation environment (MockTracker, MockPeer, NetworkSimulator)
- CLI interface with working simulation mode
- **Trait abstractions for swappable implementations** (TorrentParser, TrackerClient, PeerProtocol)
- **Controlled external dependencies** (bencode-rs, magnet-url)
- Test suite with unit and integration tests

**Next Development Phase:**
- Torrent file parsing implementation using bencode-rs
- HTTP/UDP tracker communication with reqwest/tokio
- BitTorrent wire protocol for peer communication
- Sequential piece picker for streaming optimization
- Direct HTTP streaming with range requests

## Usage

### Current CLI Interface

```bash
# Add a torrent (simulation)
riptide add "magnet:?xt=urn:btih:..."

# Run simulation environment with mock peers and network conditions
riptide simulate --peers 10 /path/to/test.torrent

# Show help
riptide --help
```

### Planned API (In Development)

```rust
use riptide::{
    TorrentEngine,
    storage::FileStorage,
    torrent::{BencodeTorrentParser, HttpTrackerClient, BitTorrentPeerProtocol}
};

// Initialize with trait implementations
let parser = BencodeTorrentParser::new();
let tracker = HttpTrackerClient::new("http://tracker.example.com/announce".to_string());
let storage = FileStorage::new("/media/downloads".into(), "/media/library".into());

let mut engine = TorrentEngine::new();

// Parse and add torrent using trait abstractions
let metadata = parser.parse_magnet_link("magnet:?xt=urn:btih:...").await?;
let info_hash = engine.add_torrent(metadata).await?;
engine.start_download(info_hash).await?;
```

## Development

### Mock-First Strategy

Riptide uses simulation for rapid development without external dependencies:

```bash
# Test simulation environment
cargo run -- simulate --peers 15 /path/to/test.torrent

# Run comprehensive test suite
cargo test

# Run integration tests with simulated network conditions
cargo test --test integration

# Performance benchmarks (development stage)
cargo bench
```

### Current Architecture

```
src/
├── torrent/          # BitTorrent protocol implementation
│   ├── engine.rs     # Download orchestration
│   ├── piece_picker.rs # Streaming-optimized piece selection
│   └── peer_connection.rs # Wire protocol handling
├── storage/          # Async storage abstraction
│   └── file_storage.rs # Directory-based implementation
├── simulation/       # Mock environment for offline development
│   ├── tracker.rs    # Mock BitTorrent tracker
│   ├── network.rs    # Network condition simulation
│   └── peer.rs       # Mock peer behavior
└── cli/              # Command-line interface
    └── commands.rs   # CLI command implementations
```

## Installation

### From Source

```bash
git clone https://github.com/mitander/riptide
cd riptide
cargo build --release
./target/release/riptide --help
```

### Development Setup

```bash
# Install with development dependencies
cargo build

# Run tests to verify setup
cargo test

# Try simulation mode
cargo run -- simulate --peers 5 /tmp/test.torrent
```

## Project Status

**Current Development Stage**: Foundation and simulation framework complete

**Ready for Use:**
- Simulation environment for BitTorrent development
- Type-safe domain modeling and error handling
- Async storage abstraction with file backend
- CLI interface for testing and development

**In Active Development:**
- BitTorrent protocol implementation
- Peer connection management
- Piece downloading and verification
- Direct streaming capabilities

**Planned Features:**
- Web UI for library management
- Automatic transcoding for device compatibility
- VPN integration and kill switch
- Bandwidth scheduling and QoS

## Documentation

- [Design Document](docs/DESIGN.md) - Architecture and implementation
- [Style Guide](docs/STYLE.md) - Code conventions
- [Naming Conventions](docs/NAMING_CONVENTIONS.md) - Project naming convention

## License

Riptide is licensed under the [MIT License](LICENSE).
