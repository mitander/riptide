#!/bin/bash
# Run performance benchmarks with baseline comparison
# Used for performance regression testing

set -e

echo "Running Riptide Performance Benchmarks"
echo "======================================"

if [ ! -d "benches" ]; then
    echo "No benchmarks directory found. Create benches/ for performance tests."
    exit 0
fi

echo "Building benchmarks..."
cargo bench --no-run

echo "Running benchmarks..."
cargo bench

echo ""
echo "Benchmark run complete."
echo "Check output above for performance regressions."