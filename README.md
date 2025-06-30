# Riptide

[![LICENSE](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)

**BitTorrent media server optimized for streaming.**

> [!WARNING]
> Early development. Core features working, streaming in progress.

## Quick Start

```bash
# Clone and build
git clone https://github.com/mitander/riptide
cd riptide
cargo build --release

# Try simulation mode
./target/release/riptide simulate --peers 5 test.torrent

# Run tests
cargo test
```

## Current Status

**Working:**
- BitTorrent protocol with trait-based architecture (production + simulation)
- Real file-to-torrent conversion with piece splitting and SHA-1 hashing
- Content distribution simulation with actual piece data serving
- Piece reconstruction for end-to-end streaming validation
- Clean workspace architecture with no circular dependencies

**In Progress:**
- Direct streaming service
- Template/asset extraction
- Web interface enhancement

## Development

```bash
# Build and test
cargo build
cargo test

# Standards check
./scripts/check_standards.sh

# Run simulation
cargo run -- simulate --peers 10 test.torrent
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

## Documentation

- [Design](docs/DESIGN.md) - Architecture decisions
- [Style](docs/STYLE.md) - Code conventions  
- [Naming](docs/NAMING_CONVENTIONS.md) - Naming rules

## License

MIT License - see [LICENSE](LICENSE)
