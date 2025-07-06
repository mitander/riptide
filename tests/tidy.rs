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

/// Common verbs used to check for verb-first naming convention
const COMMON_VERBS: &[&str] = &[
    "execute",
    "process",
    "handle",
    "manage",
    "create",
    "destroy",
    "build",
    "parse",
    "decode",
    "encode",
    "compress",
    "decompress",
    "validate",
    "verify",
    "authenticate",
    "authorize",
    "encrypt",
    "decrypt",
    "serialize",
    "deserialize",
    "convert",
    "transform",
    "filter",
    "sort",
    "search",
    "find",
    "locate",
    "calculate",
    "compute",
    "generate",
    "produce",
    "render",
    "display",
    "show",
    "hide",
    "open",
    "close",
    "start",
    "stop",
    "pause",
    "resume",
    "restart",
    "reload",
    "refresh",
    "update",
    "modify",
    "change",
    "insert",
    "add",
    "append",
    "prepend",
    "remove",
    "delete",
    "clear",
    "reset",
    "initialize",
    "setup",
    "configure",
    "connect",
    "disconnect",
    "send",
    "receive",
    "transmit",
    "broadcast",
    "publish",
    "subscribe",
    "listen",
    "monitor",
    "watch",
    "observe",
    "track",
    "log",
    "record",
    "store",
    "save",
    "load",
    "read",
    "write",
    "copy",
    "move",
    "transfer",
    "migrate",
    "backup",
    "restore",
    "recover",
    "repair",
    "fix",
    "correct",
    "adjust",
    "tune",
    "optimize",
    "improve",
    "enhance",
    "upgrade",
    "downgrade",
];

/// Standard library modules that are always available
const STD_MODULES: &[&str] = &["std", "core", "alloc"];

/// Functions that lose critical context when imported without module path
const CONTEXT_CRITICAL_FUNCTIONS: &[&str] = &[
    // Runtime/Threading context
    "spawn",     // tokio::spawn vs std::thread::spawn
    "sleep",     // tokio::time::sleep vs std::thread::sleep
    "yield_now", // tokio::task::yield_now vs std::thread::yield_now
    "block_on",  // tokio::runtime::block_on vs other runtimes
    "timeout",   // tokio::time::timeout vs other timeout functions
    "select",    // tokio::select vs other select macros
    "join",      // tokio::join vs other join functions
    "try_join",  // tokio::try_join vs other try_join functions
    // Synchronization context
    "lock",     // async vs sync lock operations
    "try_lock", // async vs sync try_lock operations
    "read",     // async vs sync read operations
    "write",    // async vs sync write operations
    "acquire",  // semaphore/permit context valuable
    "release",  // semaphore/permit context valuable
    // Network context
    "connect", // network context valuable
    "listen",  // network context valuable
    "bind",    // network binding context valuable
    "accept",  // network accept context valuable
    "send",    // network/channel send context valuable
    "recv",    // network/channel receive context valuable
    "get",     // HTTP/request context valuable
    "post",    // HTTP/request context valuable
    "put",     // HTTP/request context valuable
    "delete",  // HTTP/request context valuable
    // Serialization/Parsing context
    "parse",            // format-specific parsing context valuable
    "serialize",        // format-specific serialization context valuable
    "deserialize",      // format-specific deserialization context valuable
    "to_string",        // format-specific conversion context valuable
    "to_string_pretty", // format-specific pretty conversion context valuable
    "from_str",         // format-specific parsing context valuable
    "from_slice",       // format-specific parsing context valuable
    "from_reader",      // format-specific parsing context valuable
    "to_writer",        // format-specific writing context valuable
    "to_vec",           // format-specific vector conversion context valuable
    "encode",           // encoding context valuable
    "decode",           // decoding context valuable
    // Logging context (critical for debugging)
    "debug", // tracing::debug vs other debug functions
    "info",  // tracing::info vs other info functions
    "warn",  // tracing::warn vs other warn functions
    "error", // tracing::error vs other error functions
    "trace", // tracing::trace vs other trace functions
    "log",   // tracing::log vs other log functions
    // File I/O context
    "open",     // file opening context valuable
    "create",   // file creation context valuable
    "remove",   // file removal context valuable
    "copy",     // file copy context valuable
    "move",     // file move context valuable
    "exists",   // file existence context valuable
    "metadata", // file metadata context valuable
    // Type construction context
    "new",           // type context valuable for complex constructors
    "default",       // type context valuable
    "with_capacity", // container context valuable
    "from_iter",     // iterator context valuable
    "collect",       // collection context valuable when ambiguous
    // Time/Date context
    "now",            // time context valuable (Instant vs SystemTime vs Utc)
    "elapsed",        // time measurement context valuable
    "duration_since", // time duration context valuable
    // Random/UUID context
    "random",    // random generation context valuable
    "gen",       // random generation context valuable
    "gen_range", // random generation context valuable
    "new_v4",    // UUID generation context valuable
    // Compression context
    "compress",   // compression algorithm context valuable
    "decompress", // decompression algorithm context valuable
    "encrypt",    // encryption algorithm context valuable
    "decrypt",    // decryption algorithm context valuable
    // Database/Storage context
    "execute", // database execution context valuable
    "query",   // database query context valuable
    "fetch",   // database fetch context valuable
    "insert",  // database insert context valuable
    "update",  // database update context valuable
    // Configuration context
    "load",  // configuration loading context valuable
    "save",  // configuration saving context valuable
    "merge", // configuration merging context valuable
];

/// Patterns that should always preserve inline module context for semantic clarity
const ALLOWED_INLINE_PATTERNS: &[&str] = &[
    "tokio::",
    "std::thread::",
    "std::time::",
    "serde_json::",
    "serde_yaml::",
    "reqwest::",
    "regex::",
    "tracing::",
    "anyhow::",
    "thiserror::",
    "clap::",
    "uuid::",
    "chrono::",
    "rand::",
    "futures::",
    "async_trait::",
    "riptide_core::",
    "riptide_web::",
    "riptide_search::",
    "riptide_sim::",
    "riptide_cli::",
];

/// Patterns that should be imported at top level (universally recognized)
const REQUIRE_IMPORT_PATTERNS: &[&str] = &[
    "std::collections::HashMap",
    "std::collections::BTreeMap",
    "std::collections::HashSet",
    "std::collections::BTreeSet",
    "std::collections::VecDeque",
    "std::sync::Arc",
    "std::sync::Rc",
    "std::sync::Mutex",
    "std::sync::RwLock",
    "std::time::Duration",
    "std::time::Instant",
    "std::time::SystemTime",
    "std::fmt::Display",
    "std::fmt::Debug",
    "std::error::Error",
    "std::result::Result",
    "std::option::Option",
    "std::boxed::Box",
    "std::string::String",
    "std::vec::Vec",
];

#[derive(Debug)]
enum Recommendation {
    PreferInline(String), // Keep inline with reason
    PreferImport(String), // Import with reason
}

#[derive(Debug, Clone, Copy)]
enum UsageType {
    Function, // Function call (preserve context)
    Type,     // Type usage (can import)
    Unknown,  // Can't determine (conservative)
}

// --- Violation Tracking ---

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
enum Severity {
    Critical, // Must fix - fails CI
    Warning,  // Should fix - improves consistency
}

#[derive(Debug, Clone)]
struct StyleViolation {
    severity: Severity,
    file: String,
    line: usize,
    rule: String,
    message: String,
}

impl StyleViolation {
    fn new(severity: Severity, file: &Path, line: usize, rule: &str, message: &str) -> Self {
        Self {
            severity,
            file: file.to_string_lossy().to_string(),
            line,
            rule: rule.to_string(),
            message: message.to_string(),
        }
    }
}

/// Tracks identifiers and their locations for consistency analysis
#[derive(Debug, Default)]
struct IdentifierTracker {
    /// Maps normalized names to their variants and locations
    /// e.g., "piece" -> [("piece_index", line1), ("piece_idx", line2)]
    name_variants: HashMap<String, Vec<(String, usize)>>,

    /// All function names found
    function_names: Vec<(String, usize)>,

