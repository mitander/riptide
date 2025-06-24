//! Riptide Code Quality Enforcement
//!
//! Three-tiered mechanical enforcement of Riptide coding standards.
//! Based on project docs and real pain points from BitTorrent streaming development.
//!
//! ## Philosophy: Prevent Real Problems
//!
//! This enforces patterns that matter for streaming media servers:
//! - Performance-critical paths must not allocate  
//! - Network timeouts prevent hanging operations
//! - Clear naming prevents torrent/streaming confusion
//! - Proper error handling for network protocols
//!
//! **Level 1: Critical** - Fails CI, breaks streaming
//! **Level 2: Ratchet** - Prevents regression 
//! **Level 3: Guideline** - Human judgment

use std::fs;
use std::path::{Path, PathBuf};

use syn::spanned::Spanned;
use syn::visit::{self, Visit};
use syn::{
    Expr, FnArg, ItemFn, ItemStruct, Pat, PatType, Stmt, Type, TypePath, Visibility,
};

// --- Riptide-Specific Configuration ---

/// LEVEL 2: Max struct fields (smaller than ROHC due to streaming focus)
const STRUCT_FIELD_COUNT_MAX: usize = 15;

/// LEVEL 2: Max module size (prevents monolithic files)
const MODULE_SIZE_HIGH_WATER_MARK: usize = 500;

/// LEVEL 1: Forbidden marketing terms in documentation
const MARKETING_TERMS: &[&str] = &[
    "production-ready",
    "comprehensive", 
    "complete",
    "advanced",
    "intelligent", 
    "optimal",
    "powerful",
    "robust",
    "seamless",
    "cutting-edge",
    "state-of-the-art",
    "world-class",
    "enterprise",
];

/// LEVEL 1: Required abbreviation replacements in public APIs
const FORBIDDEN_ABBREVIATIONS: &[(&str, &str)] = &[
    ("ctx", "context"),
    ("seq", "sequence"), 
    ("msg", "message"),
    ("mgr", "manager"),
    ("hdlr", "handler"),
    ("buf", "buffer"),
];

// --- Violation Tracking ---

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Severity {
    Guideline, // Suggestion, printed but won't fail CI
    Ratchet,   // Regression prevention, fails CI
    Critical,  // Core rule violation, fails CI
}

#[derive(Debug)]
struct TidyViolation {
    severity: Severity,
    path: String,
    line: usize,
    message: String,
}

impl TidyViolation {
    fn new(severity: Severity, path: &Path, span: proc_macro2::Span, message: impl Into<String>) -> Self {
        // Extract line number from span - approximation since proc_macro2::Span doesn't expose line directly
        let line = 1; // Default to line 1 for now
        Self {
            severity,
            path: path.to_string_lossy().to_string(),
            line,
            message: message.into(),
        }
    }

    fn new_at_line(severity: Severity, path: &Path, line: usize, message: impl Into<String>) -> Self {
        Self {
            severity,
            path: path.to_string_lossy().to_string(),
            line,
            message: message.into(),
        }
    }
}

struct SourceFile<'a> {
    path: &'a Path,
    text: &'a str,
}

// --- AST Visitor for Riptide Rules ---

struct RiptideTidyVisitor<'a> {
    file: &'a SourceFile<'a>,
    is_test_context: bool,
    violations: Vec<TidyViolation>,
}

impl<'a> RiptideTidyVisitor<'a> {
    fn new(file: &'a SourceFile<'a>) -> Self {
        let path_str = file.path.to_string_lossy();
        let is_test_context = path_str.contains("/tests/")
            || path_str.ends_with("_test.rs")
            || path_str.contains("test_data")
            || path_str.contains("test_fixtures")
            || file.text.contains("#[cfg(test)]");

        Self {
            file,
            is_test_context,
            violations: Vec::new(),
        }
    }

    fn add_violation(&mut self, severity: Severity, span: proc_macro2::Span, message: impl Into<String>) {
        self.violations
            .push(TidyViolation::new(severity, self.file.path, span, message));
    }

    /// Check if a function is likely async I/O (should have timeout)
    fn is_network_function(func_name: &str) -> bool {
        func_name.contains("connect")
            || func_name.contains("fetch")
            || func_name.contains("announce")
            || func_name.contains("request")
            || func_name.contains("download")
    }

