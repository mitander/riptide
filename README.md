# Riptide

[![LICENSE](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)
[![CI Status](https://github.com/mitander/riptide/actions/workflows/ci.yml/badge.svg)](https://github.com/mitander/riptide/actions)

BitTorrent media server built for streaming performance and deterministic testing. Rust/Tokio/Axum stack with trait-based architecture enabling production and simulation implementations.

## Architecture

Multi-crate workspace with clean dependency boundaries:

```
riptide-core (BitTorrent protocol, streaming pipeline)
    ├── riptide-sim (deterministic simulation, testing)
    ├── riptide-web (HTTP API, streaming endpoints)
    ├── riptide-cli (command interface)
    └── riptide-search (media discovery, metadata)
```

**Core Features:**

- Complete BitTorrent implementation (tracker announce, peer wire protocol, piece verification)
- Stateless streaming pipeline: `DataSource -> StreamProducer -> HTTP Response`
- Direct MP4/WebM streaming with HTTP range support
- Real-time remuxing (MKV/AVI → MP4) via FFmpeg input pump
- Deterministic simulation framework for reproducible testing
- Sequential piece picking optimized for streaming workloads

## Streaming

**Direct formats:** MP4, WebM (zero-copy HTTP range requests)
**Remuxed formats:** MKV, AVI, MOV (real-time FFmpeg transcoding)

Access: `http://localhost:3000/stream/{torrent_hash}`

FFmpeg command used:

```bash
ffmpeg -i pipe:0 -c:v copy -c:a copy -movflags frag_keyframe+empty_moov+default_base_moof -f mp4 pipe:1
```

## Usage

```bash
# Build
cargo build --release

# Development server (simulation mode)
cargo run --bin riptide -- server --mode development

# Production server
cargo run --bin riptide -- server --host 0.0.0.0 --port 8080

# Add torrent
cargo run --bin riptide -- add "magnet:?xt=urn:btih:..."

# Run deterministic simulation tests
cargo test -p riptide-sim -- --nocapture
```

## Dependencies

**Required:** FFmpeg (transcoding and media analysis)

```bash
# Ubuntu/Debian
sudo apt install ffmpeg libavutil-dev libavformat-dev libavcodec-dev

# macOS
brew install ffmpeg
```

## Development

**Quality gates enforced by cargo-husky:**

- Code formatting (`cargo fmt`)
- Clippy linting (warnings as errors)
- Test suite
- Conventional commit messages

```bash
# Standard workflow
cargo build --release
cargo test --workspace
cargo clippy --workspace -- -D warnings

# Hooks auto-install on first build
```

**Style:** Pragmatic naming convention, deep modules with simple interfaces, static over dynamic dispatch, measured optimizations only.

**Testing:** Unit tests, integration tests, deterministic simulation scenarios, property-based testing with real media files.

## Implementation Details

**BitTorrent Engine:** Single-threaded actor model with message passing. Trait abstractions (`PeerManager`, `TrackerManagement`) enable swappable production/simulation implementations.

**Streaming Pipeline:** Stateless design with `PieceProvider` (file-like interface) and `StreamProducer` (HTTP response generator) traits. Complete decoupling between BitTorrent and media concerns.

**Storage:** Simple directory structure with copy-on-write optimization. No content-addressing complexity.

**Performance:** Zero allocations in hot paths, batched disk I/O, upload bandwidth limiting (20% cap), streaming piece picker for sequential downloads.

## Documentation

- [Design](docs/DESIGN.md) - Architecture decisions and implementation details
- [Style Guide](docs/STYLE.md) - Coding standards and conventions
- [Streaming Pipeline](docs/architecture/streaming-pipeline.md) - Streaming architecture specification

## License

MIT License