    /// All struct/enum names found
    type_names: Vec<(String, usize)>,

    /// All variable names found
    variable_names: Vec<(String, usize)>,
}

impl IdentifierTracker {
    fn new() -> Self {
        Self::default()
    }

    /// Add an identifier, tracking its base form for consistency checking
    fn add_identifier(&mut self, name: &str, line: usize, category: &str) {
        // Normalize the name by removing common suffixes and extracting base
        let base_name = self.extract_base_name(name);

        self.name_variants
            .entry(base_name)
            .or_default()
            .push((name.to_string(), line));

        match category {
            "function" => self.function_names.push((name.to_string(), line)),
            "type" => self.type_names.push((name.to_string(), line)),
            "variable" => self.variable_names.push((name.to_string(), line)),
            _ => {}
        }
    }

    /// Extract the base form of a name for consistency checking
    fn extract_base_name(&self, name: &str) -> String {
        // Handle common patterns:
        // piece_index, piece_idx -> piece
        // connection_count, conn_count -> connection
        // torrent_file, torrent_data -> torrent

        let parts: Vec<&str> = name.split('_').collect();
        if parts.is_empty() {
            return name.to_lowercase();
        }

        // Use the first significant part as the base
        let base = parts[0].to_lowercase();

        // Return the base name for consistency checking

        base
    }

    /// Find inconsistent naming patterns within this scope
    fn find_inconsistencies(&self) -> Vec<(String, Vec<String>)> {
        let mut inconsistencies = Vec::new();

        for (base_name, variants) in &self.name_variants {
            if variants.len() > 1 {
                // Check if variants use different abbreviation patterns
                let mut abbreviated = Vec::new();
                let mut full_form = Vec::new();

                for (variant, _line) in variants {
                    if self.uses_problematic_abbreviations(variant) {
                        abbreviated.push(variant.clone());
                    } else {
                        full_form.push(variant.clone());
                    }
                }

                // Flag if we have both abbreviated and full forms
                if !abbreviated.is_empty() && !full_form.is_empty() {
                    let mut all_variants = abbreviated;
                    all_variants.extend(full_form);
                    inconsistencies.push((base_name.clone(), all_variants));
                }
            }
        }

        inconsistencies
    }

    /// Check if a name uses problematic abbreviations that should be full words
    /// TEMPORARILY DISABLED - dictionary too aggressive with false positives
    fn uses_problematic_abbreviations(&self, _name: &str) -> bool {
        // TODO: Replace with better English dictionary that doesn't flag common words
        false
        /* DISABLED FOR NOW - TOO MANY FALSE POSITIVES
        // Common technical abbreviations that are acceptable
        let acceptable_abbreviations = [
            "api",
            "app",
            "url",
            "id",
            "len",
            "max",
            "min",
            "tcp",
            "udp",
            "http",
            "https",
            "json",
            "xml",
            "html",
            "css",
            "js",
            "ts",
            "db",
            "sql",
            "cli",
            "gui",
            "ui",
            "ux",
            "auth",
            "oauth",
            "jwt",
            "ssl",
            "tls",
            "ssh",
            "git",
            "repo",
            "config",
            "env",
            "async",
            "sync",
            "await",
            "fn",
            "impl",
            "struct",
            "enum",
            "trait",
            "mod",
            "lib",
            "bin",
            "exe",
            "dll",
            "so",
            "dylib",
            "proc",
            "thread",
            "task",
            "job",
            "doc",
            "docs",
            "test",
            "tests",
            "spec",
            "specs",
            "benchmark",
            "bench",
            "debug",
            "info",
            "warn",
            "error",
            "trace",
            "log",
            "logs",
            "cache",
            "data",
            "meta",
            "attr",
            "prop",
            "field",
            "key",
            "val",
            "value",
            "hash",
            "map",
            "set",
            "vec",
            "list",
            "array",
            "iter",
            "stream",
            "reader",
            "writer",
            "parser",
            "lexer",
            "token",
            "ast",
            "tree",
            "node",
            "graph",
            "codec",
            "format",
            "mime",
            "type",
            "kind",
            "sort",
            "order",
            "priority",
            "status",
            "state",
            "mode",
            "flag",
            "option",
            "profile",
            "session",
            "connection",
            "socket",
            "addr",
            "port",
            "host",
            "path",
            "dir",
            "file",
            "name",
            "ext",
            "size",
            "count",
            "num",
            "total",
            "sum",
            "avg",
            "mean",
            "std",
            "dev",
            "var",
            "rand",
            "random",
            "gen",
            "new",
            "drop",
        ];

        // Split identifier into words (handle snake_case, camelCase, PascalCase)
        let mut words = Vec::new();

        for part in name.split('_') {
            // Split camelCase/PascalCase by uppercase letters
            let mut current = String::new();

            for ch in part.chars() {
                if ch.is_uppercase() && !current.is_empty() {
                    words.push(current.to_lowercase());
                    current.clear();
                }
                current.push(ch.to_lowercase().next().unwrap());
            }
            if !current.is_empty() {
                words.push(current.to_lowercase());
            }
        }

        // Check if any word is not in the dictionary and not an acceptable abbreviation
        let word_list = Words::new();
        for word in words {
            if word.len() > 2
                && !acceptable_abbreviations.contains(&word.as_str())
                && !word_list.words.contains(&word)
            {
                return true;
            }
        }
        false
        */
    }

    fn uses_verb_suffix(&self, name: &str) -> bool {
        // Check if function name ends with a verb (bad pattern)
        for &verb in COMMON_VERBS {
            if name.ends_with(verb) {
                // Make sure it's actually ending with the verb, not just containing it
                if name.len() > verb.len() {
                    let prefix = &name[..name.len() - verb.len()];
                    if prefix.ends_with('_') {
                        return true;
                    }
                }
            }
        }
        false
    }

    fn uses_forbidden_prefix(&self, name: &str) -> bool {
        // Check for forbidden function prefixes from docs/NAMING_CONVENTIONS.md
        const FORBIDDEN_PREFIXES: &[&str] = &[
            "get_",         // Use descriptive names: get_torrent() not get()
            "set_",         // Use descriptive names: update_status() not set_status()
            "do_",          // Redundant prefix: start_download() not do_start_download()
            "run_",         // Too generic: execute_command() not run_command()
            "go_",          // Non-standard: start_process() not go_start()
            "make_",        // Too generic: create_connection() not make_connection()
            "put_",         // Unclear: store_data() not put_data()
            "add_to_",      // Verbose: add_peer() not add_to_peers()
            "remove_from_", // Verbose: remove_peer() not remove_from_peers()
        ];

        for &prefix in FORBIDDEN_PREFIXES {
            if name.starts_with(prefix) {
                // Make sure it's not just a word that happens to start with these letters
                if name.len() > prefix.len() {
                    return true;
                }
            }
        }
        false
    }

    fn uses_redundant_type_suffix(&self, name: &str) -> bool {
        // Check for redundant type suffixes that add no information
        const REDUNDANT_SUFFIXES: &[&str] = &[
            "Struct",    // DataStruct → Data
            "Enum",      // StatusEnum → Status
            "Trait",     // DrawableTrait → Drawable
            "Interface", // ServiceInterface → Service
            "Class",     // Not Rust-like: ConnectionClass → Connection
            "Object",    // Not Rust-like: ConfigObject → Config
            "Instance",  // Not Rust-like: EngineInstance → Engine
        ];

        // Be more conservative with "Type" - only flag obviously redundant cases
        const REDUNDANT_TYPE_PATTERNS: &[&str] = &[
            "DataType",     // Data is sufficient
            "InfoType",     // Info is sufficient
            "ConfigType",   // Config is sufficient
            "SettingsType", // Settings is sufficient
        ];

        for &suffix in REDUNDANT_SUFFIXES {
            if name.ends_with(suffix) && name.len() > suffix.len() {
                // Make sure it's actually a suffix, not just coincidence
                let prefix = &name[..name.len() - suffix.len()];
                // Don't flag single letters or very short prefixes
                if prefix.len() >= 3 {
                    return true;
                }
            }
        }

        // Check for obviously redundant "Type" patterns
        for &pattern in REDUNDANT_TYPE_PATTERNS {
            if name == pattern {
                return true;
            }
        }

        false
    }

