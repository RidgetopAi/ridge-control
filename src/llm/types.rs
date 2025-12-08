use std::collections::HashMap;

use serde::{Deserialize, Serialize};

/// Unified LLM request - provider-agnostic
#[derive(Debug, Clone)]
pub struct LLMRequest {
    /// Model identifier (e.g., "claude-sonnet-4-20250514", "gpt-4o")
    pub model: String,

    /// System prompt (positioned appropriately per provider)
    pub system: Option<String>,

    /// Conversation messages
    pub messages: Vec<Message>,

    /// Available tools for this request
    pub tools: Vec<ToolDefinition>,

    /// Maximum tokens to generate
    pub max_tokens: Option<u32>,

    /// Temperature (0.0-1.0)
    pub temperature: Option<f32>,

    /// Enable streaming
    pub stream: bool,

    /// Enable extended thinking (Anthropic-specific, ignored by others)
    pub thinking: Option<ThinkingConfig>,

    /// Provider-specific options (escape hatch)
    pub extra: HashMap<String, serde_json::Value>,
}

impl Default for LLMRequest {
    fn default() -> Self {
        Self {
            model: String::new(),
            system: None,
            messages: Vec::new(),
            tools: Vec::new(),
            max_tokens: Some(4096),
            temperature: None,
            stream: true,
            thinking: None,
            extra: HashMap::new(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct ThinkingConfig {
    pub enabled: bool,
    pub budget_tokens: Option<u32>,
}

/// A message in the conversation
#[derive(Debug, Clone)]
pub struct Message {
    pub role: Role,
    pub content: Vec<ContentBlock>,
}

impl Message {
    pub fn user(text: impl Into<String>) -> Self {
        Self {
            role: Role::User,
            content: vec![ContentBlock::Text(text.into())],
        }
    }

    pub fn assistant(text: impl Into<String>) -> Self {
        Self {
            role: Role::Assistant,
            content: vec![ContentBlock::Text(text.into())],
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    User,
    Assistant,
}

/// Content block within a message
#[derive(Debug, Clone)]
pub enum ContentBlock {
    /// Plain text
    Text(String),

    /// Image (base64 or URL)
    Image(ImageContent),

    /// Tool use request (assistant → user)
    ToolUse(ToolUse),

    /// Tool result (user → assistant)
    ToolResult(ToolResult),

    /// Thinking block (Anthropic extended thinking)
    Thinking(String),
}

#[derive(Debug, Clone)]
pub struct ImageContent {
    pub source: ImageSource,
    pub media_type: String,
}

#[derive(Debug, Clone)]
pub enum ImageSource {
    Base64(String),
    Url(String),
}

/// Tool definition for the LLM
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolDefinition {
    pub name: String,
    pub description: String,
    /// JSON Schema for input parameters
    pub input_schema: serde_json::Value,
}

/// Tool use request from the LLM
#[derive(Debug, Clone)]
pub struct ToolUse {
    /// Unique ID for this tool invocation
    pub id: String,
    /// Tool name
    pub name: String,
    /// Input arguments (JSON object)
    pub input: serde_json::Value,
}

/// Tool result to send back
#[derive(Debug, Clone)]
pub struct ToolResult {
    /// ID of the tool use this responds to
    pub tool_use_id: String,
    /// Result content (string or structured)
    pub content: ToolResultContent,
    /// Whether the tool execution failed
    pub is_error: bool,
}

#[derive(Debug, Clone)]
pub enum ToolResultContent {
    Text(String),
    Json(serde_json::Value),
    Image(ImageContent),
}

/// Streaming chunk from LLM
#[derive(Debug, Clone)]
pub enum StreamChunk {
    /// Stream started, message ID available
    Start { message_id: String },

    /// New content block started
    BlockStart { index: usize, block_type: BlockType },

    /// Delta for current block
    Delta(StreamDelta),

    /// Content block completed
    BlockStop { index: usize },

    /// Stop reason (generation complete)
    Stop {
        reason: StopReason,
        usage: Option<Usage>,
    },

    /// Error during streaming
    Error(LLMError),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlockType {
    Text,
    ToolUse,
    Thinking,
}

#[derive(Debug, Clone)]
pub enum StreamDelta {
    /// Text content delta
    Text(String),

    /// Tool use: partial JSON for input
    ToolInput {
        id: String,
        name: Option<String>,
        input_json: String,
    },

    /// Thinking delta
    Thinking(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StopReason {
    /// Natural end of response
    EndTurn,
    /// Max tokens reached
    MaxTokens,
    /// Stop sequence hit
    StopSequence,
    /// Tool use requested
    ToolUse,
    /// Content filtered
    ContentFilter,
}

#[derive(Debug, Clone, Default)]
pub struct Usage {
    pub input_tokens: u32,
    pub output_tokens: u32,
    pub thinking_tokens: Option<u32>,
}

/// Complete (non-streaming) LLM response
#[derive(Debug, Clone)]
pub struct LLMResponse {
    pub id: String,
    pub model: String,
    pub content: Vec<ContentBlock>,
    pub stop_reason: StopReason,
    pub usage: Usage,
}

/// LLM-specific errors
#[derive(Debug, Clone, thiserror::Error)]
pub enum LLMError {
    #[error("Authentication failed: {message}")]
    AuthError { message: String },

    #[error("Rate limited: retry after {retry_after_secs}s")]
    RateLimit { retry_after_secs: u32 },

    #[error("Invalid request: {message}")]
    InvalidRequest { message: String },

    #[error("Model not found: {model}")]
    ModelNotFound { model: String },

    #[error("Content filtered: {reason}")]
    ContentFiltered { reason: String },

    #[error("Provider error: {status} - {message}")]
    ProviderError { status: u16, message: String },

    #[error("Network error: {message}")]
    NetworkError { message: String },

    #[error("Stream interrupted")]
    StreamInterrupted,

    #[error("Timeout after {timeout_secs}s")]
    Timeout { timeout_secs: u32 },

    #[error("Parse error: {message}")]
    ParseError { message: String },
}
