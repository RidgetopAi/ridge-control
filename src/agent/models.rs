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
    #[allow(dead_code)]
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

    #[allow(dead_code)]
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

    /// Sync local models with default URL probing
    #[allow(dead_code)]
    pub fn sync_ollama_models(&mut self) {
        self.sync_ollama_models_with_url(None);
    }

    /// Sync local models by querying Ollama or llama-server.
    /// Removes stale hardcoded entries and adds all discovered models.
    /// If `base_url` is provided, probes that URL first.
    pub fn sync_ollama_models_with_url(&mut self, base_url: Option<&str>) {
        // Remove existing hardcoded ollama models
        self.models.retain(|_, m| m.provider != "ollama");

        let base_url_owned = base_url.map(|s| s.to_string());

        // Try configured URL first, then defaults (blocking — called during init)
        let result = std::thread::spawn(move || {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .ok();
            rt.and_then(|rt| {
                rt.block_on(async {
                    let client = reqwest::Client::builder()
                        .timeout(std::time::Duration::from_secs(3))
                        .build()
                        .ok()?;

                    // Build list of URLs to try: configured URL first, then defaults
                    let mut urls_to_try: Vec<String> = Vec::new();
                    if let Some(ref url) = base_url_owned {
                        urls_to_try.push(url.clone());
                    }
                    urls_to_try.push("http://localhost:11434".to_string());
                    urls_to_try.push("http://localhost:8090".to_string());
                    urls_to_try.push("http://localhost:8080".to_string());
                    urls_to_try.dedup();

                    for url in &urls_to_try {
                        // Try /api/tags (works for both Ollama and llama-server)
                        if let Ok(resp) = client.get(format!("{}/api/tags", url)).send().await {
                            if resp.status().is_success() {
                                if let Ok(json) = resp.json::<serde_json::Value>().await {
                                    if let Some(models) = json["models"].as_array() {
                                        if !models.is_empty() {
                                            return Some(("ollama", serde_json::Value::Array(models.clone())));
                                        }
                                    }
                                }
                            }
                        }

                        // Try /v1/models (OpenAI-compatible)
                        if let Ok(resp) = client.get(format!("{}/v1/models", url)).send().await {
                            if resp.status().is_success() {
                                if let Ok(json) = resp.json::<serde_json::Value>().await {
                                    if json["data"].is_array() {
                                        return Some(("llama-server", json));
                                    }
                                }
                            }
                        }
                    }

                    None
                })
            })
        })
        .join()
        .unwrap_or(None);

        let Some((server_type, data)) = result else {
            tracing::debug!("No local LLM server available for model catalog sync");
            return;
        };

        match server_type {
            "ollama" => {
                let models = data.as_array().unwrap_or(&Vec::new()).clone();
                for model in models {
                    let name = match model["name"].as_str() {
                        Some(n) => n,
                        None => continue,
                    };
                    let family = model["details"]["family"].as_str().unwrap_or("");
                    let param_size_str = model["details"]["parameter_size"].as_str().unwrap_or("");

                    if family.contains("bert") || name.contains("embed") {
                        continue;
                    }

                    let context_window = if family.contains("qwen35") {
                        262_144
                    } else {
                        32_768
                    };
                    let has_thinking = family.contains("qwen3");

                    let mut info = ModelInfo::new(name, context_window, 8_192, TokenizerKind::Heuristic, "ollama");
                    if has_thinking {
                        info = info.with_thinking();
                    }
                    tracing::info!("Catalog: registered Ollama model {} ({}, {})", name, family, param_size_str);
                    self.register(info);
                }
            }
            "llama-server" => {
                let models = data["data"].as_array().unwrap_or(&Vec::new()).clone();
                for model in models {
                    let id = match model["id"].as_str() {
                        Some(n) => n,
                        None => continue,
                    };
                    let lower = id.to_lowercase();
                    if lower.contains("embed") {
                        continue;
                    }

                    let context_window = if lower.contains("qwen3.5") || lower.contains("qwen35") {
                        262_144
                    } else {
                        32_768
                    };
                    let has_thinking = lower.contains("qwen3") || lower.contains("deepseek");

                    let mut info = ModelInfo::new(id, context_window, 8_192, TokenizerKind::Heuristic, "ollama");
                    if has_thinking {
                        info = info.with_thinking();
                    }
                    tracing::info!("Catalog: registered llama-server model {}", id);
                    self.register(info);
                }
            }
            _ => {}
        }
    }

    /// List all known model names
    #[allow(dead_code)]
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
        // Claude 4.5 series (latest)
        self.register(
            ModelInfo::new("claude-opus-4-5-20251101", 200_000, 16_384, TokenizerKind::Claude, "anthropic")
                .with_thinking()
        );
        self.register(
            ModelInfo::new("claude-sonnet-4-5-20250929", 200_000, 16_384, TokenizerKind::Claude, "anthropic")
                .with_thinking()
        );
        self.register(
            ModelInfo::new("claude-haiku-4-5-20251001", 200_000, 8_192, TokenizerKind::Claude, "anthropic")
                .with_thinking()
        );
        // Claude 4 series
        self.register(
            ModelInfo::new("claude-sonnet-4-20250514", 200_000, 8_192, TokenizerKind::Claude, "anthropic")
                .with_thinking()
        );
        self.register(
            ModelInfo::new("claude-opus-4-20250514", 200_000, 8_192, TokenizerKind::Claude, "anthropic")
                .with_thinking()
        );
        // Claude 3.5 series
        self.register(
            ModelInfo::new("claude-3-5-sonnet-20241022", 200_000, 8_192, TokenizerKind::Claude, "anthropic")
        );
        self.register(
            ModelInfo::new("claude-3-5-haiku-20241022", 200_000, 8_192, TokenizerKind::Claude, "anthropic")
        );
        // Claude 3 series (legacy)
        self.register(
            ModelInfo::new("claude-3-opus-20240229", 200_000, 4_096, TokenizerKind::Claude, "anthropic")
        );
        self.register(
            ModelInfo::new("claude-3-haiku-20240307", 200_000, 4_096, TokenizerKind::Claude, "anthropic")
        );

        // ─────────────────────────────────────────────────────────────────────
        // OpenAI Models
        // ─────────────────────────────────────────────────────────────────────
        // GPT-5 series (latest)
        self.register(
            ModelInfo::new("gpt-5.4-2026-03-05", 256_000, 32_768, TokenizerKind::GptLike, "openai")
        );
        self.register(
            ModelInfo::new("gpt-5.4-pro-2026-03-05", 256_000, 32_768, TokenizerKind::GptLike, "openai")
                .with_thinking()
        );
        self.register(
            ModelInfo::new("gpt-5.2-2025-12-11", 256_000, 32_768, TokenizerKind::GptLike, "openai")
        );
        self.register(
            ModelInfo::new("gpt-5.2-pro-2025-12-11", 256_000, 32_768, TokenizerKind::GptLike, "openai")
                .with_thinking()
        );
        self.register(
            ModelInfo::new("gpt-5-mini-2025-08-07", 128_000, 16_384, TokenizerKind::GptLike, "openai")
        );
        // GPT-4 series
        self.register(
            ModelInfo::new("gpt-4o", 128_000, 16_384, TokenizerKind::GptLike, "openai")
        );
        self.register(
            ModelInfo::new("gpt-4o-mini", 128_000, 16_384, TokenizerKind::GptLike, "openai")
        );
        self.register(
            ModelInfo::new("gpt-4-turbo", 128_000, 4_096, TokenizerKind::GptLike, "openai")
        );
        // o-series (reasoning)
        self.register(
            ModelInfo::new("o1", 200_000, 100_000, TokenizerKind::GptLike, "openai")
                .with_thinking()
        );
        self.register(
            ModelInfo::new("o1-mini", 128_000, 65_536, TokenizerKind::GptLike, "openai")
                .with_thinking()
        );
        self.register(
            ModelInfo::new("o3-mini", 200_000, 100_000, TokenizerKind::GptLike, "openai")
                .with_thinking()
        );

        // ─────────────────────────────────────────────────────────────────────
        // Google Gemini Models
        // ─────────────────────────────────────────────────────────────────────
        // Gemini 2.5 series (latest)
        self.register(
            ModelInfo::new("gemini-2.5-flash", 1_000_000, 8_192, TokenizerKind::Gemini, "gemini")
        );
        self.register(
            ModelInfo::new("gemini-2.5-pro", 1_000_000, 8_192, TokenizerKind::Gemini, "gemini")
                .with_thinking()
        );
        // Gemini 2.0 series
        self.register(
            ModelInfo::new("gemini-2.0-flash", 1_000_000, 8_192, TokenizerKind::Gemini, "gemini")
        );
        // Gemini 1.5 series
        self.register(
            ModelInfo::new("gemini-1.5-pro", 2_000_000, 8_192, TokenizerKind::Gemini, "gemini")
        );
        self.register(
            ModelInfo::new("gemini-1.5-flash", 1_000_000, 8_192, TokenizerKind::Gemini, "gemini")
        );

        // ─────────────────────────────────────────────────────────────────────
        // xAI Grok Models
        // ─────────────────────────────────────────────────────────────────────
        // Grok 4 series (latest)
        self.register(
            ModelInfo::new("grok-4", 256_000, 32_768, TokenizerKind::GptLike, "grok")
                .with_thinking()
        );
        self.register(
            ModelInfo::new("grok-4-fast-reasoning", 2_000_000, 32_768, TokenizerKind::GptLike, "grok")
                .with_thinking()
        );
        self.register(
            ModelInfo::new("grok-4-fast-non-reasoning", 2_000_000, 32_768, TokenizerKind::GptLike, "grok")
        );
        self.register(
            ModelInfo::new("grok-4-1-fast-reasoning", 2_000_000, 32_768, TokenizerKind::GptLike, "grok")
                .with_thinking()
        );
        self.register(
            ModelInfo::new("grok-4-1-fast-non-reasoning", 2_000_000, 32_768, TokenizerKind::GptLike, "grok")
        );
        self.register(
            ModelInfo::new("grok-code-fast-1", 256_000, 32_768, TokenizerKind::GptLike, "grok")
                .with_thinking()
        );
        // Grok 3 series
        self.register(
            ModelInfo::new("grok-3", 131_072, 16_384, TokenizerKind::GptLike, "grok")
        );
        self.register(
            ModelInfo::new("grok-3-mini", 131_072, 16_384, TokenizerKind::GptLike, "grok")
        );
        // Grok 2 series (legacy)
        self.register(
            ModelInfo::new("grok-2-1212", 131_072, 8_192, TokenizerKind::GptLike, "grok")
        );
        self.register(
            ModelInfo::new("grok-2-vision-1212", 32_768, 8_192, TokenizerKind::GptLike, "grok")
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

        // ─────────────────────────────────────────────────────────────────────
        // Ollama Local Models
        // ─────────────────────────────────────────────────────────────────────
        self.register(
            ModelInfo::new("qwen3:8b", 32_768, 8_192, TokenizerKind::Heuristic, "ollama")
                .with_thinking()
        );
        self.register(
            ModelInfo::new("qwen3:4b", 32_768, 8_192, TokenizerKind::Heuristic, "ollama")
                .with_thinking()
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
        // Updated for 2M context (TS-010 fix)
        assert_eq!(info.max_context_tokens, 2_000_000);
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