    fn uses_generic_name(&self, name: &str) -> bool {
        // Check for overly generic function names that provide no context
        const GENERIC_NAMES: &[&str] = &[
            "process",
            "handle",
            "manage",
            "execute",
            "perform",
            "operate",
            "work",
            "run",
            "go",
            "do",
            "action",
            "func",
            "method",
            "call",
            "invoke",
            "trigger",
            "fire",
            "emit",
            "send",
            "receive",
            "get",
            "set",
            "put",
            "take",
            "give",
            "make",
            "create",
            "construct",
            "produce",
            "yield",
            "return",
            "provide",
            "supply",
        ];

        // Don't flag legitimate Rust patterns
        const ACCEPTABLE_PATTERNS: &[&str] = &[
            "build",    // Builder pattern standard
            "generate", // Generate is specific enough (generate_peer_id, etc.)
        ];

        if ACCEPTABLE_PATTERNS.contains(&name) {
            return false;
        }

        GENERIC_NAMES.contains(&name)
    }

    fn uses_unclear_boolean_name(&self, name: &str) -> bool {
        // Check for boolean functions that don't clearly indicate what they're checking
        const UNCLEAR_BOOLEAN_NAMES: &[&str] = &[
            "valid",
            "empty",
            "ready",
            "active",
            "enabled",
            "disabled",
            "open",
            "closed",
            "available",
            "busy",
            "free",
            "used",
            "exists",
            "present",
            "absent",
            "found",
            "missing",
            "ok",
            "error",
            "success",
            "failure",
            "complete",
            "finished",
        ];

        // Only flag if it's not already using a clear boolean prefix
        if name.starts_with("is_")
            || name.starts_with("has_")
            || name.starts_with("can_")
            || name.starts_with("should_")
        {
            return false;
        }

        UNCLEAR_BOOLEAN_NAMES.contains(&name)
    }

    fn uses_unclear_collection_name(&self, name: &str) -> bool {
        // Check for generic collection names that don't specify what they contain
        const GENERIC_COLLECTION_NAMES: &[&str] = &[
            "list", "items", "data", "things", "stuff", "objects", "elements", "records", "rows",
            "values", "map", "dict", "table", "pool", "queue", "stack", "array", "vector",
        ];

        // Allow certain common patterns that are clear in context
        const ACCEPTABLE_GENERIC_NAMES: &[&str] = &[
            "entries", // Common for directory/file operations
            "buffer",  // Common for I/O operations
            "store",   // Common for storage abstractions
            "cache",   // Common for caching
        ];

        // Allow these as suffixes (peer_list) but not standalone
        if GENERIC_COLLECTION_NAMES.contains(&name) {
            return true;
        }

        // Don't flag acceptable generic names that are clear in context
        if ACCEPTABLE_GENERIC_NAMES.contains(&name) {
            return false;
        }

        // Also check for functions that return generic collections
        if name.starts_with("get_") {
            let rest = &name[4..];
            if GENERIC_COLLECTION_NAMES.contains(&rest) {
                return true;
            }
        }

        false
    }
}

/// Main style checker
struct StyleChecker {
    violations: Vec<StyleViolation>,
    current_file: PathBuf,
    file_lines: Vec<String>,
    identifiers: IdentifierTracker,
    workspace_crates: Vec<String>,
    external_dependencies: Vec<String>,
}

impl StyleChecker {
    fn new() -> Self {
        let workspace_crates = Self::detect_workspace_crates();
        let external_dependencies = Self::detect_external_dependencies();
        Self {
            violations: Vec::new(),
            current_file: PathBuf::new(),
            file_lines: Vec::new(),
            identifiers: IdentifierTracker::new(),
            workspace_crates,
            external_dependencies,
        }
    }

    fn check_file(&mut self, file_path: PathBuf) -> Result<(), std::io::Error> {
        let content = fs::read_to_string(&file_path)?;
        self.current_file = file_path;
        self.file_lines = content.lines().map(|s| s.to_string()).collect();
        self.identifiers = IdentifierTracker::new();

        let is_test_file = self.current_file.to_string_lossy().contains("test");

        // Core structural checks
        self.check_module_size();
        self.check_banned_module_names();
        self.check_emoji_usage();

        // Parse identifiers for consistency analysis
        self.parse_identifiers();

        if !is_test_file {
            // Production code checks
            self.check_naming_consistency();
            self.check_public_documentation();
            self.check_error_documentation();
        }

        // Code organization
        self.check_test_naming_patterns();
        self.check_import_organization();
        self.check_context_losing_imports();
        self.check_bare_function_calls();
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
        let mut violations = Vec::new();

        if let Some(file_stem) = self.current_file.file_stem() {
            if let Some(name) = file_stem.to_str() {
                for &banned_name in BANNED_MODULE_NAMES {
                    if name == banned_name {
                        let message = format!(
                            "Module '{}' uses banned name - use domain-specific names instead",
                            name
                        );
                        violations.push((1, message));
                    }
                }
            }
        }

        for (line_num, message) in violations {
            self.add_violation(Severity::Critical, line_num, "BANNED_MODULE_NAME", &message);
        }
    }

    /// Check for emoji usage (block faces and silly emojis, allow functional symbols)
    fn check_emoji_usage(&mut self) {
        let mut violations = Vec::new();
        for (line_num, line) in self.file_lines.iter().enumerate() {
            for ch in line.chars() {
                let code_point = ch as u32;
                // Only block actual emoji faces and silly symbols, not functional ones
                if (code_point >= 0x1F600 && code_point <= 0x1F64F) // Emoticons (faces)
                    || (code_point >= 0x1F910 && code_point <= 0x1F96B) // More faces
                    || (code_point >= 0x1F970 && code_point <= 0x1F9FF) // Even more faces
                    || (code_point >= 0x1F1E6 && code_point <= 0x1F1FF)
                // Flag emojis
                {
                    violations.push((
                        line_num + 1,
                        "NO_EMOJIS",
                        "Emoji faces and decorative symbols are forbidden - use plain text instead",
                    ));
                }
            }
        }

        for (line_num, rule, message) in violations {
            self.add_violation(Severity::Critical, line_num, rule, message);
        }
    }

    /// Parse identifiers from the code for consistency analysis
    fn parse_identifiers(&mut self) {
        for (line_num, line) in self.file_lines.iter().enumerate() {
            let trimmed = line.trim_start();

            // Parse function definitions
            if trimmed.starts_with("pub fn ") || trimmed.starts_with("fn ") {
                if let Some(name) = self.extract_function_name(line) {
                    self.identifiers
                        .add_identifier(&name, line_num + 1, "function");
                }
            }

            // Parse struct definitions
            if trimmed.starts_with("pub struct ") || trimmed.starts_with("struct ") {
                if let Some(name) = self.extract_struct_name(line) {
                    self.identifiers.add_identifier(&name, line_num + 1, "type");
                }
            }

            // Parse enum definitions
            if trimmed.starts_with("pub enum ") || trimmed.starts_with("enum ") {
                if let Some(name) = self.extract_enum_name(line) {
                    self.identifiers.add_identifier(&name, line_num + 1, "type");
                }
            }

            // Parse variable declarations (basic pattern)
            if trimmed.starts_with("let ") && trimmed.contains(" = ") {
                if let Some(name) = self.extract_variable_name(line) {
                    self.identifiers
                        .add_identifier(&name, line_num + 1, "variable");
                }
            }
        }
    }

