//! Riptide Style Consistency Enforcement
//!
//! Automatically detects style inconsistencies based on docs/STYLE.md and
//! docs/NAMING_CONVENTIONS.md. Focuses on consistency within the codebase
//! rather than arbitrary rules.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

// --- Configuration from Style Docs ---

/// Module size limit from docs/STYLE.md
const MAX_MODULE_LINES: usize = 500;

/// Banned module names from docs/STYLE.md - anti-pattern modules
const BANNED_MODULE_NAMES: &[&str] = &[
    "utils", "util", "helpers", "helper", "common", "shared", "misc", "tools",
];

/// Functions that lose critical context when imported (preserve inline usage)
const CONTEXT_CRITICAL_FUNCTIONS: &[&str] = &[
    // Async context (critical for debugging)
    "spawn",
    "sleep",
    "timeout",
    "select",
    "join",
    // I/O context
    "read",
    "write",
    "seek",
    "flush",
    "close",
    // Network context
    "connect",
    "bind",
    "listen",
    "accept",
    "send",
    "recv",
    // Serialization/Parsing context
    "parse",
    "serialize",
    "deserialize",
    "to_string",
    "from_str",
    // Logging context (critical for debugging)
    "debug",
    "info",
    "warn",
    "error",
    "trace",
    "log",
];

/// Types that should always preserve context (traits, errors, etc.)
const PRESERVE_CONTEXT_PATTERNS: &[&str] = &[
    "std::fmt::Display",
    "std::fmt::Debug",
    "std::str::FromStr",
    "std::convert::", // All conversion traits
    "std::clone::",   // Clone trait
    "std::marker::",  // Send, Sync traits
    "std::ops::",     // All operator traits
    "std::cmp::",     // All comparison traits
];

/// Patterns that lose context when imported (suggest keeping inline)
const KEEP_INLINE_PATTERNS: &[&str] = &[
    "tokio::",
    "serde_json::",
    "serde_yaml::",
    "reqwest::",
    "riptide_core::",
    "riptide_web::",
    "riptide_search::",
    "riptide_sim::",
];

/// Common utility types that should be imported when used frequently
const IMPORT_WHEN_FREQUENT: &[&str] = &[
    "std::collections::HashMap",
    "std::collections::BTreeMap",
    "std::collections::HashSet",
    "std::sync::Arc",
    "std::sync::Mutex",
    "std::time::Duration",
    "std::time::Instant",
    "std::io::SeekFrom",
    "std::net::SocketAddr",
    "std::net::IpAddr",
];

#[derive(Debug)]
enum Severity {
    Critical,
    Warning,
}

#[derive(Debug)]
struct StyleViolation {
    severity: Severity,
    file_path: PathBuf,
    line_number: usize,
    rule: String,
    message: String,
}

impl StyleViolation {
    fn new(
        severity: Severity,
        file_path: &Path,
        line_number: usize,
        rule: &str,
        message: &str,
    ) -> Self {
        Self {
            severity,
            file_path: file_path.to_path_buf(),
            line_number,
            rule: rule.to_string(),
            message: message.to_string(),
        }
    }
}

/// Main style checker
struct StyleChecker {
    violations: Vec<StyleViolation>,
    current_file: PathBuf,
    file_lines: Vec<String>,
}

impl StyleChecker {
    fn new() -> Self {
        Self {
            violations: Vec::new(),
            current_file: PathBuf::new(),
            file_lines: Vec::new(),
        }
    }

    fn check_file(&mut self, file_path: PathBuf) -> Result<(), std::io::Error> {
        let content = fs::read_to_string(&file_path)?;
        self.current_file = file_path;
        self.file_lines = content.lines().map(|s| s.to_string()).collect();

        // Core structural checks (always run)
        self.check_module_size();
        self.check_banned_module_names();
        self.check_inline_module_references();

        Ok(())
    }

    fn add_violation(&mut self, severity: Severity, line: usize, rule: &str, message: &str) {
        self.violations.push(StyleViolation::new(
            severity,
            &self.current_file,
            line,
            rule,
            message,
        ));
    }

    /// Check module doesn't exceed size limits
    fn check_module_size(&mut self) {
        let line_count = self.file_lines.len();
        if line_count > MAX_MODULE_LINES {
            self.add_violation(
                Severity::Warning,
                1,
                "MODULE_SIZE_LIMIT",
                &format!(
                    "Module has {} lines, exceeding {} line limit from docs/STYLE.md",
                    line_count, MAX_MODULE_LINES
                ),
            );
        }
    }

