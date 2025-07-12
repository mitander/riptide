# Riptide Style Guide

This document defines the coding standards for the Riptide project. All code must conform to these rules, which are enforced by built-in Rust tooling: `rustfmt`, `clippy`, and compiler lints.

## Core Principles

1.  **Working > Clever:** Ship code that works.
2.  **Measured > Assumed:** Benchmark before refactor.
3.  **Simple > Flexible:** YAGNI. Solve today's problem, not tomorrow's.
4.  **Explicit > Magic:** The code path must be obvious. Focus on simplicity and consistency.

## Enforcement Strategy

We use **standard Rust tooling** instead of custom linters:

- **Compiler lints:** `#![deny(missing_docs)]` in all `lib.rs` files
- **Clippy lints:** `#![deny(clippy::missing_errors_doc, clippy::missing_panics_doc)]`
- **Configuration:** `clippy.toml` for project-specific thresholds
- **CI Integration:** All lints must pass for code to merge

### Lint Configuration

Each crate's `lib.rs` enforces documentation standards:

```rust
//! Crate documentation

#![deny(missing_docs)]
#![deny(clippy::missing_errors_doc)]
#![deny(clippy::missing_panics_doc)]
#![warn(clippy::too_many_lines)]
```

The `clippy.toml` file configures project-wide rules:

```toml
# Size limits: Functions 70 lines, Modules 500 lines
too-many-lines-threshold = 500
cognitive-complexity-threshold = 15

# Safety: No unwrap/panic in production
unwrap-used = true
expect-used = true
panic = true
```

## Naming Conventions (ZERO TOLERANCE)

### Functions & Methods

Naming must describe the action and intent clearly.

| Pattern               | Usage                | Example                                    |
| :-------------------- | :------------------- | :----------------------------------------- |
| `verb_noun()`         | Standard action      | `download_piece()`, `parse_range_header()` |
| `is_...` or `has_...` | State query          | `is_complete()` (returns `bool`)           |
| `as_...`              | Cheap borrow         | `as_bytes()` (`&self -> &T`)               |
| `into_...`            | Consuming conversion | `into_stream()` (`self -> T`)              |
| `try_...`             | Fallible operation   | `try_connect()` (returns `Result`)         |

**Banned Patterns (Enforced by code review):**

| Forbidden      | Reason / Correction                                                                      |
| :------------- | :--------------------------------------------------------------------------------------- |
| `get_...()`    | No. Use the noun directly: `peer.id()` not `peer.get_id()`.                              |
| `set_...()`    | No. Use verbs describing the state change: `peer.update_speed()` not `peer.set_speed()`. |
| `handle_...()` | No. Too vague. Describe what you're doing: `process_incoming_message()`.                 |

### Types (Structs, Enums, Traits)

**Principle: "Clear at the point of use" trumps "shorter names"**

| Category      | Suffix       | Example                                    |
| :------------ | :----------- | :----------------------------------------- |
| Errors        | `...Error`   | `DownloadError`, `StorageError`            |
| Configuration | `...Config`  | `RiptideConfig`, `NetworkConfig`           |
| Handles       | `...Handle`  | `TorrentEngineHandle`, `FileLibraryHandle` |
| Builders      | `...Builder` | `ConfigBuilder`, `RequestBuilder`          |

**Naming Guidelines:**

**Good simplifications:**
| Instead of | Use | Reason |
| :---------------------- | :----------------------------------------------- | :------------------- |
| `HttpStreamingService` | `HttpStreaming` | Service adds no value |
| `MediaSearchService` | `MediaSearch` | Service adds no value |
| `FileLibraryManager` | `FileLibrary` | Clear what it is |
| `TrackerClientFactory` | `TrackerClient::new()` or `TrackerClientBuilder` | Factory pattern better |

**Keep descriptive names when they add clarity:**
| Descriptive Name | Why Keep It |
| :------------------ | :------------------------------------ |
| `PeerManager` | Trait that manages peers - clear role |
| `TrackerManager` | Trait that manages trackers - clear role |
| `TorrentEngineHandle` | Distinguishes from internal engine |

