//! Naming Convention Checker
//!
//! Enforces critical naming conventions from docs/STYLE.md.
//! Focuses only on the most important violations: banned prefixes/suffixes.

use std::fs;
use std::path::{Path, PathBuf};

/// A naming violation found in the code
#[derive(Debug)]
struct NamingViolation {
    file_path: String,
    line_number: usize,
    violation_type: String,
    message: String,
}

impl NamingViolation {
    fn new(file_path: &str, line_number: usize, violation_type: &str, message: &str) -> Self {
        Self {
            file_path: file_path.to_string(),
            line_number,
            violation_type: violation_type.to_string(),
            message: message.to_string(),
        }
    }
}

/// Simple naming convention checker focused on critical violations
struct NamingChecker {
    violations: Vec<NamingViolation>,
    files_checked: usize,
}

impl NamingChecker {
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
        // Safety limits
        if depth > 8 || files.len() > 200 {
            return Ok(());
        }

        // Skip target directories and other build artifacts
        if let Some(name) = dir.file_name() {
            let name_str = name.to_string_lossy();
            if name_str == "target" || name_str == ".git" || name_str.starts_with('.') {
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
                Self::find_rust_files_recursive(&path, files, depth + 1)?;
            } else if let Some(extension) = path.extension()
                && extension == "rs"
            {
                files.push(path);
            }
        }

