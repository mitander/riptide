#!/bin/bash
# Quick performance measurement for BitTorrent streaming operations
# Measures actual performance metrics relevant to media streaming

set -e

echo "Riptide Performance Check"
echo "========================="

echo "Building optimized binary..."
cargo build --release --quiet

echo ""
echo "1. Compilation Performance"
echo "-------------------------"
echo "Measuring clean build time..."
cargo clean --quiet
time cargo build --release --quiet

echo ""
echo "2. Memory Usage Baseline"
echo "------------------------"
echo "Binary size:"
ls -lh target/release/riptide | awk '{print $5 " " $9}'

echo ""
echo "3. Core Operation Performance"
echo "-----------------------------"
if cargo test --release --workspace --lib -- --nocapture 2>/dev/null | grep -q "BENCHMARK:"; then
    echo "Running performance-focused tests..."
    cargo test --release --workspace --lib -- --nocapture | grep "BENCHMARK:"
else
    echo "No benchmark tests found. Running streaming tests for basic validation..."
    cargo test --release --workspace -- streaming --quiet
fi

echo ""
echo "4. Critical Path Validation"
echo "---------------------------"
echo "Checking for allocation-heavy patterns in streaming code..."
if grep -r "Vec::new\|to_string\|format!" */src/ --include="*.rs" | grep -v test | head -3; then
    echo "WARNING: Found potential allocations in hot paths"
else
    echo "✓ No obvious allocation patterns in streaming paths"
fi

echo ""
echo "5. Module Complexity Check"
echo "--------------------------"
echo "Largest modules (should be <500 lines):"
find */src/ -name "*.rs" -exec wc -l {} + 2>/dev/null | sort -nr | head -5 | while read lines file; do
    if [ "$lines" -gt 500 ]; then
        echo "WARNING: $file: $lines lines (exceeds 500 line limit)"
    else
        echo "OK: $file: $lines lines"
    fi
done

echo ""
echo "Performance check complete."
echo "For detailed benchmarks, run: ./scripts/run_benchmarks.sh"
