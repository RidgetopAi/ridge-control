//! Context management - intelligent truncation and context window management

use std::sync::Arc;

use crate::llm::types::{LLMRequest, Message, ToolDefinition};

use super::models::ModelCatalog;
use super::tokens::TokenCounter;

/// Kind of context segment for priority-based retention
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SegmentKind {
    /// System prompt (highest priority - always kept)
    System,
    /// Static instructions/guidelines
    Instructions,
    /// Repository context (files, structure)
    RepoContext,
    /// Chat history (user/assistant turns)
    ChatHistory,
    /// Tool use + result pairs (kept together)
    ToolExchange,
    /// Summarized older context
    Summary,
}

/// A segment of context with its messages and metadata
#[derive(Debug, Clone)]
pub struct ContextSegment {
    pub kind: SegmentKind,
    pub messages: Vec<Message>,
    /// Cached token count (computed once)
    pub token_count: Option<u32>,
    /// Timestamp for ordering (newer = higher)
    pub sequence: u64,
}

impl ContextSegment {
    pub fn new(kind: SegmentKind, messages: Vec<Message>, sequence: u64) -> Self {
        Self {
            kind,
            messages,
            token_count: None,
            sequence,
        }
    }

    pub fn system(text: impl Into<String>, sequence: u64) -> Self {
        Self::new(
            SegmentKind::System,
            vec![Message::user(text)], // System is handled separately in LLMRequest
            sequence,
        )
    }

    pub fn chat(messages: Vec<Message>, sequence: u64) -> Self {
        Self::new(SegmentKind::ChatHistory, messages, sequence)
    }

    pub fn tool_exchange(messages: Vec<Message>, sequence: u64) -> Self {
        Self::new(SegmentKind::ToolExchange, messages, sequence)
    }
}

/// Parameters for building a context request
#[derive(Debug, Clone)]
pub struct BuildContextParams {
    /// Model to use
    pub model: String,
    /// System prompt text
    pub system_prompt: Option<String>,
    /// Short system prompt for budget constraints
    pub short_system_prompt: Option<String>,
    /// Tool definitions
    pub tools: Vec<ToolDefinition>,
    /// All context segments to consider
    pub segments: Vec<ContextSegment>,
    /// Maximum output tokens to reserve
    pub max_output_tokens: Option<u32>,
}

/// Result of building a context-aware request
#[derive(Debug)]
pub struct BuiltContext {
    /// The request ready to send to LLM
    pub request: LLMRequest,
    /// Total tokens used (estimated)
    pub total_tokens: u32,
    /// Available token budget
    pub budget: u32,
    /// Whether any context was truncated
    pub truncated: bool,
    /// Number of segments included
    pub segments_included: usize,
    /// Number of segments dropped
    pub segments_dropped: usize,
}

/// Manages context window budget and builds optimized requests
pub struct ContextManager {
    catalog: Arc<ModelCatalog>,
    counter: Arc<dyn TokenCounter>,
    /// Safety buffer percentage (default 2%)
    safety_margin_percent: u32,
}

impl ContextManager {
    pub fn new(catalog: Arc<ModelCatalog>, counter: Arc<dyn TokenCounter>) -> Self {
        Self {
            catalog,
            counter,
            safety_margin_percent: 2,
        }
    }

    pub fn with_safety_margin(mut self, percent: u32) -> Self {
        self.safety_margin_percent = percent;
        self
    }

