//! Subagent configuration for multi-model delegation
//!
//! Allows configuring different models/providers for explore, plan, and review subagents.
//! Configuration is stored in `~/.config/ridge-control/llm.toml` under `[subagents]`.

#![allow(dead_code)]

use serde::{Deserialize, Serialize};

/// Configuration for a single subagent type
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct SubagentConfig {
    /// Model to use for this agent type
    pub model: String,
    /// Provider (anthropic, openai, gemini, grok, groq)
    pub provider: String,
    /// Maximum tokens for sub-agent context (None = provider default)
    pub max_context_tokens: Option<u32>,
    /// Tools available to this sub-agent (empty = all read-only tools)
    #[serde(default)]
    pub allowed_tools: Vec<String>,
}

impl Default for SubagentConfig {
    fn default() -> Self {
        Self {
            model: "claude-haiku-4-5-20251001".to_string(),
            provider: "anthropic".to_string(),
            max_context_tokens: Some(50_000),
            allowed_tools: vec![],
        }
    }
}

impl SubagentConfig {
    /// Create a new SubagentConfig with specified model and provider
    pub fn new(provider: &str, model: &str) -> Self {
        Self {
            provider: provider.to_string(),
            model: model.to_string(),
            max_context_tokens: None,
            allowed_tools: vec![],
        }
    }

    /// Builder method to set max context tokens
    pub fn with_max_tokens(mut self, tokens: u32) -> Self {
        self.max_context_tokens = Some(tokens);
        self
    }

    /// Builder method to set allowed tools
    pub fn with_tools(mut self, tools: Vec<String>) -> Self {
        self.allowed_tools = tools;
        self
    }
}

/// Configuration for all subagent types
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct SubagentsConfig {
    /// Agent for exploration/research tasks (codebase search, understanding)
    pub explore: SubagentConfig,
    /// Agent for planning/architecture decisions
    pub plan: SubagentConfig,
    /// Agent for code review
    pub review: SubagentConfig,
    /// Default configuration for unspecified agent types
    pub default: SubagentConfig,
}

impl Default for SubagentsConfig {
    fn default() -> Self {
        Self {
            explore: SubagentConfig {
                model: "claude-haiku-4-5-20251001".to_string(),
                provider: "anthropic".to_string(),
                max_context_tokens: Some(50_000),
                allowed_tools: vec![
                    "file_read".to_string(),
                    "grep".to_string(),
                    "glob".to_string(),
                    "tree".to_string(),
                    "find_symbol".to_string(),
                    "ast_search".to_string(),
                ],
            },
            plan: SubagentConfig {
                model: "claude-sonnet-4-20250514".to_string(),
                provider: "anthropic".to_string(),
                max_context_tokens: Some(100_000),
                allowed_tools: vec![], // All tools
            },
            review: SubagentConfig {
                model: "gpt-4o-mini".to_string(),
                provider: "openai".to_string(),
                max_context_tokens: Some(30_000),
                allowed_tools: vec![
                    "file_read".to_string(),
                    "grep".to_string(),
                    "ast_search".to_string(),
                ],
            },
            default: SubagentConfig {
                model: "claude-haiku-4-5-20251001".to_string(),
                provider: "anthropic".to_string(),
                max_context_tokens: Some(50_000),
                allowed_tools: vec![],
            },
        }
    }
}

impl SubagentsConfig {
    /// Get configuration for a specific agent type
    pub fn get(&self, agent_type: &str) -> &SubagentConfig {
        match agent_type {
            "explore" => &self.explore,
            "plan" => &self.plan,
            "review" => &self.review,
            _ => &self.default,
        }
    }

    /// Get mutable configuration for a specific agent type
    pub fn get_mut(&mut self, agent_type: &str) -> &mut SubagentConfig {
        match agent_type {
            "explore" => &mut self.explore,
            "plan" => &mut self.plan,
            "review" => &mut self.review,
            _ => &mut self.default,
        }
    }

