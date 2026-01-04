//! LSP server configuration
//!
//! Configuration for language servers loaded from ~/.config/ridge-control/lsp.toml

#![allow(dead_code)]

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;

/// Configuration for a single LSP server
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct LspServerConfig {
    /// Path to the server binary (can use $PATH)
    pub command: String,

    /// Arguments to pass to the server
    pub args: Vec<String>,

    /// File extensions this server handles
    pub extensions: Vec<String>,

    /// Root marker files for workspace detection (e.g., "Cargo.toml", "package.json")
    pub root_patterns: Vec<String>,

    /// Initialization options passed to server
    pub init_options: serde_json::Value,

    /// Whether to auto-start on matching file access
    pub auto_start: bool,

    /// Request timeout in seconds
    pub timeout_secs: u64,

    /// Environment variables to set
    pub env: HashMap<String, String>,
}

impl Default for LspServerConfig {
    fn default() -> Self {
        Self {
            command: String::new(),
            args: Vec::new(),
            extensions: Vec::new(),
            root_patterns: Vec::new(),
            init_options: serde_json::Value::Null,
            auto_start: true,
            timeout_secs: 30,
            env: HashMap::new(),
        }
    }
}

/// Main LSP configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct LspConfig {
    /// Whether LSP integration is enabled globally
    pub enabled: bool,

    /// Default timeout for requests (seconds)
    pub default_timeout_secs: u64,

    /// Per-language server configurations
    pub servers: HashMap<String, LspServerConfig>,
}

impl Default for LspConfig {
    fn default() -> Self {
        let mut servers = HashMap::new();

        // TypeScript/JavaScript
        servers.insert(
            "typescript".to_string(),
            LspServerConfig {
                command: "typescript-language-server".to_string(),
                args: vec!["--stdio".to_string()],
                extensions: vec![
                    "ts".to_string(),
                    "tsx".to_string(),
                    "js".to_string(),
                    "jsx".to_string(),
                    "mjs".to_string(),
                    "cjs".to_string(),
                ],
                root_patterns: vec![
                    "package.json".to_string(),
                    "tsconfig.json".to_string(),
                    "jsconfig.json".to_string(),
                ],
                init_options: serde_json::json!({}),
                auto_start: true,
                timeout_secs: 30,
                env: HashMap::new(),
            },
        );

        // Rust
        servers.insert(
            "rust".to_string(),
            LspServerConfig {
                command: "rust-analyzer".to_string(),
                args: vec![],
                extensions: vec!["rs".to_string()],
                root_patterns: vec!["Cargo.toml".to_string()],
                init_options: serde_json::json!({}),
                auto_start: true,
                timeout_secs: 120, // rust-analyzer needs time for initial indexing
                env: HashMap::new(),
            },
        );

        // Python
        servers.insert(
            "python".to_string(),
            LspServerConfig {
                command: "pyright-langserver".to_string(),
                args: vec!["--stdio".to_string()],
                extensions: vec!["py".to_string(), "pyi".to_string()],
                root_patterns: vec![
                    "pyproject.toml".to_string(),
                    "setup.py".to_string(),
                    "requirements.txt".to_string(),
                    "pyrightconfig.json".to_string(),
                ],
                init_options: serde_json::json!({}),
                auto_start: true,
                timeout_secs: 30,
                env: HashMap::new(),
            },
        );

        Self {
            enabled: true,
            default_timeout_secs: 30,
            servers,
        }
    }
}

impl LspConfig {
    /// Load configuration from a TOML file
    pub fn load(path: &Path) -> Result<Self, LspConfigError> {
        let content = std::fs::read_to_string(path).map_err(LspConfigError::Io)?;
        toml::from_str(&content).map_err(LspConfigError::Parse)
    }

    /// Save configuration to a TOML file
    pub fn save(&self, path: &Path) -> Result<(), LspConfigError> {
        let content = toml::to_string_pretty(self).map_err(LspConfigError::Serialize)?;
        std::fs::write(path, content).map_err(LspConfigError::Io)
    }

    /// Get server config for a file extension
    pub fn server_for_extension(&self, ext: &str) -> Option<(&str, &LspServerConfig)> {
        let ext_lower = ext.to_lowercase();
        self.servers
            .iter()
            .find(|(_, cfg)| {
                cfg.extensions
                    .iter()
                    .any(|e| e.to_lowercase() == ext_lower)
            })
            .map(|(name, cfg)| (name.as_str(), cfg))
    }

    /// Get server config by name
    pub fn server_by_name(&self, name: &str) -> Option<&LspServerConfig> {
        self.servers.get(name)
    }

    /// Check if any server handles a file path
    pub fn has_server_for_file(&self, file_path: &str) -> bool {
        let ext = Path::new(file_path)
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("");
        self.server_for_extension(ext).is_some()
    }

    /// Get all configured server names
    pub fn server_names(&self) -> Vec<&str> {
        self.servers.keys().map(|s| s.as_str()).collect()
    }
}

/// LSP configuration errors
#[derive(Debug)]
pub enum LspConfigError {
    Io(std::io::Error),
    Parse(toml::de::Error),
    Serialize(toml::ser::Error),
}

impl std::fmt::Display for LspConfigError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LspConfigError::Io(e) => write!(f, "IO error: {}", e),
            LspConfigError::Parse(e) => write!(f, "Parse error: {}", e),
            LspConfigError::Serialize(e) => write!(f, "Serialize error: {}", e),
        }
    }
}

impl std::error::Error for LspConfigError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = LspConfig::default();
        assert!(config.enabled);
        assert!(config.servers.contains_key("typescript"));
        assert!(config.servers.contains_key("rust"));
        assert!(config.servers.contains_key("python"));
    }

    #[test]
    fn test_server_for_extension() {
        let config = LspConfig::default();

        let (name, _) = config.server_for_extension("rs").unwrap();
        assert_eq!(name, "rust");

        let (name, _) = config.server_for_extension("ts").unwrap();
        assert_eq!(name, "typescript");

        let (name, _) = config.server_for_extension("py").unwrap();
        assert_eq!(name, "python");

        assert!(config.server_for_extension("unknown").is_none());
    }

    #[test]
    fn test_has_server_for_file() {
        let config = LspConfig::default();
        assert!(config.has_server_for_file("/path/to/file.rs"));
        assert!(config.has_server_for_file("/path/to/file.ts"));
        assert!(!config.has_server_for_file("/path/to/file.xyz"));
    }

    #[test]
    fn test_serialization_roundtrip() {
        let config = LspConfig::default();
        let toml_str = toml::to_string_pretty(&config).unwrap();
        let parsed: LspConfig = toml::from_str(&toml_str).unwrap();

        assert_eq!(config.enabled, parsed.enabled);
        assert_eq!(config.servers.len(), parsed.servers.len());
    }

    #[test]
    fn test_parse_custom_config() {
        let toml_content = r#"
enabled = true
default_timeout_secs = 45

[servers.custom]
command = "my-language-server"
args = ["--stdio", "--debug"]
extensions = ["custom", "cst"]
root_patterns = ["custom.config"]
auto_start = false
timeout_secs = 120
"#;

        let config: LspConfig = toml::from_str(toml_content).unwrap();
        assert!(config.enabled);
        assert_eq!(config.default_timeout_secs, 45);

        let custom = config.servers.get("custom").unwrap();
        assert_eq!(custom.command, "my-language-server");
        assert_eq!(custom.args, vec!["--stdio", "--debug"]);
        assert!(!custom.auto_start);
        assert_eq!(custom.timeout_secs, 120);
    }
}
