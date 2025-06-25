#!/bin/bash
set -euo pipefail

echo "Running local CI checks..."

# Essential checks only
cargo fmt -- --check
cargo clippy --lib --tests --all-features -- -D warnings
cargo test --all-features --lib
cargo check --no-default-features  # Ensure core works without simulation

echo "All CI checks passed!"