    /// Check for banned module names
    fn check_banned_module_names(&mut self) {
        if let Some(file_name) = self.current_file.file_stem() {
            if let Some(name_str) = file_name.to_str() {
                if BANNED_MODULE_NAMES.contains(&name_str) {
                    self.add_violation(
                        Severity::Critical,
                        1,
                        "BANNED_MODULE_NAME",
                        &format!(
                            "Module name '{}' violates anti-pattern rules from docs/STYLE.md",
                            name_str
                        ),
                    );
                }
            }
        }
    }

    /// Smart inline module reference checking
    fn check_inline_module_references(&mut self) {
        // First pass: count all module usage patterns
        let usage_counts = self.count_module_usage();

        // Collect violations first
        let mut violations = Vec::new();

        // Second pass: check each usage for violations
        for (line_num, line) in self.file_lines.iter().enumerate() {
            self.collect_line_violations(line_num + 1, line, &usage_counts, &mut violations);
        }

        // Add all violations
        for (line_num, rule, message) in violations {
            self.add_violation(Severity::Critical, line_num, &rule, &message);
        }
    }

    /// Count how many times each full module path is used
    fn count_module_usage(&self) -> HashMap<String, usize> {
        let mut counts = HashMap::new();
        let pattern =
            regex::Regex::new(r"\b([a-zA-Z_][a-zA-Z0-9_]*(?:::[a-zA-Z_][a-zA-Z0-9_]*)*)::")
                .expect("Invalid regex pattern");

        for line in &self.file_lines {
            // Skip comments and use statements
            let trimmed = line.trim();
            if trimmed.starts_with("//") || trimmed.starts_with("use ") {
                continue;
            }

            for capture in pattern.find_iter(line) {
                let full_path = capture.as_str().trim_end_matches("::");
                *counts.entry(full_path.to_string()).or_insert(0) += 1;
            }
        }

        counts
    }

    /// Collect violations from a single line for inline module references
    fn collect_line_violations(
        &self,
        line_num: usize,
        line: &str,
        usage_counts: &HashMap<String, usize>,
        violations: &mut Vec<(usize, String, String)>,
    ) {
        let pattern =
            regex::Regex::new(r"\b([a-zA-Z_][a-zA-Z0-9_]*(?:::[a-zA-Z_][a-zA-Z0-9_]*)*)::")
                .expect("Invalid regex pattern");

        for capture in pattern.find_iter(line) {
            let full_path = capture.as_str().trim_end_matches("::");
            let usage_count = usage_counts.get(full_path).unwrap_or(&0);

            // Skip if context should always be preserved
            if self.should_preserve_context(line, capture.start()) {
                continue;
            }

            // Check if this pattern should be imported when used frequently
            if self.should_import_when_frequent(full_path, *usage_count) {
                let type_name = self.extract_type_name(line, capture.start());
                violations.push((
                    line_num,
                    "INLINE_MODULE_REFERENCE".to_string(),
                    format!(
                        "Import '{}' at top of file - used {} times (Used frequently)",
                        type_name, usage_count
                    ),
                ));
            }
        }
    }

    /// Check if context should be preserved for this usage
    fn should_preserve_context(&self, line: &str, start_pos: usize) -> bool {
        let context_len = 50;
        let start = start_pos.saturating_sub(context_len / 2);
        let end = std::cmp::min(start_pos + context_len, line.len());
        let context = &line[start..end];

        // Skip string literals
        if context.contains('"') || context.contains('\'') {
            return true;
        }

        // Check for context-critical function calls
        for &func in CONTEXT_CRITICAL_FUNCTIONS {
            if context.contains(&format!("::{}", func)) {
                return true;
            }
        }

        // Check for patterns that should preserve context
        for &pattern in PRESERVE_CONTEXT_PATTERNS {
            if context.contains(pattern) {
                return true;
            }
        }

        // Check for error types (wildcard matching)
        if context.contains("Error") || context.contains("ErrorKind") {
            return true;
        }

        // Check for patterns that lose semantic value when imported
        for &pattern in KEEP_INLINE_PATTERNS {
            if context.starts_with(pattern) {
                return true;
            }
        }

        false
    }

