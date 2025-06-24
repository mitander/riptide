#!/bin/bash
# Complete local CI pipeline matching production CI
# Run this before pushing to ensure CI will pass

set -e

echo "Riptide Local CI Pipeline"
echo "========================="

# 1. Standards enforcement
echo "1. Checking coding standards..."
./scripts/check_standards.sh

# 2. Documentation completeness  
echo "2. Checking documentation..."
./scripts/find_missing_docs.sh

# 3. Naming conventions
echo "3. Checking naming conventions..."
./scripts/check_naming.sh

# 4. Build check
echo "4. Building workspace..."
cargo build --workspace --all-targets

# 5. Full test suite
echo "5. Running full test suite..."
cargo test --workspace

# 6. Performance regression check (if benchmarks exist)
if [ -d "benches" ]; then
    echo "6. Running benchmark smoke tests..."
    cargo bench --no-run
fi

echo ""
echo "All CI checks passed!"
echo "Your code is ready for CI/production."