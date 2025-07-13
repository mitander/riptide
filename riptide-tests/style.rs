//! Style Enforcement Tests
//!
//! Validates that the codebase follows the style guidelines defined in docs/STYLE.md.
//! These tests enforce critical patterns that cannot be easily caught by clippy alone.
//!
//! # Test Organization
//!
//! - `naming_conventions` - Enforces naming patterns for types, functions, and modules
//! - `dead_code_enforcement` - Prevents #[allow(dead_code)] in production code
//!
//! These tests scan the entire workspace and will fail if violations are found.
//! They are designed to maintain code quality and consistency across the project.

#[path = "style/naming_conventions.rs"]
mod naming_conventions;

#[path = "style/dead_code_enforcement.rs"]
mod dead_code_enforcement;