    /// Build an LLMRequest with intelligent context truncation
    pub fn build_request(&self, params: BuildContextParams) -> BuiltContext {
        let model_info = self.catalog.info_for(&params.model);

        // Calculate budget
        let max_output = params
            .max_output_tokens
            .unwrap_or(model_info.default_max_output_tokens);
        let safety_buffer =
            (model_info.max_context_tokens * self.safety_margin_percent) / 100;
        let budget = model_info
            .max_context_tokens
            .saturating_sub(max_output)
            .saturating_sub(safety_buffer);

        // Count always-preserved content
        let system_tokens = params
            .system_prompt
            .as_ref()
            .map(|s| self.counter.count_text(&params.model, s))
            .unwrap_or(0);

        let tools_tokens = self.count_tools(&params.model, &params.tools);

        // Find and preserve the last user turn (including any tool exchanges)
        let (last_turn_segments, older_segments) = self.split_last_turn(&params.segments);
        let last_turn_tokens: u32 = last_turn_segments
            .iter()
            .map(|s| self.count_segment(&params.model, s))
            .sum();

        let preserved_tokens = system_tokens + tools_tokens + last_turn_tokens;

        // Check if we need to use short system prompt
        let (final_system, system_used_tokens) = if preserved_tokens > budget {
            // Try with short system prompt
            let short_tokens = params
                .short_system_prompt
                .as_ref()
                .map(|s| self.counter.count_text(&params.model, s))
                .unwrap_or(0);
            (params.short_system_prompt.clone(), short_tokens)
        } else {
            (params.system_prompt.clone(), system_tokens)
        };

        let preserved_tokens = system_used_tokens + tools_tokens + last_turn_tokens;
        let mut remaining_budget = budget.saturating_sub(preserved_tokens);

        // Fill remaining budget with older segments (newest first)
        let mut included_segments: Vec<&ContextSegment> = Vec::new();
        let mut segments_dropped = 0;

        // Sort older segments by sequence (newest first)
        let mut older_sorted: Vec<&ContextSegment> = older_segments.clone();
        older_sorted.sort_by(|a, b| b.sequence.cmp(&a.sequence));

        for segment in &older_sorted {
            let seg_tokens = self.count_segment(&params.model, segment);
            if seg_tokens <= remaining_budget {
                included_segments.push(segment);
                remaining_budget = remaining_budget.saturating_sub(seg_tokens);
            } else {
                segments_dropped += 1;
            }
        }

        // Reverse to maintain chronological order
        included_segments.reverse();

        // Build final message list
        let mut messages: Vec<Message> = Vec::new();
        for seg in &included_segments {
            messages.extend(seg.messages.clone());
        }
        for seg in &last_turn_segments {
            messages.extend(seg.messages.clone());
        }

        let total_tokens = budget.saturating_sub(remaining_budget);

        let request = LLMRequest {
            model: params.model.clone(),
            system: final_system,
            messages,
            tools: params.tools,
            max_tokens: Some(max_output),
            stream: true,
            ..Default::default()
        };

        BuiltContext {
            request,
            total_tokens,
            budget,
            truncated: segments_dropped > 0,
            segments_included: included_segments.len() + last_turn_segments.len(),
            segments_dropped,
        }
    }

    fn count_tools(&self, model: &str, tools: &[ToolDefinition]) -> u32 {
        let mut total = 0u32;
        for tool in tools {
            total += self.counter.count_text(model, &tool.name);
            total += self.counter.count_text(model, &tool.description);
            total += self.counter.count_text(model, &tool.input_schema.to_string());
            total += 20; // overhead per tool
        }
        total
    }

    fn count_segment(&self, model: &str, segment: &ContextSegment) -> u32 {
        segment.token_count.unwrap_or_else(|| {
            self.counter.count_messages(model, &segment.messages)
        })
    }

    /// Split segments into last turn (always preserved) and older segments
    fn split_last_turn<'a>(
        &self,
        segments: &'a [ContextSegment],
    ) -> (Vec<&'a ContextSegment>, Vec<&'a ContextSegment>) {
        if segments.is_empty() {
            return (Vec::new(), Vec::new());
        }

        // Find the last user message and include all tool exchanges after it
        let mut last_turn_start = segments.len();
        let mut in_tool_sequence = false;

        for (i, seg) in segments.iter().enumerate().rev() {
            match seg.kind {
                SegmentKind::ToolExchange => {
                    in_tool_sequence = true;
                    last_turn_start = i;
                }
                SegmentKind::ChatHistory => {
                    last_turn_start = i;
                    if !in_tool_sequence {
                        break;
                    }
                    in_tool_sequence = false;
                }
                _ => {
                    if !in_tool_sequence {
                        break;
                    }
                }
            }
        }

        let (older, last) = segments.split_at(last_turn_start);
        (last.iter().collect(), older.iter().collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::tokens::DefaultTokenCounter;

    #[test]
    fn test_context_manager_basic() {
        let catalog = Arc::new(ModelCatalog::new());
        let counter = Arc::new(DefaultTokenCounter::new(catalog.clone()));
        let manager = ContextManager::new(catalog, counter);

        let params = BuildContextParams {
            model: "gpt-4o".to_string(),
            system_prompt: Some("You are a helpful assistant.".to_string()),
            short_system_prompt: Some("Be helpful.".to_string()),
            tools: vec![],
            segments: vec![ContextSegment::chat(
                vec![Message::user("Hello"), Message::assistant("Hi there!")],
                1,
            )],
            max_output_tokens: Some(4096),
        };

        let built = manager.build_request(params);
        assert!(!built.truncated);
        assert!(built.total_tokens < built.budget);
    }
}
