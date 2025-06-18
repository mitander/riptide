# Riptide

[![LICENSE](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)
<!-- [![CI Status](https://github.com/mitander/riptide/actions/workflows/ci.yml/badge.svg)](https://github.com/mitander/riptide/actions) -->

**A high-performance torrent media server built in Rust.**

> [!WARNING]
> This project is under active development.
> The API is unstable and breaking changes are expected until v1.0.

## Overview

Downloads torrents, manages subtitles, and streams directly to your devices. Built for real-world use with zero buffering, automatic transcoding, and VPN protection.

**Current Status:**
- BitTorrent protocol implementation (pure Rust)
- Sequential piece selection for streaming
- Direct HTTP streaming with range requests
- Web UI for library management
- Automatic subtitle fetching

**Coming Next:**
- HLS adaptive streaming
- Hardware-accelerated transcoding
- Native Apple TV app
- Bandwidth scheduling

## Usage

```rust
use riptide::{TorrentEngine, StreamingService};

// Initialize engine
let mut engine = TorrentEngine::builder()
    .download_dir("/media/downloads")
    .max_connections(200)
    .require_vpn(true)
    .build()?;

// Start download
let handle = engine.start_download("magnet:?xt=urn:btih:...")?;

// Stream when ready
let streaming = StreamingService::new();
let stream_url = streaming.prepare_stream(handle.movie_id()).await?;
```

## Performance

Riptide is built for streaming performance:
- **Zero-allocation streaming path**: Pre-allocated buffers, no allocations during playback
- **Smart piece selection**: Deadline-based algorithm maintains playback buffer
- **Direct streaming**: Most content streams without transcoding (>90% of devices)
- **Efficient storage**: Copy-on-write support, no duplicate data

### Benchmarks

```bash
# Run all benchmarks
cargo bench

# Specific components
cargo bench --bench piece_selection
cargo bench --bench streaming_throughput
cargo bench --bench transcode_pipeline
```

Key metrics:
- Piece selection: <100μs for 10K pieces
- Streaming latency: <50ms to first byte
- Transcoding: 4x realtime on modern hardware

## Installation

### From Source

```bash
git clone https://github.com/mitander/riptide
cd riptide
cargo build --release
```

### Docker

```bash
docker run -d \
  -p 3000:3000 \
  -v /media:/media \
  -e REQUIRE_VPN=true \
  riptide:latest
```

## Configuration

```toml
# config.toml
[server]
port = 3000

[torrent]
download_dir = "/media/downloads"
library_dir = "/media/library"
max_active_downloads = 3
connection_limit = 200
require_vpn = true

[streaming]
direct_play_formats = ["h264", "h265", "vp9"]
transcode_preset = "fast"
segment_duration = 6

[seeding]
min_ratio = 1.0
min_hours = 72
```

## API

### REST Endpoints

```
GET    /api/search?q={query}      # Search for torrents
POST   /api/download              # Start download
GET    /api/downloads             # List active downloads
DELETE /api/downloads/{id}        # Cancel download
GET    /api/library               # List downloaded movies
GET    /api/stream/{id}           # Stream movie
```

### WebSocket Events

```javascript
ws.on('download:progress', (data) => {
  console.log(`${data.name}: ${data.progress}%`);
});

ws.on('download:complete', (data) => {
  console.log(`Ready to stream: ${data.movie_id}`);
});
```

## Development

### Building

```bash
# Debug build with checks
cargo build

# Release build with optimizations
cargo build --release

# Run tests
cargo test

# Run with mock torrents (no network)
cargo run -- --mock-mode
```

### Architecture

```
src/
├── torrent/          # BitTorrent engine
├── streaming/        # HTTP streaming server
├── storage/          # File management
├── api/              # REST API
├── web/              # Frontend
└── bin/              # CLI entry point
```

### Testing

```bash
# Unit tests
cargo test

# Integration tests (requires Docker)
./scripts/integration_tests.sh

# Property tests
cargo test --features proptest

# Benchmarks
cargo bench --bench critical_path
```

### Style

See [STYLE.md](docs/STYLE.md) and [NAMING_CONVENTIONS.md](docs/NAMING_CONVENTIONS.md).

Key points:
- Ship working code first
- Measure before optimizing
- No premature abstractions
- Every commit must pass tests

## Documentation

- [Design Document](docs/DESIGN.md) - Architecture and implementation
- [Style Guide](docs/STYLE.md) - Code conventions
- [Naming Conventions](docs/NAMING_CONVENTIONS.md) - Project naming convention

## License

Riptide is licensed under the [MIT License](LICENSE).
