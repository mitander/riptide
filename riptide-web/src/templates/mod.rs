//! Template rendering engine for the Riptide web UI
//!
//! Provides HTML template rendering with JSON context injection for the web UI.
//! Uses actual HTML template files located in riptide-web/templates/ directory.
//! This approach enables proper syntax highlighting, linting, and hot-reloading.

mod rendering;
mod types;

// Re-export public API
pub use types::TemplateEngine;
