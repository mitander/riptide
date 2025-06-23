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
- BitTorrent protocol implementation
- Piece downloading with hash verification  
- File storage with library organization
- Simulation environment for testing

**In Progress:**
- Direct streaming service
- Web interface

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

## Documentation

- [Design](docs/DESIGN.md) - Architecture decisions
- [Style](docs/STYLE.md) - Code conventions  
- [Naming](docs/NAMING_CONVENTIONS.md) - Naming rules

## License

MIT License - see [LICENSE](LICENSE)
