//! Riptide Style Consistency Enforcement - Fast Rust-based implementation
//!
//! This module enforces TigerBeetle-inspired coding standards using efficient
//! Rust pattern matching that works cross-platform.

use std::fs;
use std::path::{Path, PathBuf};

/// Style violation severity levels
#[derive(Debug, Clone, Copy)]
enum Severity {
    Critical,
    Warning,
}

/// A detected style violation
#[derive(Debug)]
struct StyleViolation {
    severity: Severity,
    file_path: String,
    line_number: usize,
    rule: String,
    message: String,
}

impl StyleViolation {
    fn new(
        severity: Severity,
        file_path: &str,
        line_number: usize,
        rule: &str,
        message: &str,
    ) -> Self {
        Self {
            severity,
            file_path: file_path.to_string(),
            line_number,
            rule: rule.to_string(),
            message: message.to_string(),
        }
    }
}

/// Fast Rust-based style checker
struct FastRustStyleChecker {
    violations: Vec<StyleViolation>,
    workspace_root: PathBuf,
    files_processed: usize,
}

impl FastRustStyleChecker {
    fn new() -> Self {
        Self {
            violations: Vec::new(),
            workspace_root: PathBuf::from(".."),
            files_processed: 0,
        }
    }

    /// Add a violation to the list
    fn add_violation(&mut self, violation: StyleViolation) {
        self.violations.push(violation);
    }

    /// Find all Rust files in the riptide workspace efficiently
    fn find_rust_files(&self) -> Result<Vec<PathBuf>, Box<dyn std::error::Error>> {
        let mut files = Vec::new();
        self.find_rust_files_recursive(&self.workspace_root, &mut files, 0)?;
        Ok(files)
    }

    /// Recursively find Rust files with safety limits
    fn find_rust_files_recursive(
        &self,
        dir: &Path,
        files: &mut Vec<PathBuf>,
        depth: usize,
    ) -> Result<(), Box<dyn std::error::Error>> {
        // Safety limits
        if depth > 10 || files.len() > 500 {
            return Ok(());
        }

        // Skip target directories and other build artifacts
        if let Some(name) = dir.file_name() {
            if name == "target" || name == ".git" {
                return Ok(());
            }
        }

        // Only process riptide directories
        let dir_str = dir.to_string_lossy();
        if !dir_str.contains("riptide") && depth > 1 {
            return Ok(());
        }

        for entry in fs::read_dir(dir)? {
            let entry = entry?;
            let path = entry.path();

            if path.is_dir() {
                self.find_rust_files_recursive(&path, files, depth + 1)?;
            } else if path.extension().map_or(false, |ext| ext == "rs") {
                files.push(path);
            }
        }

        Ok(())
    }

    /// Check file size limits efficiently
    fn check_module_size(&mut self, file_path: &Path, content: &str) {
        let line_count = content.matches('\n').count() + 1;
        if line_count > 500 {
            self.add_violation(StyleViolation::new(
                Severity::Warning,
                &file_path.display().to_string(),
                1,
                "MODULE_SIZE_LIMIT",
                &format!(
                    "Module has {} lines, exceeding 500 line limit from docs/STYLE.md",
                    line_count
                ),
            ));
        }
    }

    /// Check for banned module names
    fn check_banned_module_names(&mut self, file_path: &Path) {
        let banned_names = ["util", "helper", "common", "utils", "helpers"];

        if let Some(file_name) = file_path.file_stem() {
            if let Some(name_str) = file_name.to_str() {
                if banned_names.contains(&name_str) {
                    self.add_violation(StyleViolation::new(
                        Severity::Critical,
                        &file_path.display().to_string(),
                        1,
                        "BANNED_MODULE_NAME",
                        &format!(
                            "Module name '{}' violates anti-pattern rules from docs/STYLE.md",
                            name_str
                        ),
                    ));
                }
            }
        }
    }