    /// Check if function signature indicates streaming hot path
    fn is_streaming_hot_path(item: &ItemFn) -> bool {
        let name = item.sig.ident.to_string();
        name.contains("stream") || name.contains("segment") || name.contains("read_piece")
    }
}

impl<'ast> Visit<'ast> for RiptideTidyVisitor<'_> {
    fn visit_item_fn(&mut self, item: &'ast ItemFn) {
        let is_public = matches!(item.vis, Visibility::Public(_));
        let func_name = item.sig.ident.to_string();

        if is_public && !self.is_test_context {
            // LEVEL 1: Public functions must be documented
            let has_doc = item.attrs.iter().any(|attr| attr.path().is_ident("doc"));
            if !has_doc {
                self.add_violation(
                    Severity::Critical,
                    item.sig.fn_token.span(),
                    format!("Public function '{}' missing documentation", func_name),
                );
            }

            // LEVEL 1: Public functions returning Result must document errors
            if let syn::ReturnType::Type(_, ty) = &item.sig.output {
                if let Type::Path(TypePath { path, .. }) = &**ty {
                    if path.segments.iter().any(|seg| seg.ident == "Result") {
                        let doc_text = item
                            .attrs
                            .iter()
                            .filter(|attr| attr.path().is_ident("doc"))
                            .map(|attr| format!("{:?}", attr.meta))
                            .collect::<String>();

                        if !doc_text.contains("# Errors") {
                            self.add_violation(
                                Severity::Critical,
                                item.sig.fn_token.span(),
                                format!(
                                    "Public function '{}' returns Result but missing '# Errors' documentation",
                                    func_name
                                ),
                            );
                        }
                    }
                }
            }

            // LEVEL 1: No abbreviations in public API function names
            for (abbrev, full) in FORBIDDEN_ABBREVIATIONS {
                if func_name.contains(abbrev) {
                    self.add_violation(
                        Severity::Critical,
                        item.sig.ident.span(),
                        format!(
                            "Public function name contains '{}', use '{}' instead",
                            abbrev, full
                        ),
                    );
                }
            }

            // LEVEL 1: No abbreviations in public API parameters
            for arg in &item.sig.inputs {
                if let FnArg::Typed(PatType { pat, .. }) = arg {
                    if let Pat::Ident(pat_ident) = &**pat {
                        let param_name = pat_ident.ident.to_string();
                        for (abbrev, full) in FORBIDDEN_ABBREVIATIONS {
                            if param_name == *abbrev {
                                self.add_violation(
                                    Severity::Critical,
                                    pat_ident.span(),
                                    format!(
                                        "Public parameter '{}' should be '{}'",
                                        abbrev, full
                                    ),
                                );
                            }
                        }
                    }
                }
            }
        }

        // LEVEL 1: Async network functions should have timeout handling
        if item.sig.asyncness.is_some() && Self::is_network_function(&func_name) && !self.is_test_context {
            let func_body_text = format!("{:?}", item.block);
            if !func_body_text.contains("timeout") && !func_body_text.contains("Duration") {
                self.add_violation(
                    Severity::Critical,
                    item.sig.fn_token.span(),
                    format!(
                        "Async network function '{}' should include timeout handling",
                        func_name
                    ),
                );
            }
        }

        // LEVEL 2: Streaming functions should not allocate in hot paths  
        if Self::is_streaming_hot_path(item) && !self.is_test_context {
            let func_body_text = format!("{:?}", item.block);
            if func_body_text.contains("Vec::new") 
                || func_body_text.contains("to_vec")
                || func_body_text.contains("to_string")
                || func_body_text.contains("format!") {
                self.add_violation(
                    Severity::Ratchet,
                    item.sig.fn_token.span(),
                    format!(
                        "Streaming function '{}' may allocate in hot path - consider pre-allocated buffers",
                        func_name
                    ),
                );
            }
        }

        visit::visit_item_fn(self, item);
    }

    fn visit_item_struct(&mut self, item: &'ast ItemStruct) {
        let is_public = matches!(item.vis, Visibility::Public(_));

        // LEVEL 2: Struct field count ratchet
        if item.fields.len() > STRUCT_FIELD_COUNT_MAX {
            self.add_violation(
                Severity::Ratchet,
                item.ident.span(),
                format!(
                    "Struct '{}' has {} fields, exceeding max {}. Consider decomposition.",
                    item.ident,
                    item.fields.len(),
                    STRUCT_FIELD_COUNT_MAX
                ),
            );
        }

        if is_public && !self.is_test_context {
            // LEVEL 1: Public structs must be documented
            let has_doc = item.attrs.iter().any(|attr| attr.path().is_ident("doc"));
            if !has_doc {
                self.add_violation(
                    Severity::Critical,
                    item.struct_token.span(),
                    format!("Public struct '{}' missing documentation", item.ident),
                );
            }

            // LEVEL 1: Check for marketing language in struct documentation
            let doc_text = item
                .attrs
                .iter()
                .filter(|attr| attr.path().is_ident("doc"))
                .map(|attr| format!("{:?}", attr.meta))
                .collect::<String>()
                .to_lowercase();

            for &term in MARKETING_TERMS {
                if doc_text.contains(term) {
                    self.add_violation(
                        Severity::Critical,
                        item.struct_token.span(),
                        format!(
                            "Struct '{}' documentation contains marketing term '{}' - use technical description",
                            item.ident, term
                        ),
                    );
                }
            }
        }

        visit::visit_item_struct(self, item);
    }

    fn visit_stmt(&mut self, stmt: &'ast Stmt) {
        if !self.is_test_context {
            // LEVEL 1: No .unwrap() without safety comment
            if let Some(_span) = find_unwrap_in_stmt(stmt) {
                // For simplicity, we'll check the entire statement text for safety comments
                let stmt_text = format!("{:?}", stmt);
                
                if !stmt_text.contains("// unwrap: safe because") 
                    && !stmt_text.contains("// unwrap: ")
                    && !stmt_text.contains("// SAFETY:") {
                    // Use span from the statement itself
                    self.add_violation(
                        Severity::Critical,
                        stmt.span(),
                        "Use proper error handling instead of .unwrap(), or document safety with comment",
                    );
                }
            }

            // LEVEL 1: No .expect() in production
            if let Some(_span) = find_expect_in_stmt(stmt) {
                self.add_violation(
                    Severity::Critical,
                    stmt.span(),
                    "Use structured error handling instead of .expect()",
                );
            }
        }

        visit::visit_stmt(self, stmt);
    }
}

