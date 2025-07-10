### **Riptide Style Guide**
---

## **1. Guiding Principles**

1.  **Clarity Over Brevity:** Code is read more often than it is written. Names must be explicit and unambiguous.
2.  **Idiomatic Rust First:** We adhere to the official [Rust API Guidelines](https://rust-lang.github.io/api-guidelines/introduction.html). These conventions supplement, not replace, them.
3.  **Explain the "Why," Not the "What":** Assume the reader understands Rust. Comments must explain the rationale behind a design decision or the invariants a piece of code upholds.
4.  **Style is Not a Suggestion:** All conventions are enforced by automated checks. A violation is a build failure.

## **2. Naming Conventions**

#### **2.1. Types (Structs, Enums, Traits)**

*   Use `UpperCamelCase`.
*   **Use descriptive suffixes:**
    *   Error enums: Must end in `Error` (e.g., `TorrentError`, `StorageError`).
    *   Configuration structs: Must end in `Config` (e.g., `RiptideConfig`, `RemuxStreamingConfig`).
    *   Builder structs: Must end in `Builder` (e.g., `MockMagnetoProviderBuilder`).
    *   Handles to actors/services: Must end in `Handle` (e.g., `TorrentEngineHandle`).

#### **2.2. Functions & Methods**

*   Use `snake_case`.
*   **Functions are verbs.** The name must describe what the function *does*.
*   **`get_` and `set_` prefixes are forbidden.** This is an un-idiomatic OOP pattern.
    *   **BAD:** `get_peer_id()`, `set_download_speed()`, `get_mut()`
    *   **GOOD:** `peer_id()`, `update_download_speed()`, `as_mut()`
    *   **NO EXCEPTIONS:** Use idiomatic Rust conversions (`as_*`, `to_*`, `into_*`) instead.
*   **Predicates** (methods returning `bool`) should start with `is_`, `has_`, or `can_`.
    *   `is_complete()`, `has_piece()`, `can_request_pieces()`
*   **Conversions** follow Rust's standard: `as_*` (cheap), `to_*` (expensive), `into_*` (consuming).

#### **2.3. Modules & Crates**

*   Use `snake_case` for module file names (e.g., `piece_picker.rs`).
*   **No "Garbage Can" Modules:** Module names like `util`, `utils`, `helper`, `common`, or `core` are **strictly forbidden**. Every module must have a clear, descriptive domain noun as its name.
    *   **GOOD:** `riptide-core/src/torrent/protocol`, `riptide-web/src/streaming`
    *   **BAD:** `riptide-core/src/common`

## **3. Code Style & Implementation**

*   **Error Handling:** All fallible functions must return a `Result`. `unwrap()` and `expect()` are **forbidden** in application logic and will be caught by `clippy`. They are only permitted in tests with explicit justification.
*   **Documentation (`rustdoc`):**
    *   Every public item (`pub fn`, `pub struct`, etc.) **must** be documented.
    *   Documentation explains the **purpose and rationale ("why")**, not the implementation details ("what").
    *   Functions returning `Result` **must** have an `# Errors` section detailing failure modes.
    *   Functions that can `panic` (e.g., broken invariants) **must** have a `# Panics` section.

## **4. Commit Message Style (Enforced)**

We follow the [**Conventional Commits**](https://www.conventionalcommits.org/en/v1.0.0/) specification. This is not optional and is enforced by the `commit-msg` git hook.

#### **Format:**
```
<type>(optional scope): <description>

[optional body]

[optional footer(s)]
```

#### **Types:**

*   `feat`: A new feature for the user.
*   `fix`: A bug fix for the user.
*   `chore`: Routine tasks, dependency updates, and other non-user-facing changes.
*   `docs`: Documentation changes only.
*   `style`: Formatting, missing semicolons, etc.; no production code change.
*   `refactor`: Refactoring production code, e.g., renaming a variable.
*   `test`: Adding missing tests, refactoring tests; no production code change.
*   `build`: Changes to the build system (`ci.yml`, `Cargo.toml`).

#### **Scope:**

The scope should be the crate or module affected.
*   **GOOD:** `fix(streaming)`, `feat(web)`, `refactor(core::piece_picker)`

#### **Description:**

*   Written in the **imperative mood** ("add feature" not "adds feature").
*   Begins with a lowercase letter.
*   No period at the end.
*   **Must be between 1 and 100 characters.**

#### **Examples:**

```
feat(streaming): implement readiness API with UI integration

- Add /stream/{hash}/ready endpoint to check stream availability
- Frontend polls readiness before attempting playbook
- Show loading spinner until stream is ready for playback
- Prevents premature video element initialization
```

```
fix(streaming): replace fragmented MP4 with faststart for browser compatibility

- Replace fragmented MP4 flags with +faststart for seekable MP4 generation
- Switch from pipe output to temporary file output
- Clean up temporary files after processing
- Resolves Firefox metadata parsing errors and black screen issues
```
