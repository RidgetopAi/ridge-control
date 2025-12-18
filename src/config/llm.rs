//! LLM configuration for provider/model defaults and parameters
//!
//! Configuration is stored in `~/.config/ridge-control/llm.toml`

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;

use crate::error::{RidgeError, Result};

/// Main LLM configuration struct
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct LLMConfig {
    /// Default provider and model
    pub defaults: LLMDefaults,
    /// LLM inference parameters
    pub parameters: LLMParameters,
    /// Per-provider configuration (default models, etc.)
    #[serde(flatten)]
    pub providers: HashMap<String, ProviderConfig>,
}

impl Default for LLMConfig {
    fn default() -> Self {
        let mut providers = HashMap::new();
        providers.insert(
            "anthropic".to_string(),
            ProviderConfig {
                default_model: "claude-sonnet-4-20250514".to_string(),
            },
        );
        providers.insert(
            "openai".to_string(),
            ProviderConfig {
                default_model: "gpt-4o".to_string(),
            },
        );
        providers.insert(
            "gemini".to_string(),
            ProviderConfig {
                default_model: "gemini-2.0-flash".to_string(),
            },
        );
        providers.insert(
            "grok".to_string(),
            ProviderConfig {
                default_model: "grok-3".to_string(),
            },
        );
        providers.insert(
            "groq".to_string(),
            ProviderConfig {
                default_model: "llama-3.3-70b-versatile".to_string(),
            },
        );

        Self {
            defaults: LLMDefaults::default(),
            parameters: LLMParameters::default(),
            providers,
        }
    }
}

/// Default provider and model selection
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct LLMDefaults {
    /// The default LLM provider (anthropic, openai, gemini, grok, groq)
    pub provider: String,
    /// The default model to use
    pub model: String,
}

impl Default for LLMDefaults {
    fn default() -> Self {
        Self {
            provider: "anthropic".to_string(),
            model: "claude-sonnet-4-20250514".to_string(),
        }
    }
}

/// LLM inference parameters
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct LLMParameters {
    /// Temperature for response randomness (0.0 - 2.0)
    pub temperature: f32,
    /// Maximum tokens in response
    pub max_tokens: u32,
}

impl Default for LLMParameters {
    fn default() -> Self {
        Self {
            temperature: 0.7,
            max_tokens: 8192,
        }
    }
}

/// Per-provider configuration
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct ProviderConfig {
    /// Default model for this provider
    pub default_model: String,
}

impl LLMConfig {
    /// Load LLM config from a file path
    pub fn load(path: &Path) -> Result<Self> {
        if !path.exists() {
            return Ok(Self::default());
        }

        let content = std::fs::read_to_string(path)
            .map_err(|e| RidgeError::Config(format!("Failed to read llm.toml: {}", e)))?;

        toml::from_str(&content)
            .map_err(|e| RidgeError::Config(format!("Failed to parse llm.toml: {}", e)))
    }

    /// Save LLM config to a file path
    pub fn save(&self, path: &Path) -> Result<()> {
        // Ensure parent directory exists
        if let Some(parent) = path.parent() {
            if !parent.exists() {
                std::fs::create_dir_all(parent)
                    .map_err(|e| RidgeError::Config(format!("Failed to create config dir: {}", e)))?;
            }
        }

        let content = toml::to_string_pretty(self)
            .map_err(|e| RidgeError::Config(format!("Failed to serialize llm config: {}", e)))?;

        std::fs::write(path, content)
            .map_err(|e| RidgeError::Config(format!("Failed to write llm.toml: {}", e)))?;

        Ok(())
    }

    /// Get the default model for a specific provider
    pub fn default_model_for_provider(&self, provider: &str) -> Option<&str> {
        self.providers
            .get(provider)
            .map(|p| p.default_model.as_str())
    }

    /// Set the default model for a specific provider
    pub fn set_default_model_for_provider(&mut self, provider: &str, model: &str) {
        self.providers
            .entry(provider.to_string())
            .or_default()
            .default_model = model.to_string();
    }