/// Find .unwrap() calls in statements
fn find_unwrap_in_stmt(stmt: &Stmt) -> Option<proc_macro2::Span> {
    match stmt {
        Stmt::Expr(expr, _) => find_method_call_in_expr(expr, "unwrap"),
        _ => None,
    }
}

/// Find .expect() calls in statements  
fn find_expect_in_stmt(stmt: &Stmt) -> Option<proc_macro2::Span> {
    match stmt {
        Stmt::Expr(expr, _) => find_method_call_in_expr(expr, "expect"),
        _ => None,
    }
}

/// Recursively find method calls in expressions
fn find_method_call_in_expr(expr: &Expr, method_name: &str) -> Option<proc_macro2::Span> {
    match expr {
        Expr::MethodCall(method_call) => {
            if method_call.method == method_name {
                Some(method_call.method.span())
            } else {
                find_method_call_in_expr(&method_call.receiver, method_name)
            }
        }
        Expr::Call(call) => {
            for arg in &call.args {
                if let Some(span) = find_method_call_in_expr(arg, method_name) {
                    return Some(span);
                }
            }
            None
        }
        Expr::Await(await_expr) => find_method_call_in_expr(&await_expr.base, method_name),
        _ => None,
    }
}

// --- Standalone File-Level Checks ---

/// LEVEL 1: Check for marketing language in module documentation
fn check_marketing_language(file: &SourceFile, violations: &mut Vec<TidyViolation>) {
    for (i, line) in file.text.lines().enumerate() {
        if line.trim_start().starts_with("//!") {
            let line_lower = line.to_lowercase();
            for &term in MARKETING_TERMS {
                if line_lower.contains(term) {
                    violations.push(TidyViolation::new_at_line(
                        Severity::Critical,
                        file.path,
                        i + 1,
                        format!("Module doc contains marketing term '{}' - use technical description", term),
                    ));
                }
            }
        }
    }
}