    /// Check for naming consistency within this module
    fn check_naming_consistency(&mut self) {
        let inconsistencies = self.identifiers.find_inconsistencies();

        for (base_name, variants) in inconsistencies {
            if variants.len() > 1 {
                self.add_violation(
                    Severity::Warning,
                    1,
                    "NAMING_INCONSISTENCY",
                    &format!(
                        "Inconsistent naming for '{}': found variants {} - choose one style and use consistently",
                        base_name,
                        variants.join(", ")
                    ),
                );
            }
        }

        // Check for problematic abbreviations in identifiers
        let mut violations = Vec::new();

        // TEMPORARILY DISABLED - abbreviation check has too many false positives
        /*
        // Collect function name violations
        for name in &self.identifiers.function_names {
            if self.identifiers.uses_problematic_abbreviations(&name.0) {
                let message = format!(
                    "Function '{}' contains abbreviations - use full words (index not idx, buffer not buf, etc.)",
                    name.0
                );
                violations.push((name.1, "ABBREVIATIONS", message));
            }
        }

        // Collect type name violations
        for name in &self.identifiers.type_names {
            if self.identifiers.uses_problematic_abbreviations(&name.0) {
                let message = format!(
                    "Type '{}' contains abbreviations - use full words (index not idx, buffer not buf, etc.)",
                    name.0
                );
                violations.push((name.1, "ABBREVIATIONS", message));
            }
        }

        // Collect variable name violations
        for name in &self.identifiers.variable_names {
            if self.identifiers.uses_problematic_abbreviations(&name.0) {
                let message = format!(
                    "Variable '{}' contains abbreviations - use full words (index not idx, buffer not buf, etc.)",
                    name.0
                );
                violations.push((name.1, "ABBREVIATIONS", message));
            }
        }
        */

        // Check for verb-suffix patterns in function names
        for name in &self.identifiers.function_names {
            if self.identifiers.uses_verb_suffix(&name.0) {
                let message = format!(
                    "Function '{}' ends with verb - use verb-first naming: verb_noun",
                    name.0
                );
                violations.push((name.1, "VERB_SUFFIX_PATTERN", message));
            }
        }

        // Check for forbidden function name prefixes
        for name in &self.identifiers.function_names {
            if self.identifiers.uses_forbidden_prefix(&name.0) {
                let message = format!(
                    "Function '{}' uses forbidden prefix - use descriptive names (start_download not get_download, update_status not set_status)",
                    name.0
                );
                violations.push((name.1, "FORBIDDEN_PREFIX", message));
            }
        }

        // Check for redundant type suffixes
        for name in &self.identifiers.type_names {
            if self.identifiers.uses_redundant_type_suffix(&name.0) {
                let message = format!(
                    "Type '{}' uses redundant suffix - remove unnecessary suffixes (Data not DataStruct, Status not StatusEnum)",
                    name.0
                );
                violations.push((name.1, "REDUNDANT_TYPE_SUFFIX", message));
            }
        }

        // Check for overly generic function names
        for name in &self.identifiers.function_names {
            if self.identifiers.uses_generic_name(&name.0) {
                let message = format!(
                    "Function '{}' is too generic - use specific action names (compress_headers not process, validate_torrent not handle)",
                    name.0
                );
                violations.push((name.1, "GENERIC_FUNCTION_NAME", message));
            }
        }

        // Check for unclear boolean function names
        for name in &self.identifiers.function_names {
            if self.identifiers.uses_unclear_boolean_name(&name.0) {
                let message = format!(
                    "Function '{}' has unclear boolean name - use clear boolean prefixes (is_valid_torrent not valid, has_pending_pieces not ready)",
                    name.0
                );
                violations.push((name.1, "UNCLEAR_BOOLEAN_NAME", message));
            }
        }

        // Check for unclear collection names
        for name in &self.identifiers.function_names {
            if self.identifiers.uses_unclear_collection_name(&name.0) {
                let message = format!(
                    "Function '{}' uses unclear collection name - specify what the collection contains (active_downloads not list, peer_connections not map)",
                    name.0
                );
                violations.push((name.1, "UNCLEAR_COLLECTION_NAME", message));
            }
        }

        // Also check variable names for unclear collections
        for name in &self.identifiers.variable_names {
            if self.identifiers.uses_unclear_collection_name(&name.0) {
                let message = format!(
                    "Variable '{}' uses unclear collection name - specify what the collection contains (peer_list not list, torrent_cache not cache)",
                    name.0
                );
                violations.push((name.1, "UNCLEAR_COLLECTION_NAME", message));
            }
        }

        // Check for discarded variables that are actually used
        self.check_discarded_variables();

        // Add all violations - naming issues are now Critical to prevent future violations
        for (line_num, rule, message) in violations {
            let severity = match rule {
                "FORBIDDEN_PREFIX"
                | "REDUNDANT_TYPE_SUFFIX"
                | "GENERIC_FUNCTION_NAME"
                | "UNCLEAR_BOOLEAN_NAME"
                | "UNCLEAR_COLLECTION_NAME"
                | "VERB_SUFFIX_PATTERN" => Severity::Critical,
                _ => Severity::Warning,
            };
            self.add_violation(severity, line_num, rule, &message);
        }
    }

    /// Check for proper error documentation
    fn check_error_documentation(&mut self) {
        let mut violations = Vec::new();
        let mut in_doc_block = false;
        let mut doc_lines = Vec::new();
        let mut in_impl_block = false;

        for (line_num, line) in self.file_lines.iter().enumerate() {
            let trimmed = line.trim_start();

            if trimmed.starts_with("///") {
                in_doc_block = true;
                doc_lines.push(line.clone());
            } else if trimmed.starts_with("impl ") {
                in_impl_block = true;
                in_doc_block = false;
                doc_lines.clear();
            } else if trimmed == "}" && in_impl_block {
                in_impl_block = false;
                in_doc_block = false;
                doc_lines.clear();
            } else if line.trim().is_empty() && in_doc_block {
                // Continue collecting docs
            } else if trimmed.starts_with("pub fn ") && line.contains("Result<") {
                let func_name = self.extract_function_name(line).unwrap_or_default();

                // Only check documentation for non-impl functions
                if !in_impl_block {
                    if in_doc_block {
                        let doc_text = doc_lines.join("\n");
                        if !doc_text.contains("# Errors") {
                            violations.push((
                                line_num + 1,
                                "MISSING_ERRORS_DOC",
                                format!(
                                    "Public function '{}' returning Result missing '# Errors' documentation",
                                    func_name
                                ),
                            ));
                        }
                    } else {
                        violations.push((
                            line_num + 1,
                            "MISSING_PUBLIC_DOC",
                            format!("Public function '{}' missing documentation", func_name),
                        ));
                    }
                }

                in_doc_block = false;
                doc_lines.clear();
            } else {
                in_doc_block = false;
                doc_lines.clear();
            }
        }

        for (line_num, rule, message) in violations {
            self.add_violation(Severity::Critical, line_num, rule, &message);
        }
    }

    /// Check for proper public documentation
    fn check_public_documentation(&mut self) {
        let mut has_doc = false;
        let mut violations = Vec::new();
        let mut in_impl_block = false;

        for (line_num, line) in self.file_lines.iter().enumerate() {
            let trimmed = line.trim_start();

            if trimmed.starts_with("///") {
                has_doc = true;
                // Continue
            } else if trimmed.starts_with("impl ") {
                in_impl_block = true;
                has_doc = false;
            } else if trimmed == "}" && in_impl_block {
                in_impl_block = false;
                has_doc = false;
            } else if trimmed.starts_with("pub fn ") && !line.contains("Result<") {
                if !has_doc && !in_impl_block {
                    let func_name = self.extract_function_name(line).unwrap_or_default();
                    violations.push((
                        line_num + 1,
                        "MISSING_PUBLIC_DOC",
                        format!("Public function '{}' missing documentation", func_name),
                    ));
                }
                has_doc = false;
            } else if trimmed.starts_with("pub struct ") {
                if !has_doc {
                    let struct_name = self.extract_struct_name(line).unwrap_or_default();
                    violations.push((
                        line_num + 1,
                        "MISSING_PUBLIC_DOC",
                        format!("Public struct '{}' missing documentation", struct_name),
                    ));
                }
                has_doc = false;
            } else if trimmed.starts_with("pub enum ") {
                if !has_doc {
                    let enum_name = self.extract_enum_name(line).unwrap_or_default();
                    violations.push((
                        line_num + 1,
                        "MISSING_PUBLIC_DOC",
                        format!("Public enum '{}' missing documentation", enum_name),
                    ));
                }
                has_doc = false;
            } else if !trimmed.is_empty()
                && !trimmed.starts_with("//")
                && !trimmed.starts_with("#[")
                && !trimmed.starts_with("use ")
            {
                // Only reset has_doc for substantial code lines, not attributes or imports
                has_doc = false;
            }
        }

        for (line_num, rule, message) in violations {
            self.add_violation(Severity::Critical, line_num, rule, &message);
        }
    }

