# Riptide

[![LICENSE](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)
[![CI Status](https://github.com/mitander/riptide/actions/workflows/ci.yml/badge.svg)](https://github.com/mitander/riptide/actions)

**Riptide is a modern BitTorrent media server built for streaming performance and rapid iteration.**

> [!WARNING]
> This media server is in early development phase.
> Features are incomplete and breaking changes should be expected.

## Overview

Riptide implements BitTorrent protocol with trait-based architecture enabling both production networking and deterministic simulation. Built with focus on correctness, performance, and maintainability.

**Current Status:**

- BitTorrent protocol: Complete tracker communication, peer connections, piece downloading
- Streaming service: Direct MP4/WebM playback, FFmpeg conversion for MKV/AVI
- Trait-based architecture: Swappable production/simulation implementations
- Piece downloader with hash verification and timeout controls
- Storage layer with piece management and file reconstruction
- Deterministic simulation framework for reproducible testing

**Coming Next:**

- Sequential piece picking for streaming optimization
- Upload throttling to prioritize download bandwidth
- Media search integration and metadata services
- Enhanced web UI for library browsing

## Dependencies

- **FFmpeg** - Required for media transcoding and remuxing

```bash
# Ubuntu/Debian
sudo apt install ffmpeg libavutil-dev libavformat-dev libavcodec-dev

# macOS
brew install ffmpeg
```

## Streaming Support

Riptide provides direct browser streaming for downloaded content:

- **MP4/WebM**: Direct streaming without conversion
- **MKV/AVI/MOV**: Automatic FFmpeg conversion to MP4
- **Range requests**: Supports seeking and partial content
- **Real-time**: Streams content as it downloads (when sufficient pieces available)

Access streaming via: `http://localhost:3000/stream/{torrent_hash}`

## Usage

```bash
# Clone and build
git clone https://github.com/mitander/riptide
cd riptide
cargo build --release

# Start web server in development mode (simulation data)
cargo run --bin riptide -- server --mode development

# Add torrents via magnet links
cargo run --bin riptide -- add "magnet:?xt=urn:btih:..."

# Start server with custom host/port
cargo run --bin riptide -- server --host 0.0.0.0 --port 8080

# Start server with local movie files for simulation
cargo run --bin riptide -- server --mode development --movies-dir /path/to/movies

# Run simulation scenarios for testing
cargo test -p riptide-sim -- --nocapture
```

## Architecture

Multi-crate workspace with clean dependency structure:

```
riptide-core (foundational)
    ↑
    ├── riptide-sim (simulation & testing)
    ├── riptide-web (UI & API)
    ├── riptide-cli (command interface)
    └── riptide-search (media discovery)
```

- **riptide-core**: BitTorrent protocol, streaming, trait abstractions
- **riptide-sim**: Simulation implementations, deterministic testing
- **riptide-web**: HTTP API, UI, template rendering
- **riptide-cli**: Command interface, development tools
- **riptide-search**: Media search, metadata services

## Development

### Building

```bash
cargo build --release
cargo test --workspace
cargo clippy --workspace -- -D warnings
```

### Development Setup

The project uses strict quality gates enforced by `cargo-husky` pre-commit hooks:

```bash
# Hooks are automatically installed when you build the project
cargo build

# The pre-commit hook runs automatically on commit and checks:
# - Code formatting (cargo fmt)
# - Linting with strict clippy (warnings treated as errors)
# - Core tests
```

### Style

See [STYLE.md](docs/STYLE.md) and [NAMING_CONVENTIONS.md](docs/NAMING_CONVENTIONS.md).

Key points:

- Working over clever
- Measured over assumed
- Simple over flexible
- Comprehensive tests required

## Documentation

- [Design](docs/DESIGN.md) - Architecture and implementation
- [Style Guide](docs/STYLE.md) - Code conventions
- [Naming Conventions](docs/NAMING_CONVENTIONS.md) - Naming rules

## License

Riptide is licensed under the [MIT License](LICENSE).