/// LEVEL 1: Check for TODO comments (allowed but must be specific)
fn check_todo_quality(file: &SourceFile, violations: &mut Vec<TidyViolation>) {
    for (i, line) in file.text.lines().enumerate() {
        if line.contains("TODO") && !line.contains("TODO:") {
            violations.push(TidyViolation::new_at_line(
                Severity::Guideline,
                file.path,
                i + 1,
                "TODO should have format 'TODO: [scope] specific action'",
            ));
        }
        
        // Check for vague TODOs
        if line.contains("TODO:") && (
            line.to_lowercase().contains("fix this") ||
            line.to_lowercase().contains("clean up") ||
            line.to_lowercase().contains("improve") ||
            line.to_lowercase().contains("optimize")
        ) {
            violations.push(TidyViolation::new_at_line(
                Severity::Guideline,
                file.path,
                i + 1,
                "TODO is too vague - specify concrete action and scope",
            ));
        }
    }
}

/// LEVEL 1: Check for FIXME comments (not allowed)
fn check_no_fixme(file: &SourceFile, violations: &mut Vec<TidyViolation>) {
    for (i, line) in file.text.lines().enumerate() {
        if line.contains("FIXME") {
            violations.push(TidyViolation::new_at_line(
                Severity::Critical,
                file.path,
                i + 1,
                "FIXME comments not allowed - fix the issue or create a GitHub issue",
            ));
        }
    }
}

/// LEVEL 1: Check for inline module references (violates import conventions)
fn check_inline_module_refs(file: &SourceFile, violations: &mut Vec<TidyViolation>) {
    for (i, line) in file.text.lines().enumerate() {
        // Skip test modules
        if file.path.to_string_lossy().contains("test") {
            continue;
        }
        
        // Check for crate::module::function pattern
        if line.contains("crate::") && line.matches("::").count() >= 2 {
            // Allow test_data and test_fixtures 
            if !line.contains("test_data") && !line.contains("test_fixtures") {
                violations.push(TidyViolation::new_at_line(
                    Severity::Critical,
                    file.path,
                    i + 1,
                    "Use proper imports instead of inline module references",
                ));
            }
        }
    }
}

// --- Project-Level Checks ---

/// LEVEL 1: Anti-pattern module detection
fn check_anti_pattern_modules() -> Result<(), TidyViolation> {
    const ANTI_PATTERNS: &[&str] = &[
        "utils.rs", "helpers.rs", "misc.rs", "common.rs", "shared.rs"
    ];
    
    for entry in walkdir::WalkDir::new("src").into_iter().filter_map(Result::ok) {
        let file_name = entry.file_name().to_string_lossy();
        if ANTI_PATTERNS.iter().any(|&pattern| pattern == file_name) {
            return Err(TidyViolation {
                severity: Severity::Critical,
                path: entry.path().display().to_string(),
                line: 0,
                message: format!(
                    "Anti-pattern module '{}' found - use domain-specific names",
                    file_name
                ),
            });
        }
    }
    Ok(())
}

// --- Main Entry Points ---

