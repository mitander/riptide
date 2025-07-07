//! Runtime mode configuration for Riptide.

use serde::{Deserialize, Serialize};

/// Runtime mode for Riptide services.
///
/// Controls the implementation strategy while keeping the same engine architecture.
/// The torrent engine itself is identical across all modes - only the trait implementations differ.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RuntimeMode {
    /// Production mode - real BitTorrent networking for deployment
    Production,
    /// Simulation mode - deterministic behavior for testing and bug reproduction
    Simulation,
    /// Development mode - fast simulation optimized for feature development (>50,000 Mbps)
    Development,
}

impl RuntimeMode {
    /// Check if running in any simulation mode (Development or Simulation).
    pub fn is_any_simulation(self) -> bool {
        matches!(self, Self::Simulation | Self::Development)
    }

    /// Check if running in production mode.
    pub fn is_production(self) -> bool {
        matches!(self, Self::Production)
    }

    /// Check if running in development mode (maximum performance simulation).
    pub fn is_development(self) -> bool {
        matches!(self, Self::Development)
    }

    /// Check if running in simulation mode (deterministic testing).
    pub fn is_simulation(self) -> bool {
        matches!(self, Self::Simulation)
    }
}

impl Default for RuntimeMode {
    fn default() -> Self {
        // Default to development mode for maximum performance during development
        Self::Development
    }
}

impl std::fmt::Display for RuntimeMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Production => write!(f, "production"),
            Self::Simulation => write!(f, "simulation"),
            Self::Development => write!(f, "development"),
        }
    }
}

impl std::str::FromStr for RuntimeMode {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "production" | "prod" => Ok(Self::Production),
            "simulation" | "sim" | "deterministic" => Ok(Self::Simulation),
            "development" | "dev" | "fast" => Ok(Self::Development),
            _ => Err(format!(
                "Invalid runtime mode: '{s}'. Valid options are: production, simulation, development"
            )),
        }
    }
}