    /// Check for forbidden function prefixes efficiently
    fn check_forbidden_function_prefixes(&mut self, file_path: &Path, content: &str) {
        // Skip test files
        if file_path.to_string_lossy().contains("/tests/")
            || file_path.to_string_lossy().contains("test_")
        {
            return;
        }

        let forbidden_prefixes = ["get_", "set_"];

        for (line_num, line) in content.lines().enumerate() {
            let trimmed = line.trim();

            // Look for public function definitions
            if trimmed.starts_with("pub fn ") || trimmed.starts_with("pub async fn ") {
                for &prefix in &forbidden_prefixes {
                    let pattern = format!("pub fn {}", prefix);
                    if trimmed.contains(&pattern)
                        || trimmed.contains(&format!("pub async fn {}", prefix))
                    {
                        // Skip legitimate patterns
                        if trimmed.contains("get_mut")
                            || trimmed.contains("get_or_")
                            || trimmed.contains("set_capacity")
                        {
                            continue;
                        }

                        self.add_violation(StyleViolation::new(
                            Severity::Critical,
                            &file_path.display().to_string(),
                            line_num + 1,
                            "FORBIDDEN_PREFIX",
                            &format!(
                                "Function uses forbidden prefix '{}' - use descriptive names instead",
                                prefix
                            ),
                        ));
                    }
                }
            }
        }
    }

    /// Check for missing public documentation efficiently
    fn check_missing_public_documentation(&mut self, file_path: &Path, content: &str) {
        // Skip test files
        if file_path.to_string_lossy().contains("/tests/")
            || file_path.to_string_lossy().contains("test_")
        {
            return;
        }

        let lines: Vec<&str> = content.lines().collect();
        let mut functions_checked = 0;

        for (i, line) in lines.iter().enumerate() {
            let trimmed = line.trim();

            // Look for public functions
            if trimmed.starts_with("pub fn ") || trimmed.starts_with("pub async fn ") {
                functions_checked += 1;

                // Safety limit to prevent excessive processing
                if functions_checked > 100 {
                    break;
                }

                // Extract function name
                if let Some(fn_name) = self.extract_function_name(trimmed) {
                    // Skip test functions and constructors
                    if fn_name.starts_with("test_") || fn_name == "new" {
                        continue;
                    }

                    // Check for documentation in previous lines
                    let has_doc = self.has_documentation(i, &lines);

                    if !has_doc {
                        self.add_violation(StyleViolation::new(
                            Severity::Critical,
                            &file_path.display().to_string(),
                            i + 1,
                            "MISSING_PUBLIC_DOC",
                            &format!("Public function '{}' missing documentation", fn_name),
                        ));
                    } else {
                        // Check for Result return type and # Errors section
                        if self.returns_result(i, &lines) && !self.has_errors_section(i, &lines) {
                            self.add_violation(StyleViolation::new(
                                Severity::Critical,
                                &file_path.display().to_string(),
                                i + 1,
                                "MISSING_ERRORS_DOC",
                                &format!(
                                    "Function '{}' returns Result but missing '# Errors' section",
                                    fn_name
                                ),
                            ));
                        }

                        // Check for panic patterns and # Panics section
                        if self.can_panic(i, &lines) && !self.has_panics_section(i, &lines) {
                            self.add_violation(StyleViolation::new(
                                Severity::Critical,
                                &file_path.display().to_string(),
                                i + 1,
                                "MISSING_PANICS_DOC",
                                &format!(
                                    "Function '{}' can panic but missing '# Panics' section",
                                    fn_name
                                ),
                            ));
                        }
                    }
                }
            }
        }
    }

    /// Extract function name from function definition
    fn extract_function_name(&self, line: &str) -> Option<String> {
        if let Some(fn_start) = line.find("fn ") {
            let after_fn = &line[fn_start + 3..];
            if let Some(paren_pos) = after_fn.find('(') {
                return Some(after_fn[..paren_pos].trim().to_string());
            }
        }
        None
    }

    /// Check if function returns Result by looking at next few lines
    fn returns_result(&self, start_line: usize, lines: &[&str]) -> bool {
        let end = std::cmp::min(start_line + 3, lines.len());
        for i in start_line..end {
            if lines[i].contains("-> Result<") || lines[i].contains("->Result<") {
                return true;
            }
        }
        false
    }

