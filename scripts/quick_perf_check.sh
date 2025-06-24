#!/bin/bash
# Quick performance check for common operations
# Runs key performance tests without full benchmark suite

set -e

echo "Quick Performance Check"
echo "======================"

echo "Building optimized binary..."
cargo build --release

echo "Running performance-critical tests..."
cargo test --release --workspace -- perf

echo "Testing compilation time..."
time cargo check --workspace

echo ""
echo "Quick performance check complete."