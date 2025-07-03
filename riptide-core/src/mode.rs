//! Runtime mode configuration for Riptide.

use serde::{Deserialize, Serialize};

/// Runtime mode for Riptide services.
///
/// Controls whether to use real external services or simulated development data.
/// This allows offline development while maintaining the same interfaces.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RuntimeMode {
    /// Production mode - uses real external APIs and services
    Production,
    /// Development mode - uses simulated components for offline development
    Development,
}

impl RuntimeMode {
    /// Check if running in development mode.
    pub fn is_development(self) -> bool {
        matches!(self, Self::Development)
    }

    /// Check if running in production mode.
    pub fn is_production(self) -> bool {
        matches!(self, Self::Production)
    }
}

impl Default for RuntimeMode {
    fn default() -> Self {
        // Default to development mode for development convenience
        Self::Development
    }
}

impl std::fmt::Display for RuntimeMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Production => write!(f, "PRODUCTION"),
            Self::Development => write!(f, "DEVELOPMENT"),
        }
    }
}

impl std::str::FromStr for RuntimeMode {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "production" | "prod" => Ok(Self::Production),
            "development" | "dev" => Ok(Self::Development),
            _ => Err(format!(
                "Invalid runtime mode: '{s}'. Valid options are: production, development"
            )),
        }
    }
}