    /// Get list of all agent types
    pub fn agent_types() -> &'static [&'static str] {
        &["explore", "plan", "review"]
    }

    /// Iterate over all configured subagent types and their configs
    pub fn iter(&self) -> impl Iterator<Item = (&'static str, &SubagentConfig)> {
        [
            ("explore", &self.explore),
            ("plan", &self.plan),
            ("review", &self.review),
        ]
        .into_iter()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_subagent_config() {
        let config = SubagentConfig::default();
        assert_eq!(config.model, "claude-haiku-4-5-20251001");
        assert_eq!(config.provider, "anthropic");
        assert_eq!(config.max_context_tokens, Some(50_000));
        assert!(config.allowed_tools.is_empty());
    }

    #[test]
    fn test_subagent_config_builder() {
        let config = SubagentConfig::new("openai", "gpt-4o")
            .with_max_tokens(100_000)
            .with_tools(vec!["file_read".to_string(), "grep".to_string()]);

        assert_eq!(config.provider, "openai");
        assert_eq!(config.model, "gpt-4o");
        assert_eq!(config.max_context_tokens, Some(100_000));
        assert_eq!(config.allowed_tools.len(), 2);
    }

    #[test]
    fn test_default_subagents_config() {
        let config = SubagentsConfig::default();

        // Explore uses Haiku
        assert_eq!(config.explore.model, "claude-haiku-4-5-20251001");
        assert_eq!(config.explore.provider, "anthropic");
        assert!(!config.explore.allowed_tools.is_empty());

        // Plan uses Sonnet
        assert_eq!(config.plan.model, "claude-sonnet-4-20250514");
        assert_eq!(config.plan.provider, "anthropic");
        assert!(config.plan.allowed_tools.is_empty()); // All tools

        // Review uses GPT-4o-mini
        assert_eq!(config.review.model, "gpt-4o-mini");
        assert_eq!(config.review.provider, "openai");
    }

    #[test]
    fn test_get_agent_config() {
        let config = SubagentsConfig::default();

        assert_eq!(config.get("explore").model, "claude-haiku-4-5-20251001");
        assert_eq!(config.get("plan").model, "claude-sonnet-4-20250514");
        assert_eq!(config.get("review").model, "gpt-4o-mini");
        // Unknown type returns default
        assert_eq!(config.get("unknown").model, "claude-haiku-4-5-20251001");
    }

    #[test]
    fn test_get_mut_agent_config() {
        let mut config = SubagentsConfig::default();

        config.get_mut("explore").model = "gpt-4o".to_string();
        assert_eq!(config.explore.model, "gpt-4o");
    }

    #[test]
    fn test_agent_types() {
        let types = SubagentsConfig::agent_types();
        assert_eq!(types, &["explore", "plan", "review"]);
    }

    #[test]
    fn test_iter() {
        let config = SubagentsConfig::default();
        let items: Vec<_> = config.iter().collect();

        assert_eq!(items.len(), 3);
        assert_eq!(items[0].0, "explore");
        assert_eq!(items[1].0, "plan");
        assert_eq!(items[2].0, "review");
    }

    #[test]
    fn test_serialization_roundtrip() {
        let config = SubagentsConfig::default();
        let toml_str = toml::to_string_pretty(&config).unwrap();
        let parsed: SubagentsConfig = toml::from_str(&toml_str).unwrap();

        assert_eq!(parsed.explore.model, config.explore.model);
        assert_eq!(parsed.plan.model, config.plan.model);
        assert_eq!(parsed.review.model, config.review.model);
    }

    #[test]
    fn test_parse_custom_toml() {
        let toml_content = r#"
[explore]
model = "gpt-4o-mini"
provider = "openai"
max_context_tokens = 40000
allowed_tools = ["file_read", "grep"]

[plan]
model = "claude-opus-4-20250514"
provider = "anthropic"

[review]
model = "gemini-2.0-flash"
provider = "gemini"
max_context_tokens = 20000

[default]
model = "claude-haiku-4-5-20251001"
provider = "anthropic"
"#;

        let config: SubagentsConfig = toml::from_str(toml_content).unwrap();
        assert_eq!(config.explore.model, "gpt-4o-mini");
        assert_eq!(config.explore.provider, "openai");
        assert_eq!(config.explore.max_context_tokens, Some(40000));
        assert_eq!(config.explore.allowed_tools, vec!["file_read", "grep"]);

        assert_eq!(config.plan.model, "claude-opus-4-20250514");
        assert_eq!(config.review.model, "gemini-2.0-flash");
        assert_eq!(config.review.provider, "gemini");
    }
}
