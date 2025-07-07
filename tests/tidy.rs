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
        println!("DEBUG: Processing file: {:?}", file_path);
        let content = fs::read_to_string(&file_path)?;
        self.current_file = file_path;
        self.file_lines = content.lines().map(|s| s.to_string()).collect();

        println!("DEBUG: File has {} lines", self.file_lines.len());

        // Core structural checks (always run)
        self.check_module_size();
        self.check_banned_module_names();
        self.check_inline_module_references();

        // Naming convention checks
        self.check_forbidden_function_prefixes();

        println!("DEBUG: Starting documentation check...");
        self.check_missing_public_documentation();
        println!("DEBUG: Documentation check completed");

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

        // Explicit list of key files to check (avoids hanging on recursive traversal)
        let key_files = [
            // Core library files
            "riptide-core/src/lib.rs",
            "riptide-core/src/config.rs",
            "riptide-core/src/server_components.rs",
            "riptide-core/src/network/mod.rs",
            "riptide-core/src/network/peer.rs",
            "riptide-core/src/network/simulation.rs",
            "riptide-core/src/storage/mod.rs",
            "riptide-core/src/storage/file_library.rs",
            "riptide-core/src/storage/file_storage/mod.rs",
            "riptide-core/src/storage/file_storage/storage.rs",
            "riptide-core/src/storage/file_storage/types.rs",
            "riptide-core/src/streaming/strategy.rs",
            "riptide-core/src/streaming/ffmpeg.rs",
            "riptide-core/src/streaming/range_handler.rs",
            "riptide-core/src/streaming/remuxed_streaming.rs",
            "riptide-core/src/streaming/stream_coordinator/mod.rs",
            "riptide-core/src/streaming/stream_coordinator/coordinator.rs",
            "riptide-core/src/streaming/stream_coordinator/session.rs",
            "riptide-core/src/streaming/stream_coordinator/types.rs",
            // CLI files
            "riptide-cli/src/main.rs",
            "riptide-cli/src/lib.rs",
            // Web files
            "riptide-web/src/lib.rs",
            "riptide-web/src/routes.rs",
            "riptide-web/src/middleware.rs",
            "riptide-web/src/handlers.rs",
            // Simulation files
            "riptide-sim/src/lib.rs",
            "riptide-sim/src/deterministic/mod.rs",
            "riptide-sim/src/deterministic/simulation.rs",
            "riptide-sim/src/deterministic/events.rs",
            "riptide-sim/src/deterministic/clock.rs",
            "riptide-sim/src/deterministic/state.rs",
            "riptide-sim/src/deterministic/invariants.rs",
            "riptide-sim/src/deterministic/resources.rs",
            // Search files
            "riptide-search/src/lib.rs",
        ];

        let mut files_processed = 0;
        for file_name in &key_files {
            let file_path = workspace_root.join(file_name);
            if file_path.exists() {
                self.check_file(file_path)?;
                files_processed += 1;

                // Safety limit to prevent excessive processing
                if files_processed > 50 {
                    break;
                }
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

    /// Check for forbidden function prefixes (get_, set_)
    fn check_forbidden_function_prefixes(&mut self) {
        let mut violations = Vec::new();

        for (line_num, line) in self.file_lines.iter().enumerate() {
            let trimmed = line.trim();

            // Skip comments and use statements
            if trimmed.starts_with("//") || trimmed.starts_with("use ") {
                continue;
            }

            // Look for function definitions with forbidden prefixes
            if let Some(captures) = regex::Regex::new(r"pub\s+(?:async\s+)?fn\s+(get|set)_(\w+)")
                .expect("Invalid regex")
                .captures(line)
            {
                let prefix = &captures[1];
                let function_name = &captures[2];

                // Skip false positives for legitimate get/set cases
                if self.is_legitimate_getter_setter(function_name) {
                    continue;
                }

                let suggested_name = match prefix {
                    "get" => {
                        if function_name.starts_with("or_") {
                            // get_or_create -> get_or_create (keep as is)
                            continue;
                        } else {
                            // get_peers -> peers, get_stats -> stats
                            function_name.to_string()
                        }
                    }
                    "set" => {
                        // set_peers -> update_peers, set_config -> configure
                        if function_name.contains("config") {
                            format!("configure_{}", function_name.replace("config", ""))
                        } else {
                            format!("update_{}", function_name)
                        }
                    }
                    _ => function_name.to_string(),
                };

                violations.push((
                    line_num + 1,
                    "FORBIDDEN_PREFIX".to_string(),
                    format!(
                        "Function '{}_{}' uses forbidden prefix. Use descriptive name: '{}'",
                        prefix, function_name, suggested_name
                    ),
                ));
            }
        }

        // Add all violations
        for (line_num, rule, message) in violations {
            self.add_violation(Severity::Critical, line_num, &rule, &message);
        }
    }

    /// Check if this is a legitimate getter/setter that should be allowed
    fn is_legitimate_getter_setter(&self, function_name: &str) -> bool {
        // Allow get_or_* patterns (get_or_create, get_or_insert, etc.)
        function_name.starts_with("or_")
    }

    /// Check for missing public documentation (simplified)
    fn check_missing_public_documentation(&mut self) {
        // Skip test files
        if self.current_file.to_string_lossy().contains("/tests/")
            || self.current_file.to_string_lossy().contains("test_")
        {
            println!("DEBUG: Skipping test file");
            return;
        }

        let mut violations = Vec::new();
        let mut functions_found = 0;

        for (i, line) in self.file_lines.iter().enumerate() {
            let trimmed = line.trim();

            // Look for public functions
            if trimmed.starts_with("pub fn ") || trimmed.starts_with("pub async fn ") {
                functions_found += 1;
                if functions_found % 5 == 0 {
                    println!("DEBUG: Found {} public functions so far", functions_found);
                }

                // Extract function name
                if let Some(fn_pos) = trimmed.find("fn ") {
                    let after_fn = &trimmed[fn_pos + 3..];
                    if let Some(paren_pos) = after_fn.find('(') {
                        let function_name = after_fn[..paren_pos].trim();

                        // Skip test functions and constructors
                        if function_name.starts_with("test_") || function_name == "new" {
                            continue;
                        }

                        // Safety limit to prevent hanging
                        if functions_found > 100 {
                            println!("DEBUG: Stopping at function limit (100)");
                            break;
                        }

                        // Check for documentation in previous lines (simple check)
                        let has_doc = i > 0 && self.file_lines[i - 1].trim().starts_with("///");

                        if !has_doc {
                            violations.push((
                                i + 1,
                                "MISSING_PUBLIC_DOC",
                                format!(
                                    "Public function '{}' missing documentation",
                                    function_name
                                ),
                            ));
                        } else {
                            // Check for Result return type and # Errors section
                            let next_few_lines = self
                                .file_lines
                                .get(i..std::cmp::min(i + 3, self.file_lines.len()))
                                .unwrap_or(&[])
                                .join(" ");

                            if (next_few_lines.contains("-> Result<")
                                || next_few_lines.contains("->Result<"))
                                && !self.has_errors_in_docs(i)
                            {
                                violations.push((
                                    i + 1,
                                    "MISSING_ERRORS_DOC",
                                    format!("Function '{}' returns Result but missing '# Errors' section", function_name),
                                ));
                            }

                            // Check for panic patterns and # Panics section
                            if self.has_panic_patterns(i) && !self.has_panics_in_docs(i) {
                                violations.push((
                                    i + 1,
                                    "MISSING_PANICS_DOC",
                                    format!(
                                        "Function '{}' can panic but missing '# Panics' section",
                                        function_name
                                    ),
                                ));
                            }
                        }
                    }
                }
            }
        }

        println!(
            "DEBUG: Found {} public functions, {} violations",
            functions_found,
            violations.len()
        );

        // Add all collected violations
        for (line_num, rule, message) in violations {
            self.add_violation(Severity::Critical, line_num, rule, &message);
        }
    }

    /// Check if documentation contains # Errors section
    fn has_errors_in_docs(&self, line_index: usize) -> bool {
        for i in (0..=line_index).rev().take(10) {
            if let Some(line) = self.file_lines.get(i) {
                if line.contains("# Errors") {
                    return true;
                }
                if !line.trim().starts_with("///") && !line.trim().is_empty() {
                    break;
                }
            }
        }
        false
    }

    /// Check if documentation contains # Panics section
    fn has_panics_in_docs(&self, line_index: usize) -> bool {
        for i in (0..=line_index).rev().take(10) {
            if let Some(line) = self.file_lines.get(i) {
                if line.contains("# Panics") {
                    return true;
                }
                if !line.trim().starts_with("///") && !line.trim().is_empty() {
                    break;
                }
            }
        }
        false
    }

    /// Check if function has panic patterns in next few lines
    fn has_panic_patterns(&self, start_line: usize) -> bool {
        let end = std::cmp::min(start_line + 5, self.file_lines.len());
        for i in start_line..end {
            if let Some(line) = self.file_lines.get(i) {
                if line.contains("panic!")
                    || line.contains(".unwrap()")
                    || line.contains(".expect(")
                    || line.contains("unreachable!")
                    || line.contains("unimplemented!")
                    || line.contains("todo!")
                {
                    return true;
                }
            }
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_forbidden_prefix_regex() {
        let test_lines = vec![
            "pub fn get_stats() -> u32 {",
            "pub async fn set_config(value: u32) {",
            "pub fn get_or_create() -> Self {", // Should be allowed
            "fn get_private() -> u32 {",        // Not public, should be ignored
        ];

        let regex = regex::Regex::new(r"pub\s+(?:async\s+)?fn\s+(get|set)_(\w+)").unwrap();

        for line in &test_lines {
            println!("Testing: {}", line);
            if let Some(captures) = regex.captures(line) {
                println!(
                    "  Matched: prefix='{}', name='{}'",
                    &captures[1], &captures[2]
                );
            } else {
                println!("  No match");
            }
        }
    }

    #[test]
    fn style_consistency_coverage_report() {
        println!("\n--- Riptide Style Consistency Coverage ---");
        println!(
            "Enforcing TigerBeetle-inspired consistency from docs/STYLE.md and docs/NAMING_CONVENTIONS.md:"
        );
        println!("\nCRITICAL (Must Fix):");
        println!("  ✓ BANNED_MODULE_NAME - No utils/helpers anti-pattern modules");
        println!("  ✓ INLINE_MODULE_REFERENCE - Smart import detection for frequently used types");
        println!("  ✓ FORBIDDEN_PREFIX - No get_/set_ function prefixes (use descriptive names)");
        println!("  ✓ MISSING_PUBLIC_DOC - All public functions must have documentation");
        println!("  ✓ MISSING_ERRORS_DOC - Result-returning functions need # Errors section");
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
