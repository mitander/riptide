# Riptide Testing Guide

This document provides comprehensive guidance on testing within the Riptide project, including test types, execution strategies, and quality standards.

## Overview

Riptide employs a multi-layered testing strategy designed to ensure correctness, reliability, and maintainability:

- **Unit Tests** (316 tests): Test individual components in isolation
- **Integration Tests** (44 tests): Test component interactions and workflows
- **Engine Tests** (49 tests): Comprehensive coverage of the core torrent engine
- **E2E Tests** (currently disabled): Full application workflow validation

## Test Architecture

### Test Types

#### Unit Tests
- **Location**: Within each crate (`src/**/*test*.rs`, `#[cfg(test)]` modules)
- **Purpose**: Validate individual functions, methods, and data structures
- **Scope**: Single component, mocked dependencies
- **Count**: 316 tests across all packages

#### Integration Tests
- **Location**: `riptide-tests/integration/`
- **Purpose**: Test component interactions and complete workflows
- **Scope**: Multiple components working together
- **Count**: 44 tests covering streaming, remuxing, peer communication, and more

#### Engine Integration Tests
- **Location**: `riptide-core/tests/engine_integration_tests.rs`
- **Purpose**: Comprehensive testing of the torrent engine using public APIs
- **Scope**: Complete torrent lifecycle testing with TorrentEngineHandle
- **Count**: 13 tests covering download workflows, streaming, and error recovery

## Test Runner Usage

### Basic Commands

```bash
# Run all available tests (integration + unit)
./scripts/test_runner.sh all

# Run only integration tests
./scripts/test_runner.sh integration

# Run only unit tests for all packages
./scripts/test_runner.sh unit

# Run riptide-core tests with test-utils features
./scripts/test_runner.sh core

# Check if integration tests compile
./scripts/test_runner.sh check
```

### Advanced Commands

```bash
# Attempt to enable and fix integration tests
./scripts/test_runner.sh fix

# Run E2E tests (currently disabled)
./scripts/test_runner.sh e2e

# Show help and all available commands
./scripts/test_runner.sh help
```

### Direct Cargo Commands

```bash
# Run unit tests for a specific package
cargo test --package riptide-core --lib

# Run integration tests with features
cargo test --package riptide-tests --test integration

# Run engine tests with test-utils
cargo test --package riptide-core --features test-utils

# Run specific test by name
cargo test --package riptide-core test_complete_torrent_lifecycle
```

## Test Coverage Metrics

### Current Status (✓ = Passing, ⚠ = Issues)

| Test Type | Count | Status | Coverage |
|-----------|-------|--------|----------|
| Unit Tests | 316 | ✓ | All packages |
| Integration Tests | 44 | ✓ | Core workflows |
| Engine Unit Tests | 36 | ✓ | All engine methods |
| Engine Integration Tests | 13 | ✓ | Public API workflows |
| E2E Tests | ~10 | ⚠ | Disabled (import issues) |

### Quality Gates

All tests must pass these criteria:

- **100% Pass Rate**: No failing tests allowed in CI
- **No Clippy Warnings**: All code must pass strict linting
- **Proper Formatting**: Code must be formatted with `cargo fmt`
- **Style Compliance**: Must follow `docs/STYLE.md` conventions

## CI Integration

### GitHub Actions Workflow

The CI pipeline runs tests in this order:

```yaml
1. Format check (cargo fmt --check)
2. Linting (cargo clippy --workspace -- -D warnings)
3. Unit tests (cargo test --workspace --lib)
4. Core tests with features (cargo test --package riptide-core --features test-utils)
5. Integration tests (./scripts/test_runner.sh integration)
6. Release build (cargo build --workspace --release)
```

### Local Pre-commit Checks

Before committing, ensure all tests pass:

```bash
# Quick validation
./scripts/test_runner.sh check

# Full test suite
./scripts/test_runner.sh all

# Engine-specific tests
./scripts/test_runner.sh core
```

## Test Development Guidelines

### Writing Unit Tests

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_specific_behavior() {
        // Arrange
        let input = create_test_input();

        // Act
        let result = function_under_test(input);

        // Assert
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), expected_value);
    }

    #[tokio::test]
    async fn test_async_behavior() {
        let result = async_function().await;
        assert!(result.is_ok());
    }
}
```

### Writing Integration Tests

```rust
// In riptide-tests/integration/
use riptide_core::engine::TorrentEngineHandle;