fn list_rust_files() -> Vec<PathBuf> {
    walkdir::WalkDir::new("src")
        .into_iter()
        .filter_map(Result::ok)
        .filter(|e| {
            e.file_type().is_file() 
                && e.path().extension().map_or(false, |ext| ext == "rs")
                && !e.path().to_string_lossy().contains("tidy.rs")
        })
        .map(|e| e.path().to_path_buf())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn riptide_tidy_main() {
        let mut all_violations = Vec::new();
        let mut max_module_loc = 0;

        // --- Phase 1: File-by-file analysis ---
        for path in &list_rust_files() {
            let source_text = fs::read_to_string(path).expect("Failed to read file");
            let file = SourceFile {
                path,
                text: &source_text,
            };
            let line_count = source_text.lines().count();

            if line_count > max_module_loc {
                max_module_loc = line_count;
            }

            // Run file-level checks
            check_marketing_language(&file, &mut all_violations);
            check_todo_quality(&file, &mut all_violations);
            check_no_fixme(&file, &mut all_violations);
            check_inline_module_refs(&file, &mut all_violations);

            // Run AST-based checks
            match syn::parse_file(&source_text) {
                Ok(ast) => {
                    let mut visitor = RiptideTidyVisitor::new(&file);
                    visitor.visit_file(&ast);
                    all_violations.extend(visitor.violations);
                }
                Err(e) => {
                    all_violations.push(TidyViolation {
                        severity: Severity::Critical,
                        path: path.to_string_lossy().to_string(),
                        line: 0,
                        message: format!("Failed to parse file: {}", e),
                    });
                }
            }
        }

        // --- Phase 2: Project-wide checks ---
        if let Err(violation) = check_anti_pattern_modules() {
            all_violations.push(violation);
        }

        // LEVEL 2: Module size ratchet
        if max_module_loc > MODULE_SIZE_HIGH_WATER_MARK {
            all_violations.push(TidyViolation {
                severity: Severity::Ratchet,
                path: "Project-wide".to_string(),
                line: 0,
                message: format!(
                    "Module grown to {} lines, exceeding limit of {}. Refactor or update limit.",
                    max_module_loc, MODULE_SIZE_HIGH_WATER_MARK
                ),
            });
        }

        // --- Phase 3: Report results ---
        if all_violations.is_empty() {
            return; // Success!
        }

        // Sort violations for stable output
        all_violations.sort_by_key(|v| (v.severity, v.path.clone(), v.line));

        let guidelines: Vec<_> = all_violations
            .iter()
            .filter(|v| v.severity == Severity::Guideline)
            .collect();
        let failures: Vec<_> = all_violations
            .iter()
            .filter(|v| v.severity > Severity::Guideline)
            .collect();

        // Print guidelines but don't fail
        if !guidelines.is_empty() {
            eprintln!("\n--- Riptide Style Guidelines (Warnings) ---");
            for v in guidelines {
                eprintln!("[GUIDELINE] {}:{}: {}", v.path, v.line, v.message);
            }
        }

        // Fail on critical issues and ratchets
        if !failures.is_empty() {
            panic!(
                "\n--- Riptide Standards Violations ---\n{}\n\nRun ./scripts/check_standards.sh for more details.",
                failures
                    .iter()
                    .map(|v| format!("[{:?}] {}:{}: {}", v.severity, v.path, v.line, v.message))
                    .collect::<Vec<_>>()
                    .join("\n")
            );
        }
    }

    /// Coverage report of what this tidy suite enforces
    #[test]
    fn tidy_coverage_report() {
        println!("\n--- Riptide Tidy Coverage Report ---");
        println!("Mechanical enforcement of Riptide streaming media standards.\n");
        
        println!("CRITICAL RULES (Fail CI):");
        println!("  [✓] No marketing language in documentation");
        println!("  [✓] No .unwrap()/.expect() without safety documentation");
        println!("  [✓] No anti-pattern modules (utils.rs, helpers.rs)");
        println!("  [✓] Public APIs fully documented with # Errors for Result types");
        println!("  [✓] No abbreviations in public API names (ctx → context)");
        println!("  [✓] No inline module references (use proper imports)");
        println!("  [✓] Network functions must handle timeouts");
        println!("  [✓] No FIXME comments (fix or create issue)");
        
        println!("\nREGRESSION PREVENTION (Ratchets):");
        println!("  [✓] Module size ≤ {} lines", MODULE_SIZE_HIGH_WATER_MARK);
        println!("  [✓] Struct fields ≤ {} (encourage composition)", STRUCT_FIELD_COUNT_MAX);
        println!("  [✓] Streaming hot paths avoid allocation");
        
        println!("\nSTYLE GUIDELINES (Human Judgment):");
        println!("  [✓] TODO format: 'TODO: [scope] specific action'");
        println!("  [✓] Avoid vague TODOs ('fix this', 'clean up')");
        
        println!("\nThis enforces patterns critical for streaming media servers.");
        println!("Human reviewers focus on protocol correctness and architecture.");
    }
}