    /// Check if this module path should be imported when used frequently
    fn should_import_when_frequent(&self, full_path: &str, usage_count: usize) -> bool {
        // Check if it's a pattern we care about
        let is_import_candidate = IMPORT_WHEN_FREQUENT
            .iter()
            .any(|&pattern| full_path.starts_with(pattern));

        if !is_import_candidate {
            return false;
        }

        // Different thresholds for different module types
        let threshold = if full_path.starts_with("std::") {
            2 // Lower threshold for std types
        } else {
            5 // Higher threshold for other modules
        };

        usage_count >= threshold
    }

    /// Extract the specific type or function name being referenced
    fn extract_type_name(&self, line: &str, start_pos: usize) -> String {
        let remaining = &line[start_pos..];

        // Find the end of the reference (before parentheses, spaces, etc.)
        let delimiters = ['(', '<', ' ', '>', ')', ',', ';', '{', '}', '[', ']'];
        let mut end_pos = remaining.len();

        for &delimiter in &delimiters {
            if let Some(pos) = remaining.find(delimiter) {
                end_pos = std::cmp::min(end_pos, pos);
            }
        }

        remaining[..end_pos].to_string()
    }

    /// Run all checks on the workspace
    fn check_workspace(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        // Navigate to workspace root (parent of tests directory)
        let workspace_root = Path::new("..");
        self.check_directory(workspace_root)?;
        Ok(())
    }

    /// Recursively check all Rust files in a directory
    fn check_directory(&mut self, dir: &Path) -> Result<(), Box<dyn std::error::Error>> {
        if dir.to_string_lossy().contains("target") {
            return Ok(()); // Skip build artifacts
        }

        for entry in fs::read_dir(dir)? {
            let entry = entry?;
            let path = entry.path();

            if path.is_dir() {
                self.check_directory(&path)?;
            } else if path.extension().map_or(false, |ext| ext == "rs") {
                self.check_file(path)?;
            }
        }
        Ok(())
    }

    /// Report all violations found
    fn report_violations(&self) -> bool {
        let critical_violations: Vec<_> = self
            .violations
            .iter()
            .filter(|v| matches!(v.severity, Severity::Critical))
            .collect();

        let warning_violations: Vec<_> = self
            .violations
            .iter()
            .filter(|v| matches!(v.severity, Severity::Warning))
            .collect();

        // Report critical violations
        for violation in &critical_violations {
            println!(
                "CRITICAL [{}] {}:{} - {}",
                violation.rule,
                violation.file_path.display(),
                violation.line_number,
                violation.message
            );
        }

        // Report warnings
        for violation in &warning_violations {
            println!(
                "WARNING [{}] {}:{} - {}",
                violation.rule,
                violation.file_path.display(),
                violation.line_number,
                violation.message
            );
        }

        // Summary
        if !critical_violations.is_empty() {
            println!(
                "\nFound {} critical style violations that must be fixed",
                critical_violations.len()
            );
            return false;
        }

        if !warning_violations.is_empty() {
            println!(
                "\nFound {} warnings (non-blocking)",
                warning_violations.len()
            );
        } else {
            println!("\n✓ All style checks passed");
        }

        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn style_consistency_coverage_report() {
        println!("\n--- Riptide Style Consistency Coverage ---");
        println!(
            "Enforcing TigerBeetle-inspired consistency from docs/STYLE.md and docs/NAMING_CONVENTIONS.md:"
        );
        println!("\nCRITICAL (Must Fix):");
        println!("  ✓ BANNED_MODULE_NAME - No utils/helpers anti-pattern modules");
        println!("  ✓ INLINE_MODULE_REFERENCE - Smart import detection for frequently used types");
        println!("\nWARNING (Consistency Improvements):");
        println!("  ✓ MODULE_SIZE_LIMIT - Max 500 lines per module");
        println!("\nFocus: Smart pattern detection with context preservation");
        println!("Focus: Reduce verbosity while maintaining semantic clarity");
    }

    #[test]
    fn enforce_riptide_style_consistency() {
        let mut checker = StyleChecker::new();
        checker
            .check_workspace()
            .expect("Failed to check workspace");

        let passed = checker.report_violations();
        assert!(passed, "Style violations found - see output above");
    }
}
