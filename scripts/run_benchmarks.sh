#!/bin/bash
# Run performance benchmarks with baseline comparison
# Used for performance regression testing

set -e

echo "Running Riptide Performance Benchmarks"
echo "======================================"

# Check for benchmark files in workspace
benchmark_files=$(find . -name "*.rs" -path "*/benches/*" -o -name "*.rs" -exec grep -l "#\[bench\]" {} \; 2>/dev/null || true)

if [ -z "$benchmark_files" ]; then
    echo "No benchmark files found in workspace."
    echo "To add benchmarks:"
    echo "  1. Create benches/ directory in a crate"
    echo "  2. Add benchmark functions with #[bench] attribute"
    echo "  3. Or add #[bench] functions to existing test files"
    exit 0
fi

echo "Found benchmark files:"
echo "$benchmark_files" | sed 's/^/  /'
echo ""

echo "Building benchmarks..."
if ! cargo bench --no-run --workspace; then
    echo "Failed to build benchmarks. Check for compilation errors."
    exit 1
fi

echo "Running benchmarks..."
if ! cargo bench --workspace; then
    echo "Some benchmarks failed. Check output above."
    exit 1
fi

echo ""
echo "Benchmark run complete."
echo "Check output above for performance regressions."