    /// Check if function can panic by looking within function boundaries
    fn can_panic(&self, start_line: usize, lines: &[&str]) -> bool {
        // Find the function body by looking for opening brace
        let mut func_start = start_line;
        let mut found_opening_brace = false;

        // Look for the opening brace of the function
        for i in start_line..std::cmp::min(start_line + 5, lines.len()) {
            if lines[i].contains('{') {
                func_start = i;
                found_opening_brace = true;
                break;
            }
        }

        if !found_opening_brace {
            return false;
        }

        // Find the matching closing brace
        let mut brace_count = 0;
        let mut func_end = func_start;

        for i in func_start..std::cmp::min(func_start + 50, lines.len()) {
            let line = lines[i];
            for ch in line.chars() {
                match ch {
                    '{' => brace_count += 1,
                    '}' => {
                        brace_count -= 1;
                        if brace_count == 0 {
                            func_end = i;
                            break;
                        }
                    }
                    _ => {}
                }
            }
            if brace_count == 0 {
                break;
            }
        }

        // Check for panic patterns only within function boundaries
        for i in func_start..=func_end {
            let line = lines[i].trim();

            // Skip comments and empty lines
            if line.starts_with("//") || line.is_empty() {
                continue;
            }

            // Always panic patterns
            if line.contains("panic!(")
                || line.contains("unreachable!(")
                || line.contains("unimplemented!(")
                || line.contains("todo!(")
            {
                return true;
            }

            // Check for unwrap/expect patterns, but exclude known safe cases
            if line.contains(".unwrap()") || line.contains(".expect(") {
                // Skip if it's a safe atomic operation
                if line.contains(".load(") || line.contains(".store(") {
                    continue;
                }

                // Skip if there's a safety check in previous lines within function
                let check_start =
                    std::cmp::max(func_start, if i >= 3 { i - 3 } else { func_start });
                let mut has_safety_check = false;

                for j in check_start..i {
                    let prev_line = lines[j];
                    if prev_line.contains(".is_some()")
                        || prev_line.contains(".is_ok()")
                        || prev_line.contains("if let Some(")
                        || prev_line.contains("if let Ok(")
                        || prev_line.contains("while let Some(")
                        || prev_line.contains("match ")
                    {
                        has_safety_check = true;
                        break;
                    }
                }

                if !has_safety_check {
                    return true;
                }
            }
        }
        false
    }

    /// Check if documentation contains # Errors section
    fn has_errors_section(&self, line_index: usize, lines: &[&str]) -> bool {
        let start = if line_index > 15 { line_index - 15 } else { 0 };
        for i in (start..line_index).rev() {
            let line = lines[i].trim();
            if line.contains("# Errors") {
                return true;
            }
            // Stop if we hit non-doc content
            if !line.starts_with("///") && !line.is_empty() && !line.starts_with("#[") {
                break;
            }
        }
        false
    }

    /// Check if function has documentation comments
    fn has_documentation(&self, line_index: usize, lines: &[&str]) -> bool {
        let start = if line_index > 15 { line_index - 15 } else { 0 };
        for i in (start..line_index).rev() {
            let line = lines[i].trim();
            if line.starts_with("///") {
                return true;
            }
            // Stop if we hit non-doc content (but allow attributes)
            if !line.is_empty() && !line.starts_with("#[") && !line.starts_with("///") {
                break;
            }
        }
        false
    }

    /// Check if documentation contains # Panics section
    fn has_panics_section(&self, line_index: usize, lines: &[&str]) -> bool {
        let start = if line_index > 15 { line_index - 15 } else { 0 };
        for i in (start..line_index).rev() {
            let line = lines[i].trim();
            if line.contains("# Panics") {
                return true;
            }
            // Stop if we hit non-doc content
            if !line.starts_with("///") && !line.is_empty() && !line.starts_with("#[") {
                break;
            }
        }
        false
    }

    /// Process a single file efficiently
    fn check_file(&mut self, file_path: &Path) -> Result<(), Box<dyn std::error::Error>> {
        // Safety limit on total files processed
        self.files_processed += 1;
        if self.files_processed > 200 {
            return Ok(());
        }

        // Read file content efficiently
        let content = fs::read_to_string(file_path)?;

        // Skip very large files to prevent hanging
        if content.len() > 1_000_000 {
            // 1MB limit
            return Ok(());
        }

        // Run all checks
        self.check_module_size(file_path, &content);
        self.check_banned_module_names(file_path);
        self.check_forbidden_function_prefixes(file_path, &content);
        self.check_missing_public_documentation(file_path, &content);

        Ok(())
    }

    /// Run all style checks
    fn check_workspace(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        let files = self.find_rust_files()?;

        for file_path in files {
            if let Err(e) = self.check_file(&file_path) {
                eprintln!("Warning: Failed to check file {:?}: {}", file_path, e);
            }
        }

        Ok(())
    }

    /// Report all violations and return true if no critical violations found
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

