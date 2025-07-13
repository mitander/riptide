//! Dead Code Enforcement
//!
//! Enforces that production code does not contain #[allow(dead_code)] attributes.
//! Test code is exempt from this restriction as dead code may be acceptable during
//! development and testing phases.

use std::fs;
use std::path::{Path, PathBuf};

/// A dead code allowance violation found in production code
#[derive(Debug)]
struct DeadCodeViolation {
    file_path: String,
    line_number: usize,
    context: String,
}

impl DeadCodeViolation {
    fn new(file_path: &str, line_number: usize, context: &str) -> Self {
        Self {
            file_path: file_path.to_string(),
            line_number,
            context: context.to_string(),
        }
    }
}

/// Checker for dead code allowance violations in production code
struct DeadCodeChecker {
    violations: Vec<DeadCodeViolation>,
    files_checked: usize,
}

impl DeadCodeChecker {
    fn new() -> Self {
        Self {
            violations: Vec::new(),
            files_checked: 0,
        }
    }

    /// Find all Rust files in the workspace
    fn find_rust_files(&self) -> Result<Vec<PathBuf>, Box<dyn std::error::Error>> {
        let mut files = Vec::new();
        Self::find_rust_files_recursive(Path::new(".."), &mut files, 0)?;
        Ok(files)
    }

    fn find_rust_files_recursive(
        dir: &Path,
        files: &mut Vec<PathBuf>,
        depth: usize,
    ) -> Result<(), Box<dyn std::error::Error>> {
        // Prevent infinite recursion
        if depth > 10 {
            return Ok(());
        }

        if dir.is_dir() {
            for entry in fs::read_dir(dir)? {
                let entry = entry?;
                let path = entry.path();

                // Skip hidden directories and target directory
                if let Some(name) = path.file_name()
                    && (name.to_string_lossy().starts_with('.') || name == "target")
                {
                    continue;
                }

                if path.is_dir() {
                    Self::find_rust_files_recursive(&path, files, depth + 1)?;
                } else if path.extension().map(|s| s == "rs").unwrap_or(false) {
                    files.push(path);
                }
            }
        }
        Ok(())
    }

    /// Check if a file path represents test code
    fn is_test_file(&self, path: &Path) -> bool {
        let path_str = path.to_string_lossy().to_lowercase();

        // Allow in test directories
        path_str.contains("/tests/") ||
        path_str.contains("/test/") ||
        path_str.contains("\\tests\\") ||
        path_str.contains("\\test\\") ||
        // Allow in files with "test" in the name
        path_str.contains("test_") ||
        path_str.contains("_test") ||
        path_str.ends_with("_tests.rs") ||
        path_str.ends_with("tests.rs") ||
        // Allow in integration test files
        path_str.contains("integration") ||
        path_str.contains("e2e") ||
        // Allow in dead code enforcement test itself (contains test examples)
        path_str.contains("dead_code_enforcement.rs") ||
        // Allow in style.rs (contains documentation mentioning dead_code)
        path_str.ends_with("style.rs")
    }

    /// Check a single file for dead code allowance violations
    fn check_file(&mut self, path: &Path) -> Result<(), Box<dyn std::error::Error>> {
        // Skip test files
        if self.is_test_file(path) {
            return Ok(());
        }

        let content = fs::read_to_string(path)?;
        self.files_checked += 1;

        for (line_number, line) in content.lines().enumerate() {
            let line_number = line_number + 1;
            let trimmed = line.trim();

            // Look for #[allow(dead_code)] attributes
            if trimmed.contains("#[allow(dead_code)]")
                || (trimmed.contains("#[allow(") && trimmed.contains("dead_code"))
            {
                let violation = DeadCodeViolation::new(&path.to_string_lossy(), line_number, line);
                self.violations.push(violation);
            }
        }

        Ok(())
    }

    /// Check all files in the workspace
    fn check_workspace(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        let files = self.find_rust_files()?;

        for file in files {
            self.check_file(&file)?;
        }

        Ok(())
    }

    /// Report violations and return whether the check passed
    fn report_violations(&self) -> bool {
        if self.violations.is_empty() {
            println!(
                "Dead code enforcement: {} files checked, no violations found",
                self.files_checked
            );
            return true;
        }

        println!("Dead code enforcement violations found:");
        println!();

        for violation in &self.violations {
            println!("{}:{}", violation.file_path, violation.line_number);
            println!("  {}", violation.context.trim());
            println!();
        }

        println!(
            "Found {} violation(s) in {} file(s) checked",
            self.violations.len(),
            self.files_checked
        );
        println!();
        println!("REASON: Production code should not contain #[allow(dead_code)] attributes.");
        println!("Dead code indicates unused functionality that should be either:");
        println!("  1. Removed if truly unnecessary");
        println!("  2. Used if it serves a purpose");
        println!("  3. Documented with a clear justification if it must remain");
        println!();
        println!("If this code is legitimately dead during development, consider:");
        println!("  - Moving it to a test module or test file");
        println!("  - Using feature flags to conditionally compile it");
        println!("  - Adding a // TODO comment with a plan for its use");
        println!();
        println!("Test files are exempt from this check.");

        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_test_file() {
        let checker = DeadCodeChecker::new();

        // Should be considered test files
        assert!(checker.is_test_file(Path::new("src/tests/example.rs")));
        assert!(checker.is_test_file(Path::new("tests/integration.rs")));
        assert!(checker.is_test_file(Path::new("src/test_helper.rs")));
        assert!(checker.is_test_file(Path::new("src/helper_test.rs")));
        assert!(checker.is_test_file(Path::new("integration/peer_tests.rs")));
        assert!(checker.is_test_file(Path::new("e2e/streaming_workflow.rs")));

        // Should NOT be considered test files
        assert!(!checker.is_test_file(Path::new("src/lib.rs")));
        assert!(!checker.is_test_file(Path::new("src/engine.rs")));
        assert!(!checker.is_test_file(Path::new("src/torrent/mod.rs")));
    }

    #[test]
    fn test_dead_code_detection() {
        let mut checker = DeadCodeChecker::new();

        // Create a temporary file path for testing
        let test_path = Path::new("test_file.rs");

        // Test code with dead_code allow
        let test_content = r#"
use std::collections::HashMap;

#[allow(dead_code)]
struct UnusedStruct {
    field: u32,
}

#[allow(clippy::missing_docs, dead_code)]
fn unused_function() {
    println!("This function is never called");
}
"#;

        // Save the original check_file behavior by testing the logic inline
        for (line_number, line) in test_content.lines().enumerate() {
            let line_number = line_number + 1;
            let trimmed = line.trim();

            if trimmed.contains("#[allow(dead_code)]")
                || (trimmed.contains("#[allow(") && trimmed.contains("dead_code"))
            {
                let violation =
                    DeadCodeViolation::new(&test_path.to_string_lossy(), line_number, line);
                checker.violations.push(violation);
            }
        }

        // Should find 2 violations
        assert_eq!(checker.violations.len(), 2);
        assert_eq!(checker.violations[0].line_number, 4); // #[allow(dead_code)]
        assert_eq!(checker.violations[1].line_number, 9); // #[allow(clippy::missing_docs, dead_code)]
    }

    #[test]
    fn dead_code_enforcement() {
        let mut checker = DeadCodeChecker::new();

        // Run the check
        checker
            .check_workspace()
            .expect("Failed to check workspace");

        // Report results - this will fail if violations are found
        let passed = checker.report_violations();

        // In CI, we want this to fail if there are violations
        assert!(
            passed,
            "Dead code allowance violations found in production code - see output above"
        );
    }
}
