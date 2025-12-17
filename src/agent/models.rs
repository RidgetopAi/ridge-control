//! Model metadata - context windows, tokenizers, and capabilities

use std::collections::HashMap;

/// Tokenizer type for a model family
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TokenizerKind {
    /// Claude models - use claude-tokenizer crate
    Claude,
    /// GPT/OpenAI-compatible - use tiktoken-style BPE
    GptLike,
    /// Gemini models - use character heuristic (no public tokenizer)
    Gemini,
    /// Fallback heuristic (chars / 4)
    Heuristic,
}

/// Metadata about a specific model
#[derive(Debug, Clone)]
pub struct ModelInfo {
    /// Model identifier (e.g., "claude-sonnet-4-20250514")
    pub name: String,
    /// Maximum context window in tokens
    pub max_context_tokens: u32,
    /// Default max output tokens
    pub default_max_output_tokens: u32,
    /// Tokenizer to use for this model
    pub tokenizer: TokenizerKind,
    /// Whether model supports tool use
    pub supports_tools: bool,
    /// Whether model supports extended thinking
    pub supports_thinking: bool,
    /// Provider name (e.g., "anthropic", "openai")
    pub provider: String,
}

impl ModelInfo {
    pub fn new(
        name: impl Into<String>,
        max_context_tokens: u32,
        default_max_output_tokens: u32,
        tokenizer: TokenizerKind,
        provider: impl Into<String>,
    ) -> Self {
        Self {
            name: name.into(),
            max_context_tokens,
            default_max_output_tokens,
            tokenizer,
            supports_tools: true,
            supports_thinking: false,
            provider: provider.into(),
        }
    }

    pub fn with_thinking(mut self) -> Self {
        self.supports_thinking = true;
        self
    }

    pub fn without_tools(mut self) -> Self {
        self.supports_tools = false;
        self
    }
}

/// Catalog of known models with their metadata
#[derive(Debug, Clone)]
pub struct ModelCatalog {
    models: HashMap<String, ModelInfo>,
}

impl ModelCatalog {
    /// Create a new catalog seeded with known models
    pub fn new() -> Self {
        let mut catalog = Self {
            models: HashMap::new(),
        };
        catalog.seed_defaults();
        catalog
    }

    /// Get model info by name, with fuzzy matching for version-less lookups
    pub fn get(&self, model: &str) -> Option<&ModelInfo> {
        // Exact match first
        if let Some(info) = self.models.get(model) {
            return Some(info);
        }

        // Try prefix match for versioned models (e.g., "claude-sonnet-4" matches "claude-sonnet-4-20250514")
        for (name, info) in &self.models {
            if name.starts_with(model) || model.starts_with(name.split('-').take(3).collect::<Vec<_>>().join("-").as_str()) {
                return Some(info);
            }
        }

        None
    }

    /// Get model info, falling back to heuristic defaults
    pub fn info_for(&self, model: &str) -> ModelInfo {
        self.get(model).cloned().unwrap_or_else(|| {
            // Default fallback for unknown models
            ModelInfo::new(
                model,
                128_000,  // Conservative default
                4_096,
                TokenizerKind::Heuristic,
                "unknown",
            )
        })
    }

    /// Register a custom model
    pub fn register(&mut self, info: ModelInfo) {
        self.models.insert(info.name.clone(), info);
    }

    /// List all known model names
    pub fn list(&self) -> Vec<&str> {
        self.models.keys().map(|s| s.as_str()).collect()
    }

    /// List all unique provider names
    pub fn providers(&self) -> Vec<&str> {
        let mut providers: Vec<&str> = self.models.values()
            .map(|m| m.provider.as_str())
            .collect::<std::collections::HashSet<_>>()
            .into_iter()
            .collect();
        providers.sort();
        providers
    }

    /// List all models for a specific provider
    pub fn models_for_provider(&self, provider: &str) -> Vec<&str> {
        let mut models: Vec<&str> = self.models.values()
            .filter(|m| m.provider == provider)
            .map(|m| m.name.as_str())
            .collect();
        models.sort();
        models
    }

