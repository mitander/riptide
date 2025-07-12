//! Tracing setup for Riptide
//!
//! Provides dual output: console logs (user-controlled level) and full debug logs to disk.
//! This ensures LLMs and developers have access to complete debugging information while
//! keeping the CLI experience clean for end users.

use std::fs::{File, create_dir_all};
use std::path::Path;

use tracing::Level;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::{EnvFilter, Layer, fmt};

/// Initialize tracing with dual output: console (user level) + file (full debug)
///
/// # Arguments
/// * `console_level` - Log level for console output (what user sees)
/// * `logs_dir` - Directory to write debug logs (defaults to "./logs")
///
/// # File Output
/// Writes complete debug logs to `logs/riptide-last-run.log`, overwriting previous run.
/// This ensures LLMs and developers always have access to full debugging information.
/// # Errors
///
/// - `Box<dyn std::error::Error>` - If logs directory cannot be created or log file cannot be opened for writing
pub fn init_tracing(
    console_level: Level,
    logs_dir: Option<&Path>,
) -> Result<(), Box<dyn std::error::Error>> {
    let logs_path = logs_dir.unwrap_or_else(|| Path::new("logs"));

    // Ensure logs directory exists
    create_dir_all(logs_path)?;

    // Create file for this run's debug logs
    let log_file_path = logs_path.join("riptide-last-run.log");
    let log_file = File::create(&log_file_path)?;

    // Console layer - respects user's chosen log level
    let console_filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new(console_level.to_string()));

    let console_layer = fmt::layer()
        .with_target(true)
        .with_thread_ids(false)
        .with_thread_names(false)
        .with_file(false)
        .with_line_number(false)
        .with_filter(console_filter);

    // File layer - always captures everything at TRACE level for debugging
    let file_filter = EnvFilter::new("trace");

    let file_layer = fmt::layer()
        .with_target(true)
        .with_thread_ids(true)
        .with_thread_names(true)
        .with_file(true)
        .with_line_number(true)
        .with_ansi(false) // No color codes in files
        .with_writer(log_file)
        .with_filter(file_filter);

    // Initialize with both layers
    tracing_subscriber::registry()
        .with(console_layer)
        .with(file_layer)
        .init();

    tracing::info!(
        "Tracing initialized: console={}, debug_file={}",
        console_level,
        log_file_path.display()
    );

    Ok(())
}

/// CLI log levels for user control
#[derive(Debug, Clone, Copy, clap::ValueEnum)]
pub enum CliLogLevel {
    /// Only error messages
    Error,
    /// Warning and error messages
    Warn,
    /// Informational, warning, and error messages
    Info,
    /// Debug, informational, warning, and error messages
    Debug,
    /// All messages including detailed tracing
    Trace,
}

impl CliLogLevel {
    /// Converts CLI log level to tracing Level enum.
    ///
    /// Maps user-friendly CLI log level names to the corresponding tracing::Level
    /// values for configuring log output verbosity.
    ///
    /// # Examples
    /// ```
    /// use riptide_core::tracing_setup::CliLogLevel;
    ///
    /// let level = CliLogLevel::Info.as_tracing_level();
    /// assert_eq!(level, tracing::Level::INFO);
    /// ```
    pub fn as_tracing_level(self) -> Level {
        match self {
            CliLogLevel::Error => Level::ERROR,
            CliLogLevel::Warn => Level::WARN,
            CliLogLevel::Info => Level::INFO,
            CliLogLevel::Debug => Level::DEBUG,
            CliLogLevel::Trace => Level::TRACE,
        }
    }
}

impl std::str::FromStr for CliLogLevel {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "error" => Ok(CliLogLevel::Error),
            "warn" => Ok(CliLogLevel::Warn),
            "info" => Ok(CliLogLevel::Info),
            "debug" => Ok(CliLogLevel::Debug),
            "trace" => Ok(CliLogLevel::Trace),
            _ => Err(format!("Invalid log level: {s}")),
        }
    }
}

impl std::fmt::Display for CliLogLevel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CliLogLevel::Error => write!(f, "error"),
            CliLogLevel::Warn => write!(f, "warn"),
            CliLogLevel::Info => write!(f, "info"),
            CliLogLevel::Debug => write!(f, "debug"),
            CliLogLevel::Trace => write!(f, "trace"),
        }
    }
}
