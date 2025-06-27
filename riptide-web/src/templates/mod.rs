//! Template rendering engine for the Riptide web UI
//!
//! Provides HTML template rendering with JSON context injection for the web UI.
//! Uses a simple template system optimized for streaming media server interfaces.

pub mod engine;

// Re-export public API
pub use engine::TemplateEngine;