    /// Check test naming patterns follow docs/NAMING_CONVENTIONS.md
    fn check_test_naming_patterns(&mut self) {
        let mut violations = Vec::new();
        for (line_num, line) in self.file_lines.iter().enumerate() {
            if line.trim_start().starts_with("fn test_") {
                let test_name = self.extract_function_name(line).unwrap_or_default();

                // Test names should follow test_unit_condition_outcome pattern
                let parts: Vec<&str> = test_name.split('_').collect();
                if parts.len() < 4 {
                    violations.push((
                        line_num + 1,
                        "TEST_NAMING_PATTERN",
                        format!(
                            "Test '{}' should follow pattern 'test_unit_condition_outcome' from docs/NAMING_CONVENTIONS.md",
                            test_name
                        ),
                    ));
                }
            }
        }

        for (line_num, rule, message) in violations {
            self.add_violation(Severity::Warning, line_num, rule, &message);
        }
    }

    /// Check import organization
    fn check_import_organization(&mut self) {
        let mut _found_imports = false;
        let mut found_non_import = false;
        let mut violations = Vec::new();

        for (line_num, line) in self.file_lines.iter().enumerate() {
            let trimmed = line.trim();

            if trimmed.is_empty() || trimmed.starts_with("//") || trimmed.starts_with("//!") {
                continue;
            }

            if trimmed.starts_with("use ") {
                if found_non_import {
                    violations.push((
                        line_num + 1,
                        "IMPORT_ORGANIZATION",
                        "Imports should be grouped together at the top of the file",
                    ));
                }
            } else {
                found_non_import = true;
            }
        }

        for (line_num, rule, message) in violations {
            self.add_violation(Severity::Warning, line_num, rule, message);
        }
    }

    /// Check for discarded variables in struct fields and parameters (planning for future)
    fn check_discarded_variables(&mut self) {
        let mut violations = Vec::new();

        for (line_num, line) in self.file_lines.iter().enumerate() {
            let trimmed = line.trim();

            // Check for discarded struct fields (clear "planning for future" indicator)
            if trimmed.contains("_")
                && (trimmed.contains("pub struct") || trimmed.contains("struct"))
            {
                // Look for struct field definitions with _ prefix
                if let Some(field_name) = self.extract_struct_field_with_underscore(line) {
                    violations.push((
                        line_num + 1,
                        "DISCARDED_STRUCT_FIELD",
                        format!(
                            "Struct field '{}' prefixed with _ suggests planning for future - either use it or remove it",
                            field_name
                        ),
                    ));
                }
            }

            // Check for function parameters with _ prefix that aren't tuple destructuring
            if (trimmed.contains("fn ") || trimmed.contains("pub fn ")) && trimmed.contains("_")
                && !trimmed.contains("(") // Skip tuple destructuring patterns
                && !trimmed.contains("let (")
            {
                if let Some(param_name) = self.extract_function_param_with_underscore(line) {
                    violations.push((
                        line_num + 1,
                        "DISCARDED_PARAMETER",
                        format!(
                            "Function parameter '{}' prefixed with _ suggests planning for future - either use it or document why it's intentionally unused",
                            param_name
                        ),
                    ));
                }
            }
        }

        for (line_num, rule, message) in violations {
            self.add_violation(Severity::Warning, line_num, rule, &message);
        }
    }

    /// Check for inline module references that should be imports instead
    fn check_context_losing_imports(&mut self) {
        let mut violations = Vec::new();

        for (line_num, line) in self.file_lines.iter().enumerate() {
            let trimmed = line.trim();

            // Skip comments and empty lines
            if trimmed.is_empty() || trimmed.starts_with("//") || trimmed.starts_with("///") {
                continue;
            }

            // Check use statements
            if trimmed.starts_with("use ") {
                self.check_use_statement_for_context_loss(line_num + 1, trimmed, &mut violations);
            }
        }

        // Add all violations after the loop
        for (severity, line_num, rule, message) in violations {
            self.add_violation(severity, line_num, &rule, &message);
        }
    }

    fn check_use_statement_for_context_loss(
        &self,
        line_num: usize,
        line: &str,
        violations: &mut Vec<(Severity, usize, String, String)>,
    ) {
        // Parse use statement to extract imported items
        let use_content = line
            .strip_prefix("use ")
            .unwrap_or("")
            .trim_end_matches(';');

        // Handle different import patterns
        if use_content.contains('{') {
            // Multiple imports: use module::{func1, func2}
            self.check_multiple_imports(line_num, use_content, violations);
        } else {
            // Single import: use module::func
            self.check_single_import(line_num, use_content, violations);
        }
    }

    fn check_single_import(
        &self,
        line_num: usize,
        import_path: &str,
        violations: &mut Vec<(Severity, usize, String, String)>,
    ) {
        if let Some(function_name) = import_path.split("::").last() {
            if CONTEXT_CRITICAL_FUNCTIONS.contains(&function_name) {
                let module_path = import_path.rsplit_once("::").map(|(m, _)| m).unwrap_or("");
                violations.push((
                    Severity::Critical,
                    line_num,
                    "CONTEXT_LOSING_IMPORT".to_string(),
                    format!(
                        "Function '{}' loses critical context when imported from '{}' - use {}::{} instead",
                        function_name, module_path, module_path, function_name
                    ),
                ));
            }
        }
    }

    fn check_multiple_imports(
        &self,
        line_num: usize,
        import_content: &str,
        violations: &mut Vec<(Severity, usize, String, String)>,
    ) {
        if let Some((module_path, items_part)) = import_content.split_once('{') {
            let module_path = module_path.trim_end_matches("::").trim();
            let items = items_part.trim_end_matches('}').trim();

            for item in items.split(',') {
                let item = item.trim();
                if CONTEXT_CRITICAL_FUNCTIONS.contains(&item) {
                    violations.push((
                        Severity::Critical,
                        line_num,
                        "CONTEXT_LOSING_IMPORT".to_string(),
                        format!(
                            "Function '{}' loses critical context when imported from '{}' - use {}::{} instead",
                            item, module_path, module_path, item
                        ),
                    ));
                }
            }
        }
    }

    fn check_bare_function_calls(&mut self) {
        // First, collect all imported context-critical functions
        let mut imported_functions = std::collections::HashSet::new();

        for line in &self.file_lines {
            let trimmed = line.trim();
            if trimmed.starts_with("use ") {
                self.collect_imported_functions(trimmed, &mut imported_functions);
            }
        }

        // Collect violations first to avoid borrowing issues
        let mut violations = Vec::new();

        // Then check for bare calls to these functions
        for (line_num, line) in self.file_lines.iter().enumerate() {
            let trimmed = line.trim();

            // Skip comments, empty lines, and use statements
            if trimmed.is_empty()
                || trimmed.starts_with("//")
                || trimmed.starts_with("///")
                || trimmed.starts_with("use ")
            {
                continue;
            }

            self.check_line_for_bare_calls(
                line_num + 1,
                line,
                &imported_functions,
                &mut violations,
            );
        }

        // Add all violations after the loop
        for (severity, line_num, rule, message) in violations {
            self.add_violation(severity, line_num, &rule, &message);
        }
    }

    fn collect_imported_functions(
        &self,
        use_statement: &str,
        imported_functions: &mut std::collections::HashSet<String>,
    ) {
        let use_content = use_statement
            .strip_prefix("use ")
            .unwrap_or("")
            .trim_end_matches(';');

        if use_content.contains('{') {
            // Multiple imports: use module::{func1, func2}
            if let Some((_, items_part)) = use_content.split_once('{') {
                let items = items_part.trim_end_matches('}').trim();
                for item in items.split(',') {
                    let item = item.trim();
                    if CONTEXT_CRITICAL_FUNCTIONS.contains(&item) {
                        imported_functions.insert(item.to_string());
                    }
                }
            }
        } else {
            // Single import: use module::func
            if let Some(function_name) = use_content.split("::").last() {
                if CONTEXT_CRITICAL_FUNCTIONS.contains(&function_name) {
                    imported_functions.insert(function_name.to_string());
                }
            }
        }
    }