        if !critical_violations.is_empty() {
            eprintln!("CRITICAL VIOLATIONS (must fix):");
            for violation in &critical_violations {
                eprintln!(
                    "  {} [{}] {}:{} - {}",
                    violation.rule,
                    match violation.severity {
                        Severity::Critical => "CRITICAL",
                        Severity::Warning => "WARNING",
                    },
                    violation.file_path,
                    violation.line_number,
                    violation.message
                );
            }
            eprintln!();
        }

        if !warning_violations.is_empty() {
            for violation in &warning_violations {
                eprintln!(
                    "WARNING [{}] {}:{} - {}",
                    violation.rule, violation.file_path, violation.line_number, violation.message
                );
            }
            eprintln!();
        }

        if critical_violations.is_empty() && warning_violations.is_empty() {
            println!("✓ All style checks passed");
        } else {
            eprintln!("Found {} critical violations", critical_violations.len());
            if !warning_violations.is_empty() {
                eprintln!("Found {} warnings (non-blocking)", warning_violations.len());
            }
        }

        critical_violations.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_function_name_extraction() {
        let checker = FastRustStyleChecker::new();

        assert_eq!(
            checker.extract_function_name("pub fn test_function() -> Result<(), Error>"),
            Some("test_function".to_string())
        );

        assert_eq!(
            checker.extract_function_name("pub fn complex_function(param: &str) -> String"),
            Some("complex_function".to_string())
        );

        assert_eq!(
            checker.extract_function_name("pub async fn async_function() -> Result<(), Error>"),
            Some("async_function".to_string())
        );
    }

    #[test]
    fn test_returns_result() {
        let checker = FastRustStyleChecker::new();
        let lines = vec!["pub fn test() -> Result<(), Error> {", "    Ok(())", "}"];

        assert!(checker.returns_result(0, &lines));

        let lines2 = vec!["pub fn test() -> String {", "    String::new()", "}"];

        assert!(!checker.returns_result(0, &lines2));
    }

    #[test]
    fn test_can_panic() {
        let checker = FastRustStyleChecker::new();
        let lines = vec![
            "pub fn test() {",
            "    let value = something.unwrap();",
            "}",
        ];

        assert!(checker.can_panic(0, &lines));

        let lines2 = vec![
            "pub fn test() {",
            "    let value = something.unwrap_or_default();",
            "}",
        ];

        assert!(!checker.can_panic(0, &lines2));
    }

    #[test]
    fn test_has_errors_section() {
        let checker = FastRustStyleChecker::new();
        let lines = vec![
            "/// Test function",
            "///",
            "/// # Errors",
            "/// Returns error if something fails",
            "pub fn test() -> Result<(), Error> {",
        ];

        assert!(checker.has_errors_section(4, &lines));

        let lines2 = vec![
            "/// Test function",
            "/// Returns a result",
            "pub fn test() -> Result<(), Error> {",
        ];

        assert!(!checker.has_errors_section(2, &lines2));
    }

    #[test]
    fn style_consistency_coverage_report() {
        println!("\n--- Riptide Style Consistency Coverage ---");
        println!(
            "Enforcing TigerBeetle-inspired consistency from docs/STYLE.md and docs/NAMING_CONVENTIONS.md:\n"
        );

        println!("CRITICAL (Must Fix):");
        println!("  ✓ BANNED_MODULE_NAME - No utils/helpers anti-pattern modules");
        println!("  ✓ FORBIDDEN_PREFIX - No get_/set_ function prefixes (use descriptive names)");
        println!("  ✓ MISSING_PUBLIC_DOC - All public functions must have documentation");
        println!("  ✓ MISSING_ERRORS_DOC - Result-returning functions need # Errors section");
        println!("  ✓ MISSING_PANICS_DOC - Functions that can panic need # Panics section");
        println!();

        println!("WARNING (Consistency Improvements):");
        println!("  ✓ MODULE_SIZE_LIMIT - Max 500 lines per module");
        println!();

        println!("Focus: Fast Rust-based pattern detection");
        println!("Focus: Cross-platform compatibility");
        println!("Focus: Reduce verbosity while maintaining semantic clarity");
    }

    #[test]
    fn enforce_riptide_style_consistency() {
        let mut checker = FastRustStyleChecker::new();

        // Run all checks
        checker
            .check_workspace()
            .expect("Failed to check workspace");

        // Report results
        let passed = checker.report_violations();

        // Allow warnings but fail on critical violations
        assert!(passed, "Critical style violations found - see output above");
    }
}