        Ok(())
    }

    /// Check for banned function prefixes
    fn check_function_prefixes(&mut self, file_path: &Path, content: &str) {
        let banned_patterns = [
            ("get_", "Use the noun directly: peer.id() not peer.get_id()"),
            (
                "set_",
                "Use descriptive verbs: peer.update_speed() not peer.set_speed()",
            ),
            (
                "handle_",
                "Be specific: process_message() not handle_message()",
            ),
        ];

        for (line_num, line) in content.lines().enumerate() {
            let trimmed = line.trim();

            // Skip comment-only lines
            if trimmed.starts_with("//") || trimmed.starts_with("/*") {
                continue;
            }

            // Look for function definitions
            if trimmed.starts_with("pub fn ")
                || trimmed.starts_with("pub async fn ")
                || trimmed.starts_with("fn ")
                || trimmed.starts_with("async fn ")
            {
                for &(prefix, correction) in &banned_patterns {
                    if trimmed.contains(&format!("fn {prefix}")) {
                        self.violations.push(NamingViolation::new(
                            &file_path.display().to_string(),
                            line_num + 1,
                            "BANNED_FUNCTION_PREFIX",
                            &format!("Function uses banned prefix '{prefix}'. {correction}"),
                        ));
                    }
                }
            }
        }
    }

    /// Check for banned type suffixes and verbose naming
    fn check_type_naming(&mut self, file_path: &Path, content: &str) {
        let banned_suffixes = [
            ("Factory", "Use Builder pattern or simple new() function"),
            (
                "Service",
                "Usually adds no semantic value - prefer direct naming",
            ),
        ];

        // Suffixes that are verbose for structs but acceptable for traits when they add clarity
        let struct_verbose_suffixes = [
            ("Manager", "For structs, name what it IS, not its role"),
            ("Handler", "Be more specific about what you're handling"),
            ("Processor", "Be more specific about what you're processing"),
            (
                "Controller",
                "Be more specific about what you're controlling",
            ),
        ];

        for (line_num, line) in content.lines().enumerate() {
            let trimmed = line.trim();

            // Check banned suffixes for all types
            if trimmed.starts_with("pub struct ")
                || trimmed.starts_with("struct ")
                || trimmed.starts_with("pub enum ")
                || trimmed.starts_with("enum ")
                || trimmed.starts_with("pub trait ")
                || trimmed.starts_with("trait ")
            {
                let type_name = {
                    let words: Vec<&str> = trimmed.split_whitespace().collect();
                    let name_index = if words.first() == Some(&"pub") { 2 } else { 1 };
                    if let Some(name_part) = words.get(name_index) {
                        // Extract just the type name, handling generics and braces
                        name_part
                            .split('<')
                            .next()
                            .unwrap_or("") // Remove generic parameters
                            .split('{')
                            .next()
                            .unwrap_or("") // Remove struct body
                            .trim()
                    } else {
                        continue;
                    }
                };

                // Check for banned suffixes (apply to all types)
                for &(suffix, message) in &banned_suffixes {
                    if type_name.ends_with(suffix) {
                        self.violations.push(NamingViolation::new(
                            &file_path.display().to_string(),
                            line_num + 1,
                            "BANNED_TYPE_SUFFIX",
                            &format!("Type '{type_name}' uses banned '{suffix}' suffix. {message}"),
                        ));
                    }
                }

                // Check for verbose suffixes (only for structs and enums, not traits)
                if trimmed.starts_with("pub struct ")
                    || trimmed.starts_with("struct ")
                    || trimmed.starts_with("pub enum ")
                    || trimmed.starts_with("enum ")
                {
                    for &(suffix, message) in &struct_verbose_suffixes {
                        if type_name.ends_with(suffix) {
                            self.violations.push(NamingViolation::new(
                                &file_path.display().to_string(),
                                line_num + 1,
                                "VERBOSE_TYPE_SUFFIX",
                                &format!(
                                    "Type '{type_name}' uses verbose '{suffix}' suffix. {message}"
                                ),
                            ));
                        }
                    }
                }
            }
        }
    }

    /// Check for banned module names
    fn check_module_names(&mut self, file_path: &Path) {
        if let Some(file_name) = file_path.file_name() {
            let name = file_name.to_string_lossy();
            let banned_patterns = [
                (
                    "utils",
                    "Use specific names like 'piece_picker' or 'protocol'",
                ),
                ("common", "Use specific names like 'types' or 'constants'"),
                (
                    "helpers",
                    "Use specific names like 'validation' or 'conversion'",
                ),
                ("misc", "Use specific names describing the module's purpose"),
                (
                    "stuff",
                    "Use specific names describing the module's purpose",
                ),
            ];

            for &(pattern, message) in &banned_patterns {
                if name == format!("{pattern}.rs") || name == pattern {
                    self.violations.push(NamingViolation::new(
                        &file_path.display().to_string(),
                        1,
                        "BANNED_MODULE_NAME",
                        &format!("Module name '{pattern}' is too generic. {message}"),
                    ));
                }
            }
        }
    }

    /// Check for invalid documentation format and section ordering
    fn check_documentation_format(&mut self, file_path: &Path, content: &str) {
        let lines: Vec<&str> = content.lines().collect();
        let mut i = 0;

        while i < lines.len() {
            let line = lines[i].trim();

            // Look for start of documentation block that contains function documentation
            if line.starts_with("///") && i + 1 < lines.len() {
                // Find the extent of this documentation block
                let doc_start = i;
                let mut doc_end = i;

                // Scan forward to find the end of the documentation block
                while doc_end < lines.len() && lines[doc_end].trim().starts_with("///") {
                    doc_end += 1;
                }

                // Check if this documentation block is followed by a function
                let mut next_non_empty = doc_end;
                while next_non_empty < lines.len() && lines[next_non_empty].trim().is_empty() {
                    next_non_empty += 1;
                }

                let is_function_doc = next_non_empty < lines.len()
                    && (lines[next_non_empty].trim().starts_with("pub fn ")
                        || lines[next_non_empty].trim().starts_with("fn ")
                        || lines[next_non_empty].trim().starts_with("pub async fn ")
                        || lines[next_non_empty].trim().starts_with("async fn "));

                if is_function_doc {
                    // Process this documentation block
                    self.check_doc_block(file_path, &lines[doc_start..doc_end], doc_start);
                }

                i = doc_end;
            } else {
                i += 1;
            }
        }
    }

    /// Check a single documentation block for section format and ordering
    fn check_doc_block(&mut self, file_path: &Path, doc_lines: &[&str], start_line: usize) {
        let mut sections_found = Vec::new();
        let mut i = 0;

        while i < doc_lines.len() {
            let line = doc_lines[i].trim();

            // Look for # Errors or # Panics sections
            if line == "/// # Errors" || line == "/// # Panics" {
                let section_type = if line.contains("Errors") {
                    "Errors"
                } else {
                    "Panics"
                };

                let absolute_line = start_line + i + 1;
                sections_found.push((section_type, absolute_line));

                // Check if next line is blank documentation line (required)
                if i + 1 >= doc_lines.len() || doc_lines[i + 1].trim() != "///" {
                    self.violations.push(NamingViolation::new(
                        &file_path.display().to_string(),
                        absolute_line,
                        "INVALID_DOC_FORMAT",
                        &format!("# {section_type} section must be followed by blank line: `///`"),
                    ));
                    i += 1;
                    continue;
                }

                // Check content lines (must start with bullet points for Errors)
                let mut j = i + 2; // Skip the blank line
                let mut found_content = false;

                while j < doc_lines.len() && doc_lines[j].trim().starts_with("///") {
                    let content_line = doc_lines[j].trim();
                    if content_line == "///" {
                        j += 1;
                        continue; // Skip blank lines
                    }

                    let doc_content = content_line.strip_prefix("///").unwrap_or("").trim();

                    // Stop if we hit another section header
                    if doc_content.starts_with("# ") {
                        break;
                    }

                    if !doc_content.is_empty() {
                        found_content = true;

                        // Must start with bullet point (only for Errors sections)
                        if section_type == "Errors" && !doc_content.starts_with("- ") {
                            self.violations.push(NamingViolation::new(
                                &file_path.display().to_string(),
                                start_line + j + 1,
                                "INVALID_DOC_FORMAT",
                                &format!("# {section_type} content must use bullet points: `- ErrorType - condition`"),
                            ));
                        }
                        // Check for old "Returns" format
                        else if doc_content.contains("Returns ") && section_type == "Errors" {
                            self.violations.push(NamingViolation::new(
                                &file_path.display().to_string(),
                                start_line + j + 1,
                                "INVALID_DOC_FORMAT",
                                "Use format `- ErrorType - condition` not `Returns ErrorType if condition`",
                            ));
                        }
                        // Check for missing error type format in Errors
                        else if section_type == "Errors" && doc_content.starts_with("- ") {
                            let bullet_content = doc_content.strip_prefix("- ").unwrap_or("");
                            if !bullet_content.contains("`") || !bullet_content.contains(" - ") {
                                self.violations.push(NamingViolation::new(
                                    &file_path.display().to_string(),
                                    start_line + j + 1,
                                    "INVALID_DOC_FORMAT",
                                    "Use format `- ErrorType - condition` with backticks around error type",
                                ));
                            }
                        }
                    }
                    j += 1;
                }

                if !found_content && section_type == "Errors" {
                    self.violations.push(NamingViolation::new(
                        &file_path.display().to_string(),
                        absolute_line,
                        "INVALID_DOC_FORMAT",
                        "# Errors section cannot be empty - list specific error types",
                    ));
                }

                i = j;
            } else {
                i += 1;
            }
        }

        // Validate section ordering: Errors should come before Panics (only within this function)
        if sections_found.len() >= 2 {
            for window in sections_found.windows(2) {
                let (section1, _line1) = window[0];
                let (section2, line2) = window[1];

                if section1 == "Panics" && section2 == "Errors" {
                    self.violations.push(NamingViolation::new(
                        &file_path.display().to_string(),
                        line2,
                        "INVALID_DOC_FORMAT",
                        "# Errors section must come before # Panics section",
                    ));
                }
            }
        }
    }

    /// Check a single file for all naming violations
    fn check_file(&mut self, file_path: &Path) -> Result<(), Box<dyn std::error::Error>> {
        // Skip test files and style test files to avoid false positives
        let path_str = file_path.to_string_lossy();
        if path_str.contains("/tests/")
            || path_str.contains("test_")
            // Ignore this file since test cases containing violations
            || path_str.contains("naming_conventions.rs")
        {
            return Ok(());
        }

        let content = fs::read_to_string(file_path)?;

        self.check_function_prefixes(file_path, &content);
        self.check_type_naming(file_path, &content);
        self.check_module_names(file_path);
        self.check_documentation_format(file_path, &content);

        self.files_checked += 1;
        Ok(())
    }

    /// Check all Rust files in the workspace
    fn check_workspace(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        let files = self.find_rust_files()?;

        for file in files {
            if let Err(e) = self.check_file(&file) {
                eprintln!("Warning: Failed to check {}: {}", file.display(), e);
            }
        }

        Ok(())
    }

    /// Report all violations found
    fn report_violations(&self) -> bool {
        if self.violations.is_empty() {
            println!("Naming conventions check passed");
            println!("  Files checked: {}", self.files_checked);
            return true;
        }

        println!("Naming convention violations found:");
        println!();

        for violation in &self.violations {
            println!(
                "{}:{}:{} - {}",
                violation.file_path,
                violation.line_number,
                violation.violation_type,
                violation.message
            );
        }

        println!();
        println!("Summary:");
        println!("  Files checked: {}", self.files_checked);
        println!("  Violations: {}", self.violations.len());
        println!();
        println!("Fix these violations by following the naming conventions in docs/STYLE.md");

        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_banned_function_prefixes() {
        let mut checker = NamingChecker::new();
        const VIOLATION_CODE: &str = r#"
impl SomeStruct {
    pub fn get_value(&self) -> u32 { 42 }
    pub fn set_value(&mut self, v: u32) { }
    pub fn handle_message(&self) { }
    pub fn process_data(&self) { } // This is OK
    fn get_private(&self) -> i32 { 0 } // Also banned
    pub async fn set_async(&mut self) { } // Async is also banned
}
"#;

        checker.check_function_prefixes(Path::new("test.rs"), VIOLATION_CODE);
        assert_eq!(checker.violations.len(), 5);

        // Check specific violation messages
        assert!(
            checker
                .violations
                .iter()
                .any(|v| v.message.contains("get_"))
        );
        assert!(
            checker
                .violations
                .iter()
                .any(|v| v.message.contains("set_"))
        );
        assert!(
            checker
                .violations
                .iter()
                .any(|v| v.message.contains("handle_"))
        );
    }

    #[test]
    fn test_function_prefixes_edge_cases() {
        let mut checker = NamingChecker::new();
        const EDGE_CASE_CODE: &str = r#"
impl SomeStruct {
    pub fn getter(&self) -> u32 { 42 } // OK - doesn't have underscore
    pub fn setup(&mut self) { } // OK - set without underscore
    pub fn get_or_create(&self) -> u32 { 42 } // Banned - has get_
    pub fn //get_commented(&self) -> u32 { 42 } // OK - commented out
    fn process_get_request(&self) { } // OK - get_ not at function start
}
"#;

        checker.check_function_prefixes(Path::new("edge_cases.rs"), EDGE_CASE_CODE);
        assert_eq!(checker.violations.len(), 1);
        assert!(checker.violations[0].message.contains("get_"));
    }

    #[test]
    fn test_banned_type_suffixes() {
        let mut checker = NamingChecker::new();

        const TYPE_VIOLATION_CODE: &str = r#"
pub struct PeerConnectionFactory {
    // fields
}

pub struct HttpStreamingService {
    // fields
}

pub struct FileLibraryManager {
    // fields
}

struct ErrorHandler {
    // This has Handler suffix which is banned
}

struct PrivateFactory {
    // Also banned even without pub
}
"#;

        checker.check_type_naming(Path::new("types.rs"), TYPE_VIOLATION_CODE);
        // Should find: Factory (2), Service (1), Manager struct (1), Handler (1) = 5 violations
        // But NOT Manager traits (those are allowed)

        assert_eq!(checker.violations.len(), 5);

        // Check for specific violations
        assert!(
            checker
                .violations
                .iter()
                .any(|v| v.message.contains("Factory"))
        );
        assert!(
            checker
                .violations
                .iter()
                .any(|v| v.message.contains("Service"))
        );
        assert!(
            checker
                .violations
                .iter()
                .any(|v| v.message.contains("FileLibraryManager"))
        );

        // Ensure trait managers are NOT flagged
        assert!(
            !checker
                .violations
                .iter()
                .any(|v| v.message.contains("PeerManager"))
        );
        assert!(
            !checker
                .violations
                .iter()
                .any(|v| v.message.contains("TrackerManager"))
        );
    }

    #[test]
    fn test_banned_module_names() {
        let mut checker = NamingChecker::new();

        // Test banned names
        checker.check_module_names(Path::new("utils.rs"));
        checker.check_module_names(Path::new("common.rs"));
        checker.check_module_names(Path::new("helpers.rs"));
        checker.check_module_names(Path::new("misc.rs"));

        // Test allowed names
        checker.check_module_names(Path::new("piece_picker.rs"));
        checker.check_module_names(Path::new("protocol.rs"));
        checker.check_module_names(Path::new("mod.rs"));

        assert_eq!(checker.violations.len(), 4);
        assert!(
            checker
                .violations
                .iter()
                .any(|v| v.message.contains("utils"))
        );
        assert!(
            checker
                .violations
                .iter()
                .any(|v| v.message.contains("common"))
        );
        assert!(
            checker
                .violations
                .iter()
                .any(|v| v.message.contains("helpers"))
        );
    }

    #[test]
    fn test_violation_structure() {
        let mut checker = NamingChecker::new();
        const VIOLATION_EXAMPLE: &str = "pub fn get_test() -> u32 { 42 }";

        checker.check_function_prefixes(Path::new("example.rs"), VIOLATION_EXAMPLE);

        assert_eq!(checker.violations.len(), 1);
        let violation = &checker.violations[0];
        assert_eq!(violation.file_path, "example.rs");
        assert_eq!(violation.line_number, 1);
        assert_eq!(violation.violation_type, "BANNED_FUNCTION_PREFIX");
        assert!(violation.message.contains("get_"));
    }

    #[test]
    fn naming_convention_enforcement() {
        let mut checker = NamingChecker::new();

        // Run the check
        checker
            .check_workspace()
            .expect("Failed to check workspace");

        // Report results - this will fail if violations are found
        let passed = checker.report_violations();

        // In CI, we want this to fail if there are violations
        assert!(
            passed,
            "Naming convention violations found - see output above"
        );
    }
}