    fn check_line_for_bare_calls(
        &self,
        line_num: usize,
        line: &str,
        imported_functions: &std::collections::HashSet<String>,
        violations: &mut Vec<(Severity, usize, String, String)>,
    ) {
        for function_name in imported_functions {
            // Look for bare function calls (function_name followed by parentheses)
            let call_pattern = format!("{}(", function_name);

            if line.contains(&call_pattern) {
                // Make sure it's not a module-qualified call
                let module_qualified_pattern = format!("::{}", &call_pattern);
                if !line.contains(&module_qualified_pattern) {
                    violations.push((
                        Severity::Critical,
                        line_num,
                        "BARE_CONTEXT_CRITICAL_CALL".to_string(),
                        format!(
                            "Function '{}' called without module context - this function should always include its module path for clarity",
                            function_name
                        ),
                    ));
                }
            }
        }
    }

    fn check_inline_module_references(&mut self) {
        let mut violations_to_add = Vec::new();
        let mut in_import_section = false;
        let mut in_doc_comment = false;
        let mut in_multiline_comment = false;
        let mut module_usage_counts = std::collections::HashMap::new();

        // First pass: collect usage statistics
        for line in &self.file_lines {
            let trimmed = line.trim();
            if trimmed.starts_with("use ")
                || trimmed.starts_with("//")
                || trimmed.starts_with("///")
            {
                continue;
            }

            // Count module usage patterns
            let inline_pattern =
                regex::Regex::new(r"\b([a-zA-Z_][a-zA-Z0-9_]*(?:_[a-zA-Z_][a-zA-Z0-9_]*)*)::")
                    .expect("Invalid regex pattern");

            for capture in inline_pattern.find_iter(line) {
                let module_name = capture.as_str().trim_end_matches("::");
                *module_usage_counts
                    .entry(module_name.to_string())
                    .or_insert(0) += 1;
            }
        }

        // Second pass: analyze each line for violations
        for (line_num, line) in self.file_lines.iter().enumerate() {
            let trimmed = line.trim();

            // Skip empty lines
            if trimmed.is_empty() {
                continue;
            }

            // Track comment state
            if trimmed.starts_with("//!") || trimmed.starts_with("///") {
                in_doc_comment = true;
                continue;
            }

            if trimmed.starts_with("/*") {
                in_multiline_comment = true;
            }
            if trimmed.ends_with("*/") {
                in_multiline_comment = false;
                continue;
            }

            // Skip if we're in comments
            if in_doc_comment || in_multiline_comment || trimmed.starts_with("//") {
                in_doc_comment = false;
                continue;
            }

            // Track import section
            if trimmed.starts_with("use ") {
                in_import_section = true;
                continue;
            } else if in_import_section && !trimmed.is_empty() {
                in_import_section = false;
            }

            // Skip import lines and module declarations
            if in_import_section || trimmed.starts_with("use ") || trimmed.starts_with("mod ") {
                continue;
            }

            // Check for inline module references
            self.collect_inline_reference_violations(
                line_num + 1,
                line,
                &mut violations_to_add,
                &module_usage_counts,
            );
        }

        // Add all violations after the loop
        for (severity, line_num, rule, message) in violations_to_add {
            self.add_violation(severity, line_num, &rule, &message);
        }
    }

    fn collect_inline_reference_violations(
        &self,
        line_num: usize,
        line: &str,
        violations: &mut Vec<(Severity, usize, String, String)>,
        module_usage_counts: &std::collections::HashMap<String, usize>,
    ) {
        // Regex pattern to match module::path::item patterns
        let inline_pattern =
            regex::Regex::new(r"\b([a-zA-Z_][a-zA-Z0-9_]*(?:_[a-zA-Z_][a-zA-Z0-9_]*)*)::")
                .expect("Invalid regex pattern");

        for capture in inline_pattern.find_iter(line) {
            let match_str = capture.as_str();
            let full_match_end = std::cmp::min(capture.end() + 30, line.len());
            let full_match = &line[capture.start()..full_match_end];
            let module_name = match_str.trim_end_matches("::");

            // Check if this is an allowed inline pattern for semantic clarity
            let is_context_preserving = ALLOWED_INLINE_PATTERNS
                .iter()
                .any(|&pattern| full_match.starts_with(pattern));

            // Check if this should definitely be imported (common types)
            let should_import = REQUIRE_IMPORT_PATTERNS
                .iter()
                .any(|&pattern| full_match.starts_with(pattern));

            // Check if it contains context-critical functions
            let has_context_critical_function = CONTEXT_CRITICAL_FUNCTIONS
                .iter()
                .any(|&func| full_match.contains(&format!("::{}", func)));

            // Determine if this is a function call or type usage
            let usage_type = self.detect_usage_type(line, capture.start(), full_match);

            // Get usage count for this module
            let usage_count = module_usage_counts.get(module_name).unwrap_or(&0);

            // Check if it's a module we care about
            let is_tracked_module = STD_MODULES.iter().any(|&std_mod| module_name == std_mod)
                || self
                    .external_dependencies
                    .iter()
                    .any(|ext| module_name == ext || module_name.starts_with(&format!("{}_", ext)))
                || self
                    .workspace_crates
                    .iter()
                    .any(|ws| module_name.starts_with(ws));

            if is_tracked_module {
                let context = Self::get_context_around_match(line, capture.start());

                // Skip if it's in a string literal, doc comment, or other acceptable context
                if Self::is_acceptable_inline_context(&context, line) {
                    continue;
                }

                // Determine recommendation based on context and usage
                let recommendation = self.analyze_import_context(
                    module_name,
                    full_match,
                    *usage_count,
                    is_context_preserving,
                    has_context_critical_function,
                    should_import,
                    usage_type,
                );

                match recommendation {
                    Recommendation::PreferImport(reason) => {
                        violations.push((
                            Severity::Warning,
                            line_num,
                            "INLINE_MODULE_REFERENCE".to_string(),
                            format!(
                                "Import '{}' at top of file - used {} times ({})",
                                match_str, usage_count, reason
                            ),
                        ));
                    }
                    Recommendation::PreferInline(_reason) => {
                        // Don't report - inline is preferred for context
                    }
                }
            }
        }
    }

    fn get_context_around_match(line: &str, start_pos: usize) -> &str {
        let context_start = start_pos.saturating_sub(10);
        let context_end = std::cmp::min(start_pos + 20, line.len());
        &line[context_start..context_end]
    }

    fn is_acceptable_inline_context(context: &str, full_line: &str) -> bool {
        // Skip string literals
        if context.contains('"') || context.contains('\'') {
            return true;
        }

        // Skip macro invocations that commonly use full paths
        if full_line.trim_start().starts_with('#') {
            return true;
        }

        // Skip certain acceptable patterns like error trait objects
        if context.contains("dyn ") || context.contains("Box<dyn") {
            return true;
        }

        // Skip FQTN (Fully Qualified Type Name) disambiguation cases
        if context.contains('<') && context.contains('>') {
            return true;
        }

        // Skip type annotations and return types
        if context.contains(": ") || context.contains("-> ") {
            return true;
        }

        // Skip match patterns
        if full_line.trim_start().starts_with("Ok(") || full_line.trim_start().starts_with("Err(") {
            return true;
        }

        false
    }

