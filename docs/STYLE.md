# Riptide Style Guide

## Core Principles

1.  **Working > Clever:** Ship code that works.
2.  **Measured > Assumed:** Benchmark before refactor.
3.  **Simple > Flexible:** YAGNI. Solve today's problem, not tomorrow's.
4.  **Explicit > Magic:** The code path must be obvious. Focus on simplicity and consistency.

## Naming (ZERO TOLERANCE)

### Functions & Methods

Naming must describe the action and intent.

| Pattern | Usage | Example |
| :--- | :--- | :--- |
| `verb_noun()` | Standard action | `download_piece()`, `parse_range_header()` |
| `is_...` or `has_...` | State query | `is_complete()` (returns `bool`) |
| `as_...` | Cheap borrow | `as_bytes()` (`&self -> &T`) |
| `into_...` | Consuming conversion | `into_stream()` (`self -> T`) |
| `try_...` | Fallible operation | `try_connect()` (returns `Result`) |

**Banned Patterns (Will be rejected):**

| Forbidden | Reason / Correction |
| :--- | :--- |
| `get_...()` | No. Use the noun directly: `peer.id()` not `peer.get_id()`. |
| `set_...()` | No. Use verbs describing the state change: `peer.update_speed()` not `peer.set_speed()`. |
| `handle_...()` | No. Too vague. Describe what you're doing: `process_incoming_message()`. |

### Types (Structs, Enums)

Suffixes are mandatory.

| Category | Suffix | Example |
| :--- | :--- | :--- |
| Errors | `...Error` | `DownloadError`, `StorageError` |
| Configuration | `...Config` | `RiptideConfig`, `NetworkConfig` |
| Handles | `...Handle` | `TorrentEngineHandle` |

**Banned Suffixes:**

| Forbidden | Reason / Correction |
| :--- | :--- |
| `...Manager` | Meaningless. `PieceManager` should be `PieceDownloader` or similar. |
| `...Service` | Lazy naming. `HttpService` should be `HttpServer` or `ApiRouter`. |
| `...Factory` | No. Use idiomatic Rust `Builder` pattern or a simple `new()` function. |

## Code Structure

-   **Functions:** 70 lines max. If it's longer, your design is wrong. Break it down.
-   **Modules:** 500 lines max. A larger module is a failure of abstraction. Split it.

## Error Handling

-   **No Panics in Production Code:** Enforced by CI.
-   Use `debug_assert!` for invariants, not for validating fallible operations.
-   **Specific Errors:** Use `thiserror` to create error types with context. No generic "failed" errors.

    ```rust
    // GOOD
    #[derive(Debug, thiserror::Error)]
    pub enum DownloadError {
        #[error("piece {index} hash mismatch")]
        HashMismatch { index: u32 },
    }

    // BAD
    #[error("download failed")]
    Generic,
    ```

-   **No `.unwrap()` or `.expect()`:** Except in tests or with a `// SAFETY:` comment explaining why it can't fail.

## Async Patterns

-   **Timeout All Network Operations:** A `select!` against `tokio::time::sleep` is the standard pattern. Untimed network calls are a bug.
-   **Graceful Shutdown:** All long-running tasks must listen for a shutdown signal. Orphaned tasks are a bug.
-   **No `std::thread::sleep` in `async` code.** Use `tokio::time::sleep`.
-   **No Unbounded Channels.** `mpsc::unbounded_channel` is a memory leak waiting to happen. Use bounded channels and handle backpressure.

## Documentation

-   **Why, Not What:** `/// Validates piece hash against torrent metadata.` is bad. `/// BitTorrent uses SHA-1 for piece integrity. This function confirms a downloaded piece is not corrupt.` is good.
-   **Mandatory Sections:**
    -   `# Errors` for functions returning `Result`.
    -   `# Panics` for functions that can panic.
-   **Compile-checked Examples:** `#[test]`s are better, but doc examples must compile.