    fn seed_defaults(&mut self) {
        // ─────────────────────────────────────────────────────────────────────
        // Anthropic Claude Models
        // ─────────────────────────────────────────────────────────────────────
        self.register(
            ModelInfo::new("claude-sonnet-4-20250514", 200_000, 8_192, TokenizerKind::Claude, "anthropic")
                .with_thinking()
        );
        self.register(
            ModelInfo::new("claude-opus-4-20250514", 200_000, 8_192, TokenizerKind::Claude, "anthropic")
                .with_thinking()
        );
        self.register(
            ModelInfo::new("claude-3-5-sonnet-20241022", 200_000, 8_192, TokenizerKind::Claude, "anthropic")
        );
        self.register(
            ModelInfo::new("claude-3-5-haiku-20241022", 200_000, 8_192, TokenizerKind::Claude, "anthropic")
        );
        self.register(
            ModelInfo::new("claude-3-opus-20240229", 200_000, 4_096, TokenizerKind::Claude, "anthropic")
        );
        self.register(
            ModelInfo::new("claude-3-haiku-20240307", 200_000, 4_096, TokenizerKind::Claude, "anthropic")
        );

        // ─────────────────────────────────────────────────────────────────────
        // OpenAI Models
        // ─────────────────────────────────────────────────────────────────────
        self.register(
            ModelInfo::new("gpt-4o", 128_000, 4_096, TokenizerKind::GptLike, "openai")
        );
        self.register(
            ModelInfo::new("gpt-4o-mini", 128_000, 16_384, TokenizerKind::GptLike, "openai")
        );
        self.register(
            ModelInfo::new("gpt-4-turbo", 128_000, 4_096, TokenizerKind::GptLike, "openai")
        );
        self.register(
            ModelInfo::new("gpt-4", 8_192, 4_096, TokenizerKind::GptLike, "openai")
        );
        self.register(
            ModelInfo::new("o1", 200_000, 100_000, TokenizerKind::GptLike, "openai")
        );
        self.register(
            ModelInfo::new("o1-mini", 128_000, 65_536, TokenizerKind::GptLike, "openai")
        );
        self.register(
            ModelInfo::new("o3-mini", 200_000, 100_000, TokenizerKind::GptLike, "openai")
        );

        // ─────────────────────────────────────────────────────────────────────
        // Google Gemini Models
        // ─────────────────────────────────────────────────────────────────────
        self.register(
            ModelInfo::new("gemini-1.5-pro", 1_000_000, 8_192, TokenizerKind::Gemini, "gemini")
        );
        self.register(
            ModelInfo::new("gemini-1.5-flash", 1_000_000, 8_192, TokenizerKind::Gemini, "gemini")
        );
        self.register(
            ModelInfo::new("gemini-2.0-flash", 1_000_000, 8_192, TokenizerKind::Gemini, "gemini")
        );
        self.register(
            ModelInfo::new("gemini-2.0-flash-thinking-exp", 1_000_000, 8_192, TokenizerKind::Gemini, "gemini")
                .with_thinking()
        );

        // ─────────────────────────────────────────────────────────────────────
        // xAI Grok Models
        // ─────────────────────────────────────────────────────────────────────
        self.register(
            ModelInfo::new("grok-2", 131_072, 4_096, TokenizerKind::GptLike, "grok")
        );
        self.register(
            ModelInfo::new("grok-2-mini", 131_072, 4_096, TokenizerKind::GptLike, "grok")
        );
        self.register(
            ModelInfo::new("grok-beta", 131_072, 4_096, TokenizerKind::GptLike, "grok")
        );

        // ─────────────────────────────────────────────────────────────────────
        // Groq Models (fast inference)
        // ─────────────────────────────────────────────────────────────────────
        self.register(
            ModelInfo::new("llama-3.3-70b-versatile", 128_000, 8_192, TokenizerKind::GptLike, "groq")
        );
        self.register(
            ModelInfo::new("llama-3.1-70b-versatile", 128_000, 8_192, TokenizerKind::GptLike, "groq")
        );
        self.register(
            ModelInfo::new("llama-3.1-8b-instant", 128_000, 8_192, TokenizerKind::GptLike, "groq")
        );
        self.register(
            ModelInfo::new("mixtral-8x7b-32768", 32_768, 4_096, TokenizerKind::GptLike, "groq")
        );
        self.register(
            ModelInfo::new("gemma2-9b-it", 8_192, 4_096, TokenizerKind::GptLike, "groq")
        );
    }
}

impl Default for ModelCatalog {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_catalog_exact_match() {
        let catalog = ModelCatalog::new();
        let info = catalog.get("claude-sonnet-4-20250514");
        assert!(info.is_some());
        let info = info.unwrap();
        assert_eq!(info.max_context_tokens, 200_000);
        assert!(info.supports_thinking);
    }

    #[test]
    fn test_catalog_fallback() {
        let catalog = ModelCatalog::new();
        let info = catalog.info_for("unknown-model-xyz");
        assert_eq!(info.max_context_tokens, 128_000);
        assert_eq!(info.tokenizer, TokenizerKind::Heuristic);
    }

    #[test]
    fn test_catalog_openai() {
        let catalog = ModelCatalog::new();
        let info = catalog.get("gpt-4o").unwrap();
        assert_eq!(info.provider, "openai");
        assert_eq!(info.tokenizer, TokenizerKind::GptLike);
    }

    #[test]
    fn test_catalog_gemini() {
        let catalog = ModelCatalog::new();
        let info = catalog.get("gemini-1.5-pro").unwrap();
        assert_eq!(info.max_context_tokens, 1_000_000);
    }

    #[test]
    fn test_catalog_providers() {
        let catalog = ModelCatalog::new();
        let providers = catalog.providers();
        assert!(providers.contains(&"anthropic"));
        assert!(providers.contains(&"openai"));
        assert!(providers.contains(&"gemini"));
        assert!(providers.contains(&"grok"));
        assert!(providers.contains(&"groq"));
    }

    #[test]
    fn test_catalog_models_for_provider() {
        let catalog = ModelCatalog::new();
        let anthropic_models = catalog.models_for_provider("anthropic");
        assert!(anthropic_models.iter().any(|m| m.contains("claude")));
        
        let openai_models = catalog.models_for_provider("openai");
        assert!(openai_models.iter().any(|m| m.contains("gpt")));
    }
}
