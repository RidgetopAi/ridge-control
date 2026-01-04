//! Mandrel configuration types
//!
//! These types are used by both the config module and the agent/mandrel client.
//! Located here to avoid circular dependencies between config and agent modules.

#![allow(dead_code)]

use serde::{Deserialize, Serialize};

/// Error types for Mandrel operations
#[derive(Debug, Clone, thiserror::Error)]
pub enum MandrelError {
    #[error("HTTP request failed: {message}")]
    HttpError { message: String },

    #[error("Failed to parse response: {message}")]
    ParseError { message: String },

    #[error("Mandrel server error: {message}")]
    ServerError { message: String },

    #[error("Not connected to Mandrel")]
    NotConnected,

    #[error("Invalid project: {project}")]
    InvalidProject { project: String },
}

impl From<reqwest::Error> for MandrelError {
    fn from(e: reqwest::Error) -> Self {
        MandrelError::HttpError {
            message: e.to_string(),
        }
    }
}

/// Configuration for Mandrel connection
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MandrelConfig {
    /// Base URL of the Mandrel server (e.g., "https://mandrel.ridgetopai.net")
    pub base_url: String,
    /// Current project name
    pub project: String,
    /// Whether Mandrel integration is enabled
    pub enabled: bool,
    /// Request timeout in seconds
    pub timeout_secs: u64,
}

impl Default for MandrelConfig {
    fn default() -> Self {
        Self {
            base_url: "https://mandrel.ridgetopai.net".to_string(),
            project: "ridge-control".to_string(),
            enabled: true,
            timeout_secs: 30,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mandrel_config_default() {
        let config = MandrelConfig::default();
        assert_eq!(config.base_url, "https://mandrel.ridgetopai.net");
        assert_eq!(config.project, "ridge-control");
        assert!(config.enabled);
        assert_eq!(config.timeout_secs, 30);
    }

    #[test]
    fn test_mandrel_config_serialization() {
        let config = MandrelConfig {
            base_url: "http://localhost:8080".to_string(),
            project: "test-project".to_string(),
            enabled: false,
            timeout_secs: 60,
        };

        let toml = toml::to_string(&config).unwrap();
        assert!(toml.contains("http://localhost:8080"));
        assert!(toml.contains("test-project"));

        let parsed: MandrelConfig = toml::from_str(&toml).unwrap();
        assert_eq!(parsed.base_url, config.base_url);
        assert_eq!(parsed.project, config.project);
    }

    #[test]
    fn test_mandrel_error_display() {
        let err = MandrelError::HttpError {
            message: "Connection refused".to_string(),
        };
        assert!(err.to_string().contains("Connection refused"));

        let err = MandrelError::NotConnected;
        assert!(err.to_string().contains("Not connected"));

        let err = MandrelError::InvalidProject {
            project: "bad-project".to_string(),
        };
        assert!(err.to_string().contains("bad-project"));
    }
}