#[tokio::test]
async fn test_complete_workflow() {
    // Setup
    let handle = create_test_engine().await;

    // Execute workflow
    let info_hash = handle.add_magnet("magnet:?...").await?;
    handle.start_download(info_hash).await?;

    // Verify results
    let session = handle.session_details(info_hash).await?;
    assert!(session.is_downloading);
}
```

### Test Naming Conventions

- **Unit tests**: `test_[function_name]_[scenario]`
- **Integration tests**: `test_[workflow_name]_[integration_type]`
- **Edge cases**: `test_[function_name]_edge_case_[specific_case]`
- **Error handling**: `test_[function_name]_error_[error_type]`

## Troubleshooting

### Common Issues

#### "E2E tests are currently disabled"
E2E tests have API import issues and are temporarily disabled. They can be re-enabled by:
1. Fixing import paths in `riptide-tests/e2e/streaming_workflow.rs`
2. Uncommenting the e2e test target in `riptide-tests/Cargo.toml`

#### "riptide-tests is currently excluded from workspace"
Integration tests require riptide-tests to be enabled in the root `Cargo.toml`:
```toml
members = [
    "riptide-core",
    "riptide-web",
    "riptide-search",
    "riptide-cli",
    "riptide-sim",
    "riptide-tests"  # Uncomment this line
]
```

#### Test hangs or deadlocks
- Check for infinite loops in async code
- Ensure all async operations have timeouts
- Use `tokio::time::timeout()` for potentially blocking operations

#### FFmpeg-related test failures
- Ensure FFmpeg is installed and available in PATH
- Check that required FFmpeg libraries are installed
- Use `ffmpeg -version` to verify installation

### Debugging Tests

```bash
# Run tests with output capture disabled
cargo test --package riptide-core -- --nocapture

# Run a specific test with debug output
RUST_LOG=debug cargo test test_specific_function

# Run tests with backtrace on panic
RUST_BACKTRACE=1 cargo test

# Run tests with extra verbose output
cargo test -- --test-threads=1 --nocapture
```

## Performance Testing

### Benchmarks
Performance-critical code should include benchmarks:

```rust
#[cfg(test)]
mod benches {
    use criterion::{black_box, criterion_group, criterion_main, Criterion};

    fn bench_critical_function(c: &mut Criterion) {
        c.bench_function("critical_function", |b| {
            b.iter(|| critical_function(black_box(test_input)))
        });
    }

    criterion_group!(benches, bench_critical_function);
    criterion_main!(benches);
}
```

### Memory Testing
Monitor memory usage in long-running tests:

```rust
#[tokio::test]
async fn test_memory_usage() {
    let initial_memory = get_memory_usage();

    // Run memory-intensive operation
    run_operation().await;

    let final_memory = get_memory_usage();
    assert!(final_memory - initial_memory < MEMORY_THRESHOLD);
}
```

## Test Data Management

### Test Fixtures
- **Location**: `riptide-core/src/*/test_fixtures.rs`
- **Purpose**: Reusable test data and mock objects
- **Usage**: Shared across multiple test files

### Temporary Files
Always use `tempfile` crate for test file creation:

```rust
use tempfile::tempdir;

#[test]
fn test_with_temp_files() {
    let temp_dir = tempdir().unwrap();
    let file_path = temp_dir.path().join("test_file.dat");
    // Test operations...
    // Cleanup automatic when temp_dir drops
}
```

## Continuous Improvement

### Quality Metrics to Monitor
- Test execution time (should remain under 10 minutes total)
- Test coverage percentage (aim for >90% on critical paths)
- Flaky test rate (should be <1%)
- Integration test pass rate in CI (should be 100%)

### Future Enhancements
- [ ] Re-enable E2E tests after fixing import issues
- [ ] Add property-based testing for critical algorithms
- [ ] Implement mutation testing for test quality validation
- [ ] Add performance regression detection in CI
- [ ] Create visual test reports and coverage dashboards

---

**Remember**: Tests are documentation. Write them to be clear, maintainable, and valuable for future developers working on the codebase.