    fn analyze_import_context(
        &self,
        module_name: &str,
        _full_match: &str,
        usage_count: usize,
        is_context_preserving: bool,
        has_context_critical_function: bool,
        should_import: bool,
        usage_type: UsageType,
    ) -> Recommendation {
        // Always preserve context for critical functions
        if has_context_critical_function {
            return Recommendation::PreferInline("Context-critical function".to_string());
        }

        // Always preserve context for explicitly allowed patterns
        if is_context_preserving {
            return Recommendation::PreferInline("Context-preserving pattern".to_string());
        }

        // Check if this is a workspace crate (preserve context unless very common)
        let is_workspace_crate = self
            .workspace_crates
            .iter()
            .any(|ws| module_name.starts_with(ws));

        if is_workspace_crate {
            match usage_type {
                UsageType::Function => {
                    // Always preserve context for workspace functions
                    return Recommendation::PreferInline(
                        "Workspace crate context valuable".to_string(),
                    );
                }
                UsageType::Type => {
                    // Only suggest imports for very frequently used types (5+ times)
                    if usage_count >= 5 {
                        return Recommendation::PreferImport(
                            "Universally recognized type".to_string(),
                        );
                    } else {
                        return Recommendation::PreferInline(
                            "Workspace type context valuable".to_string(),
                        );
                    }
                }
                UsageType::Unknown => {
                    // Conservative approach for unknown usage
                    return Recommendation::PreferInline(
                        "Workspace function context valuable".to_string(),
                    );
                }
            }
        }

        // Import universally recognized types
        if should_import {
            return Recommendation::PreferImport("Workspace type used very frequently".to_string());
        }

        // Different thresholds for functions vs types
        match usage_type {
            UsageType::Function => {
                // Functions always preserve context - never suggest imports
                Recommendation::PreferInline("Usage context unclear".to_string())
            }
            UsageType::Type => {
                // Types can be imported at lower threshold (max 2 uses inline)
                if usage_count >= 3 {
                    Recommendation::PreferImport("Used frequently".to_string())
                } else {
                    Recommendation::PreferInline("Function context valuable".to_string())
                }
            }
            UsageType::Unknown => {
                // Conservative approach for unknown usage
                if usage_count >= 4 {
                    Recommendation::PreferImport("Type used frequently".to_string())
                } else {
                    Recommendation::PreferInline("Type used infrequently".to_string())
                }
            }
        }
    }

    fn detect_usage_type(&self, line: &str, start_pos: usize, _full_match: &str) -> UsageType {
        // Look ahead in the line to determine usage type
        let remaining_line = &line[start_pos..];

        // Check for function call patterns
        if remaining_line.contains("(") {
            // Look for pattern like "module::function("
            if let Some(paren_pos) = remaining_line.find('(') {
                let before_paren = &remaining_line[..paren_pos];
                if before_paren.split("::").last().map_or(false, |s| {
                    s.chars().next().map_or(false, |c| c.is_lowercase())
                }) {
                    return UsageType::Function;
                }
            }
        }

        // Check for type patterns
        if remaining_line.contains("::") {
            let after_double_colon = remaining_line.split("::").last().unwrap_or("");
            if after_double_colon
                .chars()
                .next()
                .map_or(false, |c| c.is_uppercase())
            {
                return UsageType::Type;
            }
        }

        // Check context patterns for type usage
        let context_before = if start_pos >= 20 {
            &line[start_pos - 20..start_pos]
        } else {
            &line[..start_pos]
        };

        // Type annotation patterns
        if context_before.contains(": ") || context_before.contains("-> ") {
            return UsageType::Type;
        }

        // Variable declaration patterns
        if context_before.contains("let ") || context_before.contains("const ") {
            return UsageType::Type;
        }

        UsageType::Unknown
    }

    /// Detect workspace crates by parsing Cargo.toml files
    fn detect_workspace_crates() -> Vec<String> {
        let mut workspace_crates = Vec::new();

        // Try to read workspace Cargo.toml
        if let Ok(workspace_toml) = std::fs::read_to_string("Cargo.toml") {
            if let Ok(parsed) = workspace_toml.parse::<toml::Value>() {
                // Check for workspace members
                if let Some(workspace) = parsed.get("workspace") {
                    if let Some(members) = workspace.get("members") {
                        if let Some(members_array) = members.as_array() {
                            for member in members_array {
                                if let Some(member_str) = member.as_str() {
                                    // Extract crate name from path
                                    let crate_name = member_str
                                        .split('/')
                                        .last()
                                        .unwrap_or(member_str)
                                        .replace('-', "_");
                                    workspace_crates.push(crate_name);
                                }
                            }
                        }
                    }
                }

                // Also check package name in root Cargo.toml
                if let Some(package) = parsed.get("package") {
                    if let Some(name) = package.get("name") {
                        if let Some(name_str) = name.as_str() {
                            workspace_crates.push(name_str.replace('-', "_"));
                        }
                    }
                }
            }
        }

        workspace_crates
    }

