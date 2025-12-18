// Provider trait and types - some fields/methods for future use
#![allow(dead_code)]

use std::collections::HashMap;
use std::pin::Pin;
use std::sync::Arc;

use async_trait::async_trait;
use futures::Stream;

use super::types::{ContentBlock, LLMError, LLMRequest, LLMResponse, StreamChunk};

/// Stream of LLM chunks - boxed for trait object safety
pub type StreamBox = Pin<Box<dyn Stream<Item = Result<StreamChunk, LLMError>> + Send>>;

/// Unified LLM provider interface
#[async_trait]
pub trait Provider: Send + Sync {
    /// Provider name (e.g., "anthropic", "openai", "google")
    fn name(&self) -> &str;

    /// Available models for this provider
    fn models(&self) -> &[ModelInfo];

    /// Default model for this provider
    fn default_model(&self) -> &str;

    /// Check if provider supports a capability
    fn supports(&self, capability: Capability) -> bool;

    /// Send a non-streaming request
    async fn complete(&self, request: LLMRequest) -> Result<LLMResponse, LLMError>;

    /// Send a streaming request, returns a stream of chunks
    async fn stream(&self, request: LLMRequest) -> Result<StreamBox, LLMError>;

    /// Count tokens for a request (optional - default estimates)
    async fn count_tokens(&self, request: &LLMRequest) -> Result<u32, LLMError> {
        let chars: usize = request
            .messages
            .iter()
            .flat_map(|m| m.content.iter())
            .map(|c| match c {
                ContentBlock::Text(t) => t.len(),
                ContentBlock::Thinking(t) => t.len(),
                _ => 100,
            })
            .sum();
        Ok((chars / 4) as u32)
    }

    /// Test if the API key is valid by making a minimal API call (TS-007)
    /// Returns Ok(()) if key is valid, Err with specific error otherwise
    async fn test_key(&self) -> Result<(), LLMError>;
}

/// Model information
#[derive(Debug, Clone)]
pub struct ModelInfo {
    pub id: String,
    pub name: String,
    pub context_window: u32,
    pub max_output_tokens: u32,
    pub supports_vision: bool,
    pub supports_tools: bool,
    pub supports_thinking: bool,
}

impl ModelInfo {
    pub fn new(id: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            context_window: 200_000,
            max_output_tokens: 8192,
            supports_vision: true,
            supports_tools: true,
            supports_thinking: false,
        }
    }

    pub fn with_thinking(mut self) -> Self {
        self.supports_thinking = true;
        self
    }

    pub fn with_context_window(mut self, tokens: u32) -> Self {
        self.context_window = tokens;
        self
    }

    pub fn with_max_output(mut self, tokens: u32) -> Self {
        self.max_output_tokens = tokens;
        self
    }
}

/// Provider capabilities
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Capability {
    Streaming,
    ToolUse,
    Vision,
    Thinking,
    Reasoning,
    CodeExecution,
    LiveSearch,
}

/// Registry for managing multiple providers
pub struct ProviderRegistry {
    providers: HashMap<String, Arc<dyn Provider>>,
    default_provider: String,
}

impl ProviderRegistry {
    pub fn new() -> Self {
        Self {
            providers: HashMap::new(),
            default_provider: String::new(),
        }
    }

    pub fn register(&mut self, provider: Arc<dyn Provider>) {
        if self.providers.is_empty() {
            self.default_provider = provider.name().to_string();
        }
        self.providers.insert(provider.name().to_string(), provider);
    }

    pub fn get(&self, name: &str) -> Option<Arc<dyn Provider>> {
        self.providers.get(name).cloned()
    }

    pub fn default(&self) -> Option<Arc<dyn Provider>> {
        self.get(&self.default_provider)
    }

    pub fn set_default(&mut self, name: &str) {
        if self.providers.contains_key(name) {
            self.default_provider = name.to_string();
        }
    }

    pub fn list(&self) -> Vec<&str> {
        self.providers.keys().map(|s| s.as_str()).collect()
    }

    pub fn is_empty(&self) -> bool {
        self.providers.is_empty()
    }
}

impl Default for ProviderRegistry {
    fn default() -> Self {
        Self::new()
    }
}
