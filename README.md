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
- Trait-based architecture: Swappable production/simulation implementations
- Piece downloader with hash verification and timeout controls
- Storage layer with piece management and file reconstruction
- Deterministic simulation framework for reproducible testing

**Coming Next:**

- Streaming optimization with deadline-based piece selection
- Media search integration and metadata services
- Web UI for library browsing

## Dependencies

- **FFmpeg** - Required for media transcoding and remuxing

```bash
# Ubuntu/Debian
sudo apt install ffmpeg libavutil-dev libavformat-dev libavcodec-dev

# macOS
brew install ffmpeg
```

## Usage

```bash
# Clone and build
git clone https://github.com/mitander/riptide
cd riptide
cargo build --release

# Start web server with demo data
cargo run -p riptide-cli -- server --demo

# Add torrents via magnet links
cargo run -p riptide-cli -- add "magnet:?xt=urn:btih:..."

# Start server in development mode
cargo run -p riptide-cli -- server --host 0.0.0.0 --port 8080 --development

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
