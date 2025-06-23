#!/bin/bash
# Riptide Standards Enforcement Script
# Enforces our documented patterns and catches violations automatically

set -e

echo "Checking Riptide coding standards..."

# 1. Format check
echo "Checking code formatting..."
cargo fmt --check
echo "Format check passed"

# 2. Clippy with our specific rules  
echo "Running Clippy with standards enforcement..."
cargo clippy --workspace -- \
    -D warnings \
    -D clippy::wildcard_imports \
    -D clippy::missing_errors_doc
echo "Clippy standards check passed"

# 3. Test all modules
echo "Running all tests..."
cargo test --workspace --quiet
echo "All tests passed"

# 4. Check for anti-patterns
echo "Checking for anti-patterns..."

# Check for utils/helpers files
if find src -name "*util*" -o -name "*helper*" -o -name "*common*" | grep -q .; then
    echo "FAIL: Found utils/helpers files (violates naming conventions):"
    find src -name "*util*" -o -name "*helper*" -o -name "*common*"
    exit 1
fi

# Check for inline module references (crate::module::function)
if grep -r "crate::[a-z_]*::[a-z_]*::" src --include="*.rs" | grep -v "test_data\|test_fixtures"; then
    echo "FAIL: Found inline module references (should use proper imports):"
    grep -r "crate::[a-z_]*::[a-z_]*::" src --include="*.rs" | grep -v "test_data\|test_fixtures"
    exit 1
fi

# Check for missing # Errors in Result-returning public functions
if grep -r "pub.*fn.*Result<" src --include="*.rs" -A 10 | grep -B 5 -A 5 "pub.*fn.*Result<" | grep -L "# Errors" >/dev/null 2>&1; then
    echo "WARNING: Found public Result functions missing # Errors documentation"
fi

echo "Anti-pattern check passed"

echo ""
echo "All standards checks passed!"
echo "Your code follows Riptide conventions."