    /// Get list of configured provider names
    pub fn configured_providers(&self) -> Vec<&str> {
        self.providers.keys().map(|s| s.as_str()).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_default_llm_config() {
        let config = LLMConfig::default();
        assert_eq!(config.defaults.provider, "anthropic");
        assert_eq!(config.defaults.model, "claude-sonnet-4-20250514");
        assert_eq!(config.parameters.temperature, 0.7);
        assert_eq!(config.parameters.max_tokens, 8192);
    }

    #[test]
    fn test_default_providers() {
        let config = LLMConfig::default();
        assert_eq!(
            config.default_model_for_provider("anthropic"),
            Some("claude-sonnet-4-20250514")
        );
        assert_eq!(
            config.default_model_for_provider("openai"),
            Some("gpt-4o")
        );
        assert_eq!(
            config.default_model_for_provider("gemini"),
            Some("gemini-2.0-flash")
        );
        assert_eq!(
            config.default_model_for_provider("grok"),
            Some("grok-3")
        );
        assert_eq!(
            config.default_model_for_provider("groq"),
            Some("llama-3.3-70b-versatile")
        );
    }

    #[test]
    fn test_llm_config_serialization() {
        let config = LLMConfig::default();
        let toml_str = toml::to_string_pretty(&config).unwrap();
        let parsed: LLMConfig = toml::from_str(&toml_str).unwrap();
        assert_eq!(parsed.defaults.provider, config.defaults.provider);
        assert_eq!(parsed.defaults.model, config.defaults.model);
        assert_eq!(parsed.parameters.temperature, config.parameters.temperature);
        assert_eq!(parsed.parameters.max_tokens, config.parameters.max_tokens);
    }

    #[test]
    fn test_load_nonexistent_file_returns_default() {
        let config = LLMConfig::load(Path::new("/nonexistent/path/llm.toml")).unwrap();
        assert_eq!(config.defaults.provider, "anthropic");
    }

    #[test]
    fn test_save_and_load() {
        let temp_dir = TempDir::new().unwrap();
        let path = temp_dir.path().join("llm.toml");

        let mut config = LLMConfig::default();
        config.defaults.provider = "openai".to_string();
        config.defaults.model = "gpt-4o".to_string();
        config.parameters.temperature = 0.5;
        config.parameters.max_tokens = 4096;

        config.save(&path).unwrap();

        let loaded = LLMConfig::load(&path).unwrap();
        assert_eq!(loaded.defaults.provider, "openai");
        assert_eq!(loaded.defaults.model, "gpt-4o");
        assert_eq!(loaded.parameters.temperature, 0.5);
        assert_eq!(loaded.parameters.max_tokens, 4096);
    }

    #[test]
    fn test_set_default_model_for_provider() {
        let mut config = LLMConfig::default();
        config.set_default_model_for_provider("anthropic", "claude-3-opus-20240229");
        assert_eq!(
            config.default_model_for_provider("anthropic"),
            Some("claude-3-opus-20240229")
        );

        // Test setting for new provider
        config.set_default_model_for_provider("custom", "custom-model");
        assert_eq!(
            config.default_model_for_provider("custom"),
            Some("custom-model")
        );
    }

    #[test]
    fn test_configured_providers() {
        let config = LLMConfig::default();
        let providers = config.configured_providers();
        assert!(providers.contains(&"anthropic"));
        assert!(providers.contains(&"openai"));
        assert!(providers.contains(&"gemini"));
        assert!(providers.contains(&"grok"));
        assert!(providers.contains(&"groq"));
    }

    #[test]
    fn test_parse_custom_toml() {
        let toml_content = r#"
[defaults]
provider = "gemini"
model = "gemini-2.0-flash"

[parameters]
temperature = 0.9
max_tokens = 16384

[anthropic]
default_model = "claude-3-opus-20240229"

[openai]
default_model = "gpt-4-turbo"
"#;

        let config: LLMConfig = toml::from_str(toml_content).unwrap();
        assert_eq!(config.defaults.provider, "gemini");
        assert_eq!(config.defaults.model, "gemini-2.0-flash");
        assert_eq!(config.parameters.temperature, 0.9);
        assert_eq!(config.parameters.max_tokens, 16384);
        assert_eq!(
            config.default_model_for_provider("anthropic"),
            Some("claude-3-opus-20240229")
        );
        assert_eq!(
            config.default_model_for_provider("openai"),
            Some("gpt-4-turbo")
        );
    }
}
