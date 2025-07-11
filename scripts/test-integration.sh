#!/bin/bash
set -e

# Integration Test Runner for Riptide
#
# This script runs integration and e2e tests explicitly, separate from unit tests.
# Integration tests verify component interactions and end-to-end workflows.

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(dirname "$SCRIPT_DIR")"

cd "$PROJECT_ROOT"

echo "🧪 Riptide Integration Test Runner"
echo "================================="

# Check if riptide-tests is enabled in workspace
if ! grep -q '"riptide-tests"' Cargo.toml; then
    echo "⚠️  riptide-tests is currently excluded from workspace"
    echo "   Re-enable it in Cargo.toml to run integration tests"
    echo ""
    echo "   Uncomment the line: # \"riptide-tests\""
    exit 1
fi

# Parse command line arguments
TEST_TYPE="${1:-all}"
VERBOSE="${2:-false}"

case "$TEST_TYPE" in
    "integration")
        echo "🔧 Running integration tests only..."
        cargo test --package riptide-tests --test integration
        ;;
    "e2e")
        echo "🚀 Running e2e tests only..."
        cargo test --package riptide-tests --test e2e
        ;;
    "all")
        echo "🔧 Running integration tests..."
        cargo test --package riptide-tests --test integration
        echo ""
        echo "🚀 Running e2e tests..."
        cargo test --package riptide-tests --test e2e
        ;;
    "check")
        echo "✅ Checking integration test compilation..."
        cargo check --package riptide-tests
        ;;
    "fix")
        echo "🔨 Attempting to fix one integration test..."
        echo "   This will enable riptide-tests in workspace and try to compile"

        # Temporarily enable riptide-tests
        sed -i.bak 's/# "riptide-tests"/"riptide-tests"/' Cargo.toml

        echo "   Checking compilation..."
        if cargo check --package riptide-tests; then
            echo "✅ Integration tests compile successfully!"
        else
            echo "❌ Integration tests have compilation errors"
            echo "   Reverting workspace changes..."
            mv Cargo.toml.bak Cargo.toml
            exit 1
        fi

        rm -f Cargo.toml.bak
        ;;
    *)
        echo "Usage: $0 [integration|e2e|all|check|fix]"
        echo ""
        echo "Commands:"
        echo "  integration  - Run only integration tests"
        echo "  e2e         - Run only end-to-end tests"
        echo "  all         - Run all integration and e2e tests (default)"
        echo "  check       - Check if integration tests compile"
        echo "  fix         - Attempt to enable and fix integration tests"
        echo ""
        echo "Note: Integration tests are separate from unit tests and must be run explicitly."
        exit 1
        ;;
esac

echo "✅ Integration test run completed"
