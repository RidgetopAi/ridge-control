//! Token counting - per-model tokenizers with heuristic fallback

use std::sync::Arc;

use super::models::{ModelCatalog, ModelInfo, TokenizerKind};
use crate::llm::types::Message;

/// Trait for counting tokens in text and messages
pub trait TokenCounter: Send + Sync {
    /// Count tokens in raw text for a specific model
    fn count_text(&self, model: &str, text: &str) -> u32;

    /// Count tokens in a sequence of messages
    fn count_messages(&self, model: &str, messages: &[Message]) -> u32;
}

/// Default token counter using per-model tokenizers
pub struct DefaultTokenCounter {
    catalog: Arc<ModelCatalog>,
    cl100k: tiktoken_rs::CoreBPE,
}

impl DefaultTokenCounter {
    pub fn new(catalog: Arc<ModelCatalog>) -> Self {
        let cl100k = tiktoken_rs::cl100k_base().expect("Failed to load cl100k tokenizer");
        Self { catalog, cl100k }
    }

    fn count_with_tokenizer(&self, tokenizer: TokenizerKind, text: &str) -> u32 {
        match tokenizer {
            TokenizerKind::Claude => {
                // Claude uses a similar tokenizer to GPT-4, cl100k is close enough
                // For production accuracy, we'd use claude-tokenizer crate
                self.cl100k.encode_ordinary(text).len() as u32
            }
            TokenizerKind::GptLike => {
                self.cl100k.encode_ordinary(text).len() as u32
            }
            TokenizerKind::Gemini => {
                // Gemini doesn't have a public tokenizer, use heuristic
                // Their tokens are roughly similar to GPT-4
                self.cl100k.encode_ordinary(text).len() as u32
            }
            TokenizerKind::Heuristic => {
                // Fallback: ~4 characters per token on average
                (text.chars().count() as f64 / 4.0).ceil() as u32
            }
        }
    }
}

impl TokenCounter for DefaultTokenCounter {
    fn count_text(&self, model: &str, text: &str) -> u32 {
        let info = self.catalog.info_for(model);
        self.count_with_tokenizer(info.tokenizer, text)
    }

    fn count_messages(&self, model: &str, messages: &[Message]) -> u32 {
        let info = self.catalog.info_for(model);
        let mut total = 0u32;

        for message in messages {
            // Role overhead (approximately 4 tokens for role + formatting)
            total += 4;

            for block in &message.content {
                total += self.count_content_block(&info, block);
            }
        }

        // Message boundary overhead
        total += 3;

        total
    }
}

impl DefaultTokenCounter {
    fn count_content_block(&self, info: &ModelInfo, block: &crate::llm::types::ContentBlock) -> u32 {
        use crate::llm::types::ContentBlock;

        match block {
            ContentBlock::Text(text) => self.count_with_tokenizer(info.tokenizer, text),
            ContentBlock::Thinking(text) => self.count_with_tokenizer(info.tokenizer, text),
            ContentBlock::ToolUse(tool_use) => {
                // Tool name + JSON input
                let input_str = tool_use.input.to_string();
                self.count_with_tokenizer(info.tokenizer, &tool_use.name)
                    + self.count_with_tokenizer(info.tokenizer, &input_str)
                    + 10 // overhead for structure
            }
            ContentBlock::ToolResult(result) => {
                let content_tokens = match &result.content {
                    crate::llm::types::ToolResultContent::Text(t) => {
                        self.count_with_tokenizer(info.tokenizer, t)
                    }
                    crate::llm::types::ToolResultContent::Json(v) => {
                        self.count_with_tokenizer(info.tokenizer, &v.to_string())
                    }
                    crate::llm::types::ToolResultContent::Image(_) => 1000, // Rough estimate for images
                };
                content_tokens + 10 // overhead
            }
            ContentBlock::Image(_) => 1000, // Images are roughly 1000 tokens
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_token_counting_basic() {
        let catalog = Arc::new(ModelCatalog::new());
        let counter = DefaultTokenCounter::new(catalog);

        // "Hello, world!" should be a small number of tokens
        let tokens = counter.count_text("gpt-4o", "Hello, world!");
        assert!(tokens > 0 && tokens < 10);
    }

    #[test]
    fn test_token_counting_long_text() {
        let catalog = Arc::new(ModelCatalog::new());
        let counter = DefaultTokenCounter::new(catalog);

        let long_text = "Hello ".repeat(1000);
        let tokens = counter.count_text("claude-sonnet-4-20250514", &long_text);
        // Should be roughly 1000-2000 tokens
        assert!(tokens > 500 && tokens < 3000);
    }

    #[test]
    fn test_heuristic_fallback() {
        let catalog = Arc::new(ModelCatalog::new());
        let counter = DefaultTokenCounter::new(catalog);

        // Unknown model should use heuristic
        let text = "a".repeat(400);
        let tokens = counter.count_text("unknown-model", &text);
        // ~4 chars per token = ~100 tokens
        assert!((99..=101).contains(&tokens));
    }
}