**Banned Suffixes:**

| Forbidden    | Reason                                           |
| :----------- | :----------------------------------------------- |
| `...Factory` | Use `Builder` pattern or simple `new()` function |
| `...Service` | Usually adds no semantic value                   |

## Documentation Standards

**All public items MUST have complete documentation.** This is enforced by `#![deny(missing_docs)]`.

### Required Documentation Sections

The compiler and Clippy enforce these sections:

- **All `pub` items:** Functions, structs, enums, traits, modules
- **`# Errors`** for functions returning `Result` (enforced by `clippy::missing_errors_doc`)
- **`# Panics`** for functions that can panic (enforced by `clippy::missing_panics_doc`)

### Documentation Format

Use the standard Rust documentation format. Focus on **why** and **what**, not **how**.

**Good Example (Passes all lints):**

```rust
/// Simulated tracker client for deterministic testing and development.
///
/// Provides controllable tracker responses without network communication.
/// Maintains internal state for realistic swarm simulation and supports
/// injecting specific responses for edge case testing.
pub struct SimulatedTrackerClient {
    // fields
}

impl SimulatedTrackerClient {
    /// Creates new simulated tracker client with default configuration.
    ///
    /// Uses realistic default values for interval, peer count, and swarm statistics.
    /// Peer addresses are generated deterministically for reproducible testing.
    pub fn new(announce_url: String) -> Self {
        // implementation
    }

    /// Simulates tracker announce with deterministic peer list generation.
    ///
    /// Returns realistic peer lists and swarm statistics without network communication.
    /// Supports failure injection and configurable response characteristics.
    ///
    /// # Errors
    /// - `TorrentError::TrackerConnectionFailed` - When configured for failure simulation
    pub async fn announce(&self, request: AnnounceRequest) -> Result<AnnounceResponse, TorrentError> {
        // implementation
    }

    /// Injects predefined swarm statistics for specific info hash.
    ///
    /// Useful for testing scenarios with specific seeder/leecher ratios
    /// or simulating swarm evolution over time.
    ///
    /// # Panics
    /// Panics if the swarm stats mutex is poisoned.
    pub fn configure_swarm_stats(&self, info_hash: InfoHash, complete: u32, incomplete: u32, downloaded: u32) {
        let mut stats = self.swarm_stats.lock().unwrap(); // This can panic
        // implementation
    }
}
```

**Bad Examples (Will fail lints):**

```rust
// FAIL: missing_docs - No documentation
pub fn download_piece(index: u32) -> Result<Vec<u8>, DownloadError> {

// FAIL: clippy::missing_errors_doc - Missing # Errors section
/// Downloads a piece from peers.
pub fn download_piece(index: u32) -> Result<Vec<u8>, DownloadError> {

// FAIL: clippy::missing_panics_doc - Missing # Panics section
/// Gets the piece data.
pub fn get_piece(&self, index: u32) -> &[u8] {
    &self.pieces[index as usize] // This can panic on out-of-bounds
}

// FAIL: Describes implementation, not purpose
/// Uses SHA-1 algorithm to hash the piece data and compare with expected.
pub fn validate_piece_integrity(piece: &[u8]) -> Result<(), ValidationError>
```

## Code Structure

- **Functions:** 70 lines max (enforced by `clippy::too_many_lines`)
- **Modules:** 500 lines max (configured in `clippy.toml`)
- **Complexity:** Keep cognitive complexity under 15 (configured in `clippy.toml`)

## Error Handling

- **No Panics in Production Code:** Enforced by `#![deny(clippy::panic, clippy::unwrap_used, clippy::expect_used)]`
- Use `debug_assert!` for invariants, not for validating fallible operations
- **Specific Errors:** Use `thiserror` to create error types with context

  ```rust
  // GOOD
  #[derive(Debug, thiserror::Error)]
  pub enum DownloadError {
      #[error("piece {index} hash mismatch")]
      HashMismatch { index: u32 },
  }

  // BAD
  #[derive(Debug, thiserror::Error)]
  pub enum DownloadError {
      #[error("download failed")]
      Generic,
  }
  ```