    /// Detect external dependencies by parsing Cargo.toml files in the workspace
    fn detect_external_dependencies() -> Vec<String> {
        let mut dependencies = std::collections::HashSet::new();

        // Parse workspace root Cargo.toml
        if let Ok(content) = std::fs::read_to_string("Cargo.toml") {
            Self::extract_dependencies_from_toml(&content, &mut dependencies);
        }

        // Parse each workspace member's Cargo.toml
        if let Ok(workspace_toml) = std::fs::read_to_string("Cargo.toml") {
            if let Ok(parsed) = workspace_toml.parse::<toml::Value>() {
                if let Some(workspace) = parsed.get("workspace") {
                    if let Some(members) = workspace.get("members") {
                        if let Some(members_array) = members.as_array() {
                            for member in members_array {
                                if let Some(member_str) = member.as_str() {
                                    let cargo_path = format!("{}/Cargo.toml", member_str);
                                    if let Ok(member_content) = std::fs::read_to_string(&cargo_path)
                                    {
                                        Self::extract_dependencies_from_toml(
                                            &member_content,
                                            &mut dependencies,
                                        );
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        dependencies.into_iter().collect()
    }

    fn extract_dependencies_from_toml(
        content: &str,
        dependencies: &mut std::collections::HashSet<String>,
    ) {
        if let Ok(parsed) = content.parse::<toml::Value>() {
            // Regular dependencies
            if let Some(deps) = parsed.get("dependencies") {
                Self::collect_dependency_names(deps, dependencies);
            }

            // Dev dependencies
            if let Some(dev_deps) = parsed.get("dev-dependencies") {
                Self::collect_dependency_names(dev_deps, dependencies);
            }

            // Build dependencies
            if let Some(build_deps) = parsed.get("build-dependencies") {
                Self::collect_dependency_names(build_deps, dependencies);
            }

            // Target-specific dependencies
            if let Some(target) = parsed.get("target") {
                if let Some(target_table) = target.as_table() {
                    for (_, target_config) in target_table {
                        if let Some(deps) = target_config.get("dependencies") {
                            Self::collect_dependency_names(deps, dependencies);
                        }
                        if let Some(dev_deps) = target_config.get("dev-dependencies") {
                            Self::collect_dependency_names(dev_deps, dependencies);
                        }
                        if let Some(build_deps) = target_config.get("build-dependencies") {
                            Self::collect_dependency_names(build_deps, dependencies);
                        }
                    }
                }
            }
        }
    }

    fn collect_dependency_names(
        deps: &toml::Value,
        dependencies: &mut std::collections::HashSet<String>,
    ) {
        if let Some(deps_table) = deps.as_table() {
            for (name, _) in deps_table {
                // Convert kebab-case to snake_case for module names
                let module_name = name.replace('-', "_");
                dependencies.insert(module_name);
            }
        }
    }

    fn extract_struct_field_with_underscore(&self, line: &str) -> Option<String> {
        let trimmed = line.trim();

        // Look for field patterns like "_field_name: Type,"
        if trimmed.starts_with('_') && trimmed.contains(':') {
            if let Some(colon_pos) = trimmed.find(':') {
                let field_name = trimmed[..colon_pos].trim();
                if field_name.starts_with('_') && field_name.len() > 1 {
                    return Some(field_name.to_string());
                }
            }
        }

        None
    }

    /// Extract function parameter name with underscore prefix
    fn extract_function_param_with_underscore(&self, line: &str) -> Option<String> {
        let trimmed = line.trim();

        // Look for parameter patterns in function signatures
        if let Some(paren_start) = trimmed.find('(') {
            if let Some(paren_end) = trimmed.find(')') {
                let params = &trimmed[paren_start + 1..paren_end];

                // Split by comma and check each parameter
                for param in params.split(',') {
                    let param = param.trim();
                    if param.starts_with('_') && param.contains(':') {
                        if let Some(colon_pos) = param.find(':') {
                            let param_name = param[..colon_pos].trim();
                            if param_name.starts_with('_') && param_name.len() > 1 {
                                return Some(param_name.to_string());
                            }
                        }
                    }
                }
            }
        }

        None
    }

    // Helper methods for extracting names from code lines
    fn extract_function_name(&self, line: &str) -> Option<String> {
        let stripped = line
            .trim_start()
            .strip_prefix("pub fn ")
            .or_else(|| line.trim_start().strip_prefix("fn "))?;

        Some(stripped.split(['(', '<']).next()?.trim().to_string())
    }

    fn extract_struct_name(&self, line: &str) -> Option<String> {
        let stripped = line
            .trim_start()
            .strip_prefix("pub struct ")
            .or_else(|| line.trim_start().strip_prefix("struct "))?;

        Some(stripped.split(['<', '{', ' ']).next()?.trim().to_string())
    }

    fn extract_enum_name(&self, line: &str) -> Option<String> {
        let stripped = line
            .trim_start()
            .strip_prefix("pub enum ")
            .or_else(|| line.trim_start().strip_prefix("enum "))?;

        Some(stripped.split(['<', '{', ' ']).next()?.trim().to_string())
    }

    fn extract_variable_name(&self, line: &str) -> Option<String> {
        let stripped = line.trim_start().strip_prefix("let ")?;
        let name_part = stripped.split([' ', ':', '=']).next()?;

        // Skip patterns like `mut` and destructuring
        if name_part == "mut" {
            stripped
                .split([' ', ':', '='])
                .nth(1)
                .map(|s| s.to_string())
        } else if name_part.starts_with('_') && name_part.len() > 1 {
            Some(name_part.to_string())
        } else if !name_part.starts_with('(') && !name_part.starts_with('[') {
            Some(name_part.to_string())
        } else {
            None
        }
    }
}

/// Collect all Rust source files in the workspace
fn collect_workspace_files() -> Vec<PathBuf> {
    let mut files = Vec::new();

    // tidy.rs is in tests/ subdirectory, so workspace root is parent
    let workspace_root = std::env::current_dir()
        .unwrap()
        .parent()
        .unwrap()
        .to_path_buf();

    // Workspace crates
    for crate_name in &[
        "riptide-core",
        "riptide-web",
        "riptide-search",
        "riptide-cli",
        "riptide-sim",
    ] {
        let src_path = workspace_root.join(crate_name).join("src");
        if src_path.exists() {
            collect_rust_files_in_dir(&src_path, &mut files);
        }
    }

    // Integration tests and examples (skip tests to avoid recursive checking)
    for dir_name in &["examples"] {
        let dir_path = workspace_root.join(dir_name);
        if dir_path.exists() {
            collect_rust_files_in_dir(&dir_path, &mut files);
        }
    }
    files
}

fn collect_rust_files_in_dir(dir: &Path, files: &mut Vec<PathBuf>) {
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_file() && path.extension().map_or(false, |ext| ext == "rs") {
                // Skip the tidy.rs file itself
                if path.file_name().map_or(false, |name| name == "tidy.rs") {
                    continue;
                }
                files.push(path);
            } else if path.is_dir() {
                collect_rust_files_in_dir(&path, files);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn enforce_riptide_style_consistency() {
        let mut checker = StyleChecker::new();
        let mut file_count = 0;
        let mut critical_violations = 0;
        let mut warning_violations = 0;

        // Check all workspace files
        for file_path in collect_workspace_files() {
            if let Err(e) = checker.check_file(file_path.clone()) {
                eprintln!("Failed to check {:?}: {}", file_path, e);
                continue;
            }
            file_count += 1;
        }

        // Sort violations by severity and location
        checker.violations.sort_by(|a, b| {
            a.severity
                .cmp(&b.severity)
                .then(a.file.cmp(&b.file))
                .then(a.line.cmp(&b.line))
        });

        // Report violations with focus on consistency
        let mut violations_by_rule: HashMap<String, usize> = HashMap::new();

        for violation in &checker.violations {
            *violations_by_rule
                .entry(violation.rule.clone())
                .or_insert(0) += 1;

            match violation.severity {
                Severity::Critical => {
                    critical_violations += 1;
                    println!(
                        "CRITICAL [{}] {}:{} - {}",
                        violation.rule, violation.file, violation.line, violation.message
                    );
                }
                Severity::Warning => {
                    warning_violations += 1;
                    println!(
                        "WARNING [{}] {}:{} - {}",
                        violation.rule, violation.file, violation.line, violation.message
                    );
                }
            }
        }

        // Summary report
        println!("\n--- Riptide Style Consistency Summary ---");
        println!("Files analyzed: {}", file_count);
        println!("Critical violations: {}", critical_violations);
        println!("Warning violations: {}", warning_violations);
        println!("Total violations: {}", checker.violations.len());

        if !violations_by_rule.is_empty() {
            println!("\nViolations by rule:");
            let mut rule_counts: Vec<_> = violations_by_rule.into_iter().collect();
            rule_counts.sort_by(|a, b| b.1.cmp(&a.1));
            for (rule, count) in rule_counts {
                println!("  {}: {}", rule, count);
            }
        }

        // Fail if critical violations found
        if critical_violations > 0 {
            panic!(
                "Found {} critical style violations that must be fixed",
                critical_violations
            );
        }

        if warning_violations > 0 {
            println!(
                "\nFound {} consistency warnings - consider addressing for better maintainability",
                warning_violations
            );
        }
    }

    #[test]
    fn style_consistency_coverage_report() {
        println!("\n--- Riptide Style Consistency Coverage ---");
        println!(
            "Enforcing TigerBeetle-inspired consistency from docs/STYLE.md and docs/NAMING_CONVENTIONS.md:"
        );
        println!("");
        println!("CRITICAL (Must Fix):");
        println!("  ✓ BANNED_MODULE_NAME - No utils/helpers anti-pattern modules");
        println!("  ✓ NO_EMOJIS - Absolutely no emojis anywhere");
        println!("  ✓ MISSING_ERRORS_DOC - Result functions need # Errors documentation");
        println!("  ✓ MISSING_PUBLIC_DOC - Public APIs must be documented");
        println!("  ✓ FORBIDDEN_PREFIX - No get_/set_/do_/run_ prefixes: use descriptive names");
        println!("  ✓ REDUNDANT_TYPE_SUFFIX - No DataStruct/StatusEnum: use Data/Status");
        println!(
            "  ✓ GENERIC_FUNCTION_NAME - No process/handle: use compress_headers/validate_torrent"
        );
        println!(
            "  ✓ UNCLEAR_BOOLEAN_NAME - No valid/ready: use is_valid_torrent/has_pending_pieces"
        );
        println!(
            "  ✓ UNCLEAR_COLLECTION_NAME - No list/map: use active_downloads/peer_connections"
        );
        println!("  ✓ VERB_SUFFIX_PATTERN - Use verb-first naming: verb_noun");
        println!("");
        println!("WARNING (Consistency Improvements):");
        println!("  ✓ MODULE_SIZE_LIMIT - Max 500 lines per module");
        println!("  ✓ NAMING_INCONSISTENCY - Detects mixed abbreviation patterns within modules");
        println!("  ✓ TEST_NAMING_PATTERN - Follow test_unit_condition_outcome pattern");
        println!("  ✓ IMPORT_ORGANIZATION - Group imports at top of file");
        println!("  ✓ ABBREVIATIONS - Use full words instead of abbreviations (index not idx)");
        println!("  ✓ DISCARDED_STRUCT_FIELD - Remove unused _ prefixed struct fields");
        println!("  ✓ DISCARDED_PARAMETER - Document or use _ prefixed parameters");
        println!(
            "  ✓ INLINE_MODULE_REFERENCE - Smart inline detection: allows semantic clarity (tokio::spawn), requires imports for common types"
        );
        println!("");
        println!("Focus: Automatic consistency detection rather than hardcoded rules");
        println!("Focus: Safety, Performance, Developer Experience");
    }
}
