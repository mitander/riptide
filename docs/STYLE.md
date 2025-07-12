# Riptide Style Guide

This document defines the coding standards for the Riptide project. All code must conform to these rules, which are enforced by built-in Rust tooling: `rustfmt`, `clippy`, and style enforcement tests.

## 1. Guiding Principles

1.  **Correctness > Performance > Features.** A clever optimization is worthless if it introduces subtle bugs.
2.  **Clarity at the Point of Use.** Code must be immediately understandable without tracing its implementation. No magic.
3.  **Deep Modules, Simple Interfaces.** Complexity is to be hidden within a module, not exposed through its public API. (See: "A Philosophy of Software Design").
4.  **Static over Dynamic.** Prefer static dispatch. Use `dyn Trait` only at major architectural boundaries for decoupling (e.g., swapping production vs. simulation components).
5.  **Measure, Don't Guess.** Performance optimizations must be backed by benchmarks.

## 2. Naming Conventions

### 2.1. Types (Structs, Enums, Traits)

Name what the type *is*, not its architectural pattern or role. Role-based suffixes like `Service`, `Manager`, or `Factory` are banned because they are verbose and add no semantic value.

| Correct           | Banned                       | Rationale                                   |
| :---------------- | :--------------------------- | :------------------------------------------ |
| `HttpStreaming`   | `HttpStreamingService`       | Redundant. The module context is sufficient.  |
| `FileLibrary`     | `FileLibraryManager`         | It *is* a library, not a manager *for* one.   |
| `new()`           | `PeerFactory::create()`      | `new()` is idiomatic. The Factory pattern is banned. |
| `TorrentEngine`   | `TorrentEngineController`    | Use direct, concrete names.                   |
| `Remuxer`         | `RemuxProcessor` or `RemuxHandler` | The name should describe the entity's essence. |

Standard suffixes for common patterns are required:

-   **Errors:** `*Error` (e.g., `TorrentError`)
-   **Configuration:** `*Config` (e.g., `RiptideConfig`)
-   **Handles:** `*Handle` (e.g., `TorrentEngineHandle`)
-   **Builders:** `*Builder` (e.g., `ConfigBuilder`)

### 2.2. Functions & Methods

Function names must be verbs that clearly state their action and effect.

| Convention                 | Example                           | Description                                     |
| :------------------------- | :-------------------------------- | :---------------------------------------------- |
| **Actions**                | `connect_peer()`, `parse_header()`  | `verb_noun` format. Clear and direct.           |
| **State Checkers**         | `is_complete()`, `has_piece()`    | Prefixed with `is_` or `has_`. Must return `bool`.|
| **Fallible Operations**    | `try_create_session()`            | Prefix with `try_`. Must return a `Result`.       |
| **Optional Getters**       | `output_path()`                   | Noun only. Must return `Option`.                  |
| **Simple Getters**         | `peer_id()`                       | Noun only. For fields that are always present.  |
| **Conversions (Borrow)**   | `as_bytes()`                      | Prefix with `as_`. Returns a reference.         |
| **Conversions (Consuming)**| `into_inner()`                    | Prefix with `into_`. Consumes `self`.           |

**Forbidden Prefixes:**

-   **`get_`**: REJECT. Use the noun directly. (`peer.id()` not `peer.get_id()`).
-   **`set_`**: REJECT. Use a descriptive verb. (`peer.update_speed(new_speed)` not `peer.set_speed(speed)`).
-   **`handle_`**: REJECT. Be specific. (`process_request()` not `handle_request()`).

### 2.3. Modules

Module names should describe their contents. Generic, meaningless names are banned.

| Correct          | Banned                      | Rationale                                   |
| :--------------- | :-------------------------- | :------------------------------------------ |
| `piece_picker`   | `utils`, `helpers`          | Describes the specific responsibility.        |
| `protocol`, `types` | `common`, `misc`, `stuff` | Use clear, domain-specific names.           |

## 3. Commit Message Convention

All commits **must** adhere to the [Conventional Commits](https://www.conventionalcommits.org/) specification. This is enforced by a git hook and enables automated changelogs and a clear, parsable project history.

**Format:** `<type>(scope)!: <description>`

-   **Types:** `feat`, `fix`, `docs`, `style`, `refactor`, `test`, `chore`, `build`, `ci`, `perf`, `revert`.
-   **Description:** Must start with a letter or digit and be 1-100 characters long.
-   **Breaking Changes:** Use `!` for breaking changes: `refactor(core)!: rename PeerManager trait`.

---

**Example: Single-line Commit**
```
docs: fix missing documentation warnings
```
---

**Example: Multi-line Commit with Body**
```
refactor(sim): unify peer manager implementations

- Consolidate DevPeerManager and SimPeerManager into organized peer_manager module.
- Create peer_manager/deterministic.rs for controlled simulation.
- Create peer_manager/development.rs for rapid iteration.
- Move piece store implementation to peer_manager/piece_store.rs.
- Update simulation_mode.rs to use the new unified peer manager structure.
- Remove redundant dev_peer_manager.rs and sim_peer_manager.rs files.

This change creates a cleaner, more maintainable peer management architecture with better separation between testing modes.
```

## 4. Documentation

All `pub` items **must** have complete documentation. This is enforced by `cargo clippy`. Non-compliance is a build failure.

-   **Summary:** Start with a concise one-line summary.
-   **Detail:** Follow with a paragraph explaining purpose, behavior, and any non-obvious details.
-   **`# Errors`**: Required for any function returning a `Result`. **Always** use a bulleted list.
-   **`# Panics`**: Required for any function that can panic. Use a sentence for a single condition; use a list for multiple.

**Golden Standard:**
```rust
/// Handles an HTTP streaming request.
///
/// This is the primary entry point for media streaming. It parses the `Range`
/// header, determines the required data from the underlying `DataSource`,
/// and constructs an appropriate `206 Partial Content` or `200 OK` response.
///
/// # Errors
///
/// - `StreamingError::DataSource` - If reading from the `DataSource` fails.
/// - `StreamingError::InvalidRange` - If the requested byte range is invalid.
///
/// # Panics
///
/// Panics if the `Content-Length` header cannot be constructed from the
/// response size, which should never happen for valid data.
pub async fn process_http_request(
    // ...
) -> Result<Response, StreamingError> {
    // ...
}
```

## 5. Error Handling

-   **Libraries (`riptide-core`, `riptide-sim`):** Use `thiserror` to create specific, well-defined error enums.
-   **Applications (`riptide-cli`):** Use `anyhow` for simple, context-rich error handling.
-   **Zero Tolerance for `.unwrap()` or `.expect()`:** These are forbidden outside of tests and will fail CI. The only exception is a "checked" `unwrap()` with a `// SAFETY:` comment explaining why it cannot fail.

## 6. Testing

-   **Deterministic Simulation:** Inspired by TigerBeetle, we use deterministic simulation for robust and reproducible testing of complex concurrency scenarios.
-   **Coverage:** All new features and bug fixes must be accompanied by tests. This is a non-negotiable part of a "Done" feature.
-   **Test Types:**
    -   **Unit Tests:** Located in the same file as the code they test.
    -   **Integration Tests:** In the `riptide-tests/integration` directory. Verify interactions between modules.
    -   **E2E Tests:** In the `riptide-tests/e2e` directory. Verify complete user workflows.