- **Exception for Tests:** `.unwrap()` and `.expect()` are allowed in test code and with `// SAFETY:` comments:

  ```rust
  // Allowed in production with safety comment
  if option.is_none() { return; }
  // SAFETY: The `is_none` check above ensures this unwrap can never fail.
  let value = option.unwrap();
  ```

## Async Patterns

- **Timeout All Network Operations:** Enforced by `clippy::await_holding_lock`
- **Graceful Shutdown:** All long-running tasks must listen for shutdown signals
- **No `std::thread::sleep` in `async` code:** Use `tokio::time::sleep`
- **No Unbounded Channels:** Use bounded channels and handle backpressure

## Commit Message Convention

All commits must follow the Conventional Commits specification. The git hook validates this format:

**Format:** `<type>(scope): <description>`

**Valid Types:**

- `feat`: New feature
- `fix`: Bug fix
- `docs`: Documentation only changes
- `style`: Code style changes (formatting, missing semicolons, etc.)
- `refactor`: Code refactoring without changing functionality
- `test`: Adding or modifying tests
- `chore`: Maintenance tasks, dependency updates
- `build`: Build system or external dependencies
- `ci`: CI configuration changes
- `perf`: Performance improvements
- `revert`: Reverting previous commits
- `wip`: Work in progress (use sparingly)

**Rules:**

- Description must start with a letter or digit
- Description must be 1-100 characters long
- Optional scope in parentheses: `feat(core): add piece validation`
- Breaking changes: add `!` after scope: `feat(api)!: remove deprecated endpoint`

**Examples:**

```
feat(streaming): implement background transcoding pipeline
fix(core): correct piece hash validation algorithm
docs(api): add streaming endpoint documentation
refactor(sim): unify peer manager implementations
test(engine): add comprehensive torrent lifecycle tests
chore(deps): update tokio to 1.35
```

## Quality Gates

All code must pass these automated checks:

1. **Format:** `cargo fmt --all -- --check`
2. **Lints:** `cargo clippy --workspace --all-targets -- -D warnings`
3. **Tests:** `cargo test --workspace`
4. **Docs:** `cargo doc --workspace --no-deps`

These are enforced by:

- **Pre-commit hooks** (local development)
- **CI pipeline** (all pull requests)
- **Git hooks** (commit message format)

## Development Workflow

```bash
# Before committing, ensure all checks pass:
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo doc --workspace --no-deps

# Or use the test runner:
./scripts/test_runner.sh all
```

## Anti-Patterns to Reject

| The Violation (REJECT)          | The Riptide Way (ACCEPT)                   |
| :------------------------------ | :----------------------------------------- |
| `get_peer_id()`                 | `peer_id()`                                |
| `set_speed()`                   | `update_speed(new_speed)`                  |
| `utils.rs`, `common.rs`         | `piece_picker.rs`, `protocol.rs`           |
| `HttpStreamingService`          | `HttpStreaming`                            |
| `MediaSearchService`            | `MediaSearch`                              |
| `FileLibraryManager`            | `FileLibrary`                              |
| `TorrentEngine` (two meanings)  | `TorrentEngine` + `TorrentEngineHandle`    |
| `Peers` trait (unclear)         | `PeerManager` trait (clear role)           |
| `.unwrap()` / `.expect()`       | `?` operator or `match`                    |
| `println!` for debugging        | `tracing::debug!`, `tracing::info!`        |
| Monolithic functions            | Small, single-responsibility functions     |
| Generic `reason: String` errors | Specific `thiserror` enums                 |
| Over-engineering with generics  | Concrete types until abstraction is needed |

## Philosophy

Build the simplest thing that streams movies reliably. Use standard Rust tooling to enforce quality. Measure everything. Optimize based on real usage data, not assumptions.
