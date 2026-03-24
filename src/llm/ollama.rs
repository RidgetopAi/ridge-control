// Ollama provider - local LLM via Ollama's OpenAI-compatible API
#![allow(dead_code)]

use async_trait::async_trait;
use futures::{Stream, StreamExt};
use reqwest::Client;
use serde::Deserialize;
use serde_json::json;
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;

use super::provider::{Capability, ModelInfo, Provider, StreamBox};
use super::types::{
    BlockType, ContentBlock, LLMError, LLMRequest, LLMResponse, StopReason, StreamChunk,
    StreamDelta, ToolUse, Usage,
};

const DEFAULT_OLLAMA_URL: &str = "http://localhost:11434";

/// What kind of local server we're talking to
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LocalServerKind {
    /// Ollama - uses /api/tags for discovery
    Ollama,
    /// llama-server (llama.cpp) - uses /v1/models and /health
    LlamaServer,
}

/// Ollama / llama-server local LLM provider
pub struct OllamaProvider {
    base_url: String,
    http_client: Client,
    models: Vec<ModelInfo>,
    default_model: String,
    server_kind: LocalServerKind,
}

impl OllamaProvider {
    pub fn new(base_url: Option<String>) -> Self {
        let base_url = base_url.unwrap_or_else(|| DEFAULT_OLLAMA_URL.to_string());
        let http_client = Client::builder()
            .timeout(std::time::Duration::from_secs(120))
            .build()
            .unwrap_or_default();

        // Default model list - auto-discovery can update this
        let models = vec![
            ModelInfo::new("qwen3:8b", "Qwen3 8B")
                .with_thinking()
                .with_context_window(32_768)
                .with_max_output(8_192),
            ModelInfo::new("qwen3:4b", "Qwen3 4B")
                .with_thinking()
                .with_context_window(32_768)
                .with_max_output(8_192),
        ];

        Self {
            base_url,
            http_client,
            models,
            default_model: "qwen3:8b".to_string(),
            server_kind: LocalServerKind::Ollama,
        }
    }

    /// Discover models from a local server and update the model list.
    /// Tries Ollama's /api/tags first, then falls back to llama-server's /v1/models.
    pub async fn discover_models(&mut self) -> Result<(), LLMError> {
        // Try Ollama first
        if let Ok(()) = self.discover_ollama_models().await {
            self.server_kind = LocalServerKind::Ollama;
            return Ok(());
        }

        // Fall back to llama-server /v1/models
        if let Ok(()) = self.discover_llama_server_models().await {
            self.server_kind = LocalServerKind::LlamaServer;
            return Ok(());
        }

        Err(LLMError::NetworkError {
            message: format!(
                "No local LLM server found at {} (tried Ollama /api/tags and llama-server /v1/models)",
                self.base_url
            ),
        })
    }

    /// Discover models via Ollama's /api/tags endpoint
    async fn discover_ollama_models(&mut self) -> Result<(), LLMError> {
        let url = format!("{}/api/tags", self.base_url);
        let response = self
            .http_client
            .get(&url)
            .timeout(std::time::Duration::from_secs(3))
            .send()
            .await
            .map_err(|e| LLMError::NetworkError {
                message: format!("Failed to reach Ollama at {}: {}", self.base_url, e),
            })?;

        if !response.status().is_success() {
            return Err(LLMError::ProviderError {
                status: response.status().as_u16(),
                message: "Failed to list Ollama models".to_string(),
            });
        }

        let tags: OllamaTagsResponse = response.json().await.map_err(|e| LLMError::ParseError {
            message: e.to_string(),
        })?;

        let discovered: Vec<ModelInfo> = tags
            .models
            .into_iter()
            .filter(|m| {
                // Skip embedding models
                !m.details.family.contains("bert") && !m.name.contains("embed")
            })
            .map(|m| {
                let has_thinking = m.details.family.contains("qwen3") || m.details.family.contains("qwen35");
                let ctx = if m.details.family.contains("qwen35") { 262_144 } else { 32_768 };
                let mut info = ModelInfo::new(&m.name, &m.name)
                    .with_context_window(ctx)
                    .with_max_output(8_192);
                if has_thinking {
                    info = info.with_thinking();
                }
                info
            })
            .collect();

        self.apply_discovered(discovered);
        Ok(())
    }

    /// Discover models via llama-server's /v1/models endpoint (OpenAI-compatible)
    async fn discover_llama_server_models(&mut self) -> Result<(), LLMError> {
        let url = format!("{}/v1/models", self.base_url);
        let response = self
            .http_client
            .get(&url)
            .timeout(std::time::Duration::from_secs(3))
            .send()
            .await
            .map_err(|e| LLMError::NetworkError {
                message: format!("Failed to reach llama-server at {}: {}", self.base_url, e),
            })?;

        if !response.status().is_success() {
            return Err(LLMError::ProviderError {
                status: response.status().as_u16(),
                message: "Failed to list llama-server models".to_string(),
            });
        }

        let body: serde_json::Value = response.json().await.map_err(|e| LLMError::ParseError {
            message: e.to_string(),
        })?;

        let discovered: Vec<ModelInfo> = body["data"]
            .as_array()
            .unwrap_or(&Vec::new())
            .iter()
            .filter_map(|m| {
                let id = m["id"].as_str()?;
                // llama-server model IDs are often the filename — extract useful info
                let name = id.to_string();
                let lower = name.to_lowercase();

                // Detect capabilities from model name
                let has_thinking = lower.contains("qwen3") || lower.contains("deepseek");
                let ctx = if lower.contains("qwen3.5") || lower.contains("qwen35") {
                    262_144
                } else {
                    32_768
                };

                let mut info = ModelInfo::new(&name, &name)
                    .with_context_window(ctx)
                    .with_max_output(8_192);
                if has_thinking {
                    info = info.with_thinking();
                }
                Some(info)
            })
            .collect();

        self.apply_discovered(discovered);
        tracing::info!("Discovered {} models from llama-server", self.models.len());
        Ok(())
    }

    /// Apply discovered models to the provider state
    fn apply_discovered(&mut self, discovered: Vec<ModelInfo>) {
        if !discovered.is_empty() {
            let has_default = discovered.iter().any(|m| m.id == self.default_model);
            if !has_default {
                self.default_model = discovered[0].id.clone();
            }
            self.models = discovered;
        }
    }

    /// Check if a local LLM server is reachable (tries Ollama then llama-server)
    pub async fn is_available(base_url: Option<&str>) -> bool {
        let url = base_url.unwrap_or(DEFAULT_OLLAMA_URL);
        let client = Client::builder()
            .timeout(std::time::Duration::from_secs(2))
            .build()
            .unwrap_or_default();

        // Try Ollama /api/tags
        if client
            .get(format!("{}/api/tags", url))
            .send()
            .await
            .map(|r| r.status().is_success())
            .unwrap_or(false)
        {
            return true;
        }

        // Try llama-server /v1/models
        client
            .get(format!("{}/v1/models", url))
            .send()
            .await
            .map(|r| r.status().is_success())
            .unwrap_or(false)
    }

    /// Get the detected server kind
    pub fn server_kind(&self) -> LocalServerKind {
        self.server_kind
    }

    fn api_url(&self) -> String {
        format!("{}/v1/chat/completions", self.base_url)
    }

    fn build_request_body(&self, request: &LLMRequest) -> serde_json::Value {
        let mut messages: Vec<serde_json::Value> = Vec::new();

        // System message
        if let Some(system) = &request.system {
            messages.push(json!({
                "role": "system",
                "content": system
            }));
        }

        // Convert messages
        for m in &request.messages {
            let role = match m.role {
                super::types::Role::User => "user",
                super::types::Role::Assistant => "assistant",
            };

            let has_tool_results = m
                .content
                .iter()
                .any(|c| matches!(c, ContentBlock::ToolResult(_)));

            if has_tool_results {
                // Tool results as separate messages (OpenAI format)
                for block in &m.content {
                    if let ContentBlock::ToolResult(result) = block {
                        messages.push(json!({
                            "role": "tool",
                            "tool_call_id": result.tool_use_id,
                            "content": match &result.content {
                                super::types::ToolResultContent::Text(t) => t.clone(),
                                super::types::ToolResultContent::Json(j) => j.to_string(),
                                super::types::ToolResultContent::Image(_) => "[image]".to_string(),
                            }
                        }));
                    }
                }
            } else {
                let content = self.convert_content_blocks(&m.content);

                // Check for tool calls in assistant messages
                let tool_calls: Vec<serde_json::Value> = m
                    .content
                    .iter()
                    .filter_map(|c| {
                        if let ContentBlock::ToolUse(tool_use) = c {
                            Some(json!({
                                "id": tool_use.id,
                                "type": "function",
                                "function": {
                                    "name": tool_use.name,
                                    "arguments": tool_use.input.to_string()
                                }
                            }))
                        } else {
                            None
                        }
                    })
                    .collect();

                let has_content = content != json!(null) && content != json!([]);
                let has_tool_calls = !tool_calls.is_empty();

                if has_content || has_tool_calls {
                    let mut msg = json!({ "role": role });

                    if has_content {
                        msg["content"] = content;
                    } else {
                        msg["content"] = json!(null);
                    }

                    if has_tool_calls {
                        msg["tool_calls"] = json!(tool_calls);
                    }

                    messages.push(msg);
                }
            }
        }

        let model = if request.model.is_empty() {
            &self.default_model
        } else {
            &request.model
        };

        let mut body = json!({
            "model": model,
            "messages": messages,
            "stream": request.stream,
        });

        if let Some(max_tokens) = request.max_tokens {
            body["max_completion_tokens"] = json!(max_tokens);
        }

        if let Some(temp) = request.temperature {
            body["temperature"] = json!(temp);
        }

        // Convert tools to OpenAI format
        if !request.tools.is_empty() {
            let tools: Vec<serde_json::Value> = request
                .tools
                .iter()
                .map(|t| {
                    json!({
                        "type": "function",
                        "function": {
                            "name": t.name,
                            "description": t.description,
                            "parameters": t.input_schema
                        }
                    })
                })
                .collect();
            body["tools"] = json!(tools);
        }

        body
    }

    fn convert_content_blocks(&self, content: &[ContentBlock]) -> serde_json::Value {
        let parts: Vec<serde_json::Value> = content
            .iter()
            .filter_map(|c| match c {
                ContentBlock::Text(text) => Some(json!({
                    "type": "text",
                    "text": text
                })),
                // Skip images - local models generally don't support vision well
                // Skip tool use/results - handled separately
                _ => None,
            })
            .collect();

        if parts.len() == 1 {
            if let Some(text) = parts[0].get("text") {
                return text.clone();
            }
        }

        json!(parts)
    }
}

#[async_trait]
impl Provider for OllamaProvider {
    fn name(&self) -> &str {
        "ollama"
    }

    fn models(&self) -> &[ModelInfo] {
        &self.models
    }

    fn default_model(&self) -> &str {
        &self.default_model
    }

    fn supports(&self, capability: Capability) -> bool {
        matches!(
            capability,
            Capability::Streaming | Capability::ToolUse | Capability::Thinking
        )
    }

    async fn complete(&self, request: LLMRequest) -> Result<LLMResponse, LLMError> {
        let mut req = request;
        req.stream = false;

        let body = self.build_request_body(&req);

        let response = self
            .http_client
            .post(self.api_url())
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await
            .map_err(|e| LLMError::NetworkError {
                message: format!("Ollama not reachable: {}", e),
            })?;

        let status = response.status();
        if !status.is_success() {
            let text = response.text().await.unwrap_or_default();
            return Err(parse_error_response(status.as_u16(), &text));
        }

        let resp: OllamaResponse = response.json().await.map_err(|e| LLMError::ParseError {
            message: e.to_string(),
        })?;

        Ok(convert_response(resp))
    }

    async fn stream(&self, request: LLMRequest) -> Result<StreamBox, LLMError> {
        let mut req = request;
        req.stream = true;

        let body = self.build_request_body(&req);

        let response = self
            .http_client
            .post(self.api_url())
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await
            .map_err(|e| LLMError::NetworkError {
                message: format!("Ollama not reachable: {}", e),
            })?;

        let status = response.status();
        if !status.is_success() {
            let text = response.text().await.unwrap_or_default();
            return Err(parse_error_response(status.as_u16(), &text));
        }

        let (tx, rx) = mpsc::channel::<Result<StreamChunk, LLMError>>(32);
        let byte_stream = response.bytes_stream();

        tokio::spawn(async move {
            parse_sse_stream(byte_stream, tx).await;
        });

        let stream: StreamBox = Box::pin(ReceiverStream::new(rx));
        Ok(stream)
    }

    async fn test_key(&self) -> Result<(), LLMError> {
        // No API key for local servers - just check reachability
        let url = match self.server_kind {
            LocalServerKind::Ollama => format!("{}/api/tags", self.base_url),
            LocalServerKind::LlamaServer => format!("{}/health", self.base_url),
        };
        let response = self
            .http_client
            .get(&url)
            .timeout(std::time::Duration::from_secs(5))
            .send()
            .await
            .map_err(|e| LLMError::NetworkError {
                message: format!("Local server not reachable at {}: {}", self.base_url, e),
            })?;

        if response.status().is_success() {
            Ok(())
        } else {
            Err(LLMError::ProviderError {
                status: response.status().as_u16(),
                message: format!("Local server ({:?}) returned error", self.server_kind),
            })
        }
    }
}

// --- SSE Streaming Parser ---
// Ollama's /v1/ endpoint uses the same SSE format as OpenAI,
// but adds a `reasoning` field in deltas for thinking models (Qwen3)

async fn parse_sse_stream(
    mut byte_stream: impl Stream<Item = Result<bytes::Bytes, reqwest::Error>> + Unpin,
    tx: mpsc::Sender<Result<StreamChunk, LLMError>>,
) {
    let mut buffer = String::new();
    let mut message_id = String::new();
    let mut current_tool_id = String::new();
    let mut current_tool_name = String::new();
    let mut block_index: usize = 0;
    let mut in_tool_block = false;
    let mut in_thinking_block = false;

    while let Some(chunk_result) = byte_stream.next().await {
        let chunk = match chunk_result {
            Ok(c) => c,
            Err(e) => {
                let _ = tx
                    .send(Err(LLMError::NetworkError {
                        message: e.to_string(),
                    }))
                    .await;
                break;
            }
        };

        buffer.push_str(&String::from_utf8_lossy(&chunk));

        while let Some(pos) = buffer.find('\n') {
            let line = buffer[..pos].trim().to_string();
            buffer = buffer[pos + 1..].to_string();

            if line.is_empty() {
                continue;
            }

            if let Some(data) = line.strip_prefix("data: ") {
                if data == "[DONE]" {
                    continue;
                }

                if let Some(chunks) = parse_sse_data(
                    data,
                    &mut message_id,
                    &mut current_tool_id,
                    &mut current_tool_name,
                    &mut block_index,
                    &mut in_tool_block,
                    &mut in_thinking_block,
                ) {
                    for chunk in chunks {
                        if tx.send(Ok(chunk)).await.is_err() {
                            return;
                        }
                    }
                }
            }
        }
    }
}

fn parse_sse_data(
    data: &str,
    message_id: &mut String,
    current_tool_id: &mut String,
    current_tool_name: &mut String,
    block_index: &mut usize,
    in_tool_block: &mut bool,
    in_thinking_block: &mut bool,
) -> Option<Vec<StreamChunk>> {
    let json: serde_json::Value = serde_json::from_str(data).ok()?;
    let mut chunks = Vec::new();

    // Extract message ID from first chunk
    if message_id.is_empty() {
        if let Some(id) = json["id"].as_str() {
            *message_id = id.to_string();
            chunks.push(StreamChunk::Start {
                message_id: message_id.clone(),
            });
        }
    }

    // Process choices
    if let Some(choices) = json["choices"].as_array() {
        for choice in choices {
            let delta = &choice["delta"];

            // Handle reasoning/thinking content (Qwen3 via Ollama uses "reasoning",
            // llama-server uses "reasoning_content")
            let reasoning_value = delta["reasoning"].as_str()
                .or_else(|| delta["reasoning_content"].as_str());
            if let Some(reasoning) = reasoning_value {
                if !reasoning.is_empty() {
                    if !*in_thinking_block {
                        // Start thinking block
                        *in_thinking_block = true;
                        chunks.push(StreamChunk::BlockStart {
                            index: *block_index,
                            block_type: BlockType::Thinking,
                            tool_id: None,
                            tool_name: None,
                        });
                        *block_index += 1;
                    }
                    chunks.push(StreamChunk::Delta(StreamDelta::Thinking(
                        reasoning.to_string(),
                    )));
                }
            }

            // Handle tool calls
            if let Some(tool_calls) = delta["tool_calls"].as_array() {
                for tool_call in tool_calls {
                    let tc_index = tool_call["index"].as_u64().unwrap_or(0) as usize;

                    if let Some(id) = tool_call["id"].as_str() {
                        // Close previous block
                        if *block_index > 0 {
                            chunks.push(StreamChunk::BlockStop {
                                index: *block_index - 1,
                            });
                        }
                        *in_thinking_block = false;

                        *current_tool_id = id.to_string();
                        *in_tool_block = true;

                        if let Some(func) = tool_call["function"].as_object() {
                            if let Some(name) = func.get("name").and_then(|n| n.as_str()) {
                                *current_tool_name = name.to_string();
                            }
                        }

                        chunks.push(StreamChunk::BlockStart {
                            index: tc_index + 1,
                            block_type: BlockType::ToolUse,
                            tool_id: Some(current_tool_id.clone()),
                            tool_name: if current_tool_name.is_empty() {
                                None
                            } else {
                                Some(current_tool_name.clone())
                            },
                        });
                        *block_index = tc_index + 2;
                    }

                    if let Some(func) = tool_call["function"].as_object() {
                        if let Some(args) = func.get("arguments").and_then(|a| a.as_str()) {
                            if !args.is_empty() {
                                chunks.push(StreamChunk::Delta(StreamDelta::ToolInput {
                                    block_index: tc_index + 1,
                                    input_json: args.to_string(),
                                }));
                            }
                        }
                    }
                }
            }

            // Handle text content
            if let Some(content) = delta["content"].as_str() {
                if !content.is_empty() {
                    // Transition from thinking to text
                    if *in_thinking_block {
                        *in_thinking_block = false;
                        chunks.push(StreamChunk::BlockStop {
                            index: *block_index - 1,
                        });
                        chunks.push(StreamChunk::BlockStart {
                            index: *block_index,
                            block_type: BlockType::Text,
                            tool_id: None,
                            tool_name: None,
                        });
                        *block_index += 1;
                    } else if *in_tool_block {
                        *in_tool_block = false;
                        chunks.push(StreamChunk::BlockStop {
                            index: *block_index - 1,
                        });
                        chunks.push(StreamChunk::BlockStart {
                            index: *block_index,
                            block_type: BlockType::Text,
                            tool_id: None,
                            tool_name: None,
                        });
                        *block_index += 1;
                    } else if *block_index == 0 {
                        // First content chunk with no prior thinking — start text block
                        chunks.push(StreamChunk::BlockStart {
                            index: 0,
                            block_type: BlockType::Text,
                            tool_id: None,
                            tool_name: None,
                        });
                        *block_index = 1;
                    }
                    chunks.push(StreamChunk::Delta(StreamDelta::Text(content.to_string())));
                }
            }

            // Check for finish reason
            if let Some(finish_reason) = choice["finish_reason"].as_str() {
                let reason = match finish_reason {
                    "stop" => StopReason::EndTurn,
                    "length" => StopReason::MaxTokens,
                    "tool_calls" => StopReason::ToolUse,
                    _ => StopReason::EndTurn,
                };

                // Close any open block
                if *block_index > 0 {
                    chunks.push(StreamChunk::BlockStop {
                        index: *block_index - 1,
                    });
                }

                let usage = json["usage"].as_object().map(|u| Usage {
                    input_tokens: u
                        .get("prompt_tokens")
                        .and_then(|v| v.as_u64())
                        .unwrap_or(0) as u32,
                    output_tokens: u
                        .get("completion_tokens")
                        .and_then(|v| v.as_u64())
                        .unwrap_or(0) as u32,
                    thinking_tokens: None,
                });

                chunks.push(StreamChunk::Stop { reason, usage });
            }
        }
    }

    if chunks.is_empty() {
        None
    } else {
        Some(chunks)
    }
}

fn parse_error_response(status: u16, body: &str) -> LLMError {
    if let Ok(json) = serde_json::from_str::<serde_json::Value>(body) {
        let message = json["error"]["message"]
            .as_str()
            .or_else(|| json["error"].as_str())
            .unwrap_or("Unknown Ollama error")
            .to_string();

        match status {
            404 => LLMError::ModelNotFound {
                model: message.clone(),
            },
            _ => LLMError::ProviderError { status, message },
        }
    } else {
        LLMError::ProviderError {
            status,
            message: body.to_string(),
        }
    }
}

fn convert_response(resp: OllamaResponse) -> LLMResponse {
    let choice = resp.choices.into_iter().next().unwrap_or_default();
    let mut content: Vec<ContentBlock> = Vec::new();

    // Add thinking/reasoning content (Ollama: "reasoning", llama-server: "reasoning_content")
    let reasoning = choice.message.reasoning.or(choice.message.reasoning_content);
    if let Some(reasoning) = reasoning {
        if !reasoning.is_empty() {
            content.push(ContentBlock::Thinking(reasoning));
        }
    }

    // Add text content
    if let Some(text) = choice.message.content {
        if !text.is_empty() {
            content.push(ContentBlock::Text(text));
        }
    }

    // Add tool calls
    if let Some(tool_calls) = choice.message.tool_calls {
        for tc in tool_calls {
            let input: serde_json::Value =
                serde_json::from_str(&tc.function.arguments).unwrap_or(serde_json::Value::Null);
            content.push(ContentBlock::ToolUse(ToolUse {
                id: tc.id,
                name: tc.function.name,
                input,
            }));
        }
    }

    let stop_reason = match choice.finish_reason.as_deref() {
        Some("stop") => StopReason::EndTurn,
        Some("length") => StopReason::MaxTokens,
        Some("tool_calls") => StopReason::ToolUse,
        _ => StopReason::EndTurn,
    };

    LLMResponse {
        id: resp.id,
        model: resp.model,
        content,
        stop_reason,
        usage: Usage {
            input_tokens: resp.usage.prompt_tokens,
            output_tokens: resp.usage.completion_tokens,
            thinking_tokens: None,
        },
    }
}

fn parse_param_size(s: &str) -> f64 {
    // Parse strings like "8.2B", "4.0B", "137M"
    let s = s.trim().to_uppercase();
    if let Some(num) = s.strip_suffix('B') {
        num.parse::<f64>().unwrap_or(0.0)
    } else if let Some(num) = s.strip_suffix('M') {
        num.parse::<f64>().unwrap_or(0.0) / 1000.0
    } else {
        0.0
    }
}

// --- Response types ---

#[derive(Debug, Deserialize)]
struct OllamaTagsResponse {
    models: Vec<OllamaModelTag>,
}

#[derive(Debug, Deserialize)]
struct OllamaModelTag {
    name: String,
    #[allow(dead_code)]
    model: String,
    details: OllamaModelDetails,
}

#[derive(Debug, Deserialize)]
struct OllamaModelDetails {
    family: String,
    parameter_size: String,
}

#[derive(Debug, Deserialize)]
struct OllamaResponse {
    id: String,
    model: String,
    choices: Vec<OllamaChoice>,
    usage: OllamaUsage,
}

#[derive(Debug, Deserialize, Default)]
struct OllamaChoice {
    message: OllamaMessage,
    finish_reason: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
struct OllamaMessage {
    content: Option<String>,
    /// Ollama uses "reasoning", llama-server uses "reasoning_content"
    reasoning: Option<String>,
    reasoning_content: Option<String>,
    tool_calls: Option<Vec<OllamaToolCall>>,
}

#[derive(Debug, Deserialize)]
struct OllamaToolCall {
    id: String,
    function: OllamaFunction,
}

#[derive(Debug, Deserialize)]
struct OllamaFunction {
    name: String,
    arguments: String,
}

#[derive(Debug, Deserialize, Default)]
struct OllamaUsage {
    prompt_tokens: u32,
    completion_tokens: u32,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_param_size() {
        assert!((parse_param_size("8.2B") - 8.2).abs() < 0.01);
        assert!((parse_param_size("4.0B") - 4.0).abs() < 0.01);
        assert!((parse_param_size("137M") - 0.137).abs() < 0.001);
    }

    #[test]
    fn test_parse_sse_text_delta() {
        let data =
            r#"{"id":"chatcmpl-1","choices":[{"delta":{"content":"Hello"},"index":0}]}"#;
        let mut message_id = String::new();
        let mut tool_id = String::new();
        let mut tool_name = String::new();
        let mut block_index = 0;
        let mut in_tool = false;
        let mut in_thinking = false;

        let chunks = parse_sse_data(
            data,
            &mut message_id,
            &mut tool_id,
            &mut tool_name,
            &mut block_index,
            &mut in_tool,
            &mut in_thinking,
        )
        .unwrap();

        assert!(chunks.len() >= 2);
        assert!(matches!(chunks[0], StreamChunk::Start { .. }));
    }

    #[test]
    fn test_parse_sse_reasoning_then_text() {
        // First chunk: reasoning
        let data1 = r#"{"id":"chatcmpl-1","choices":[{"delta":{"reasoning":"Let me think","content":""},"index":0}]}"#;
        let mut message_id = String::new();
        let mut tool_id = String::new();
        let mut tool_name = String::new();
        let mut block_index = 0;
        let mut in_tool = false;
        let mut in_thinking = false;

        let chunks1 = parse_sse_data(
            data1,
            &mut message_id,
            &mut tool_id,
            &mut tool_name,
            &mut block_index,
            &mut in_tool,
            &mut in_thinking,
        )
        .unwrap();

        // Should have Start + BlockStart(Thinking) + Delta(Thinking)
        assert!(chunks1
            .iter()
            .any(|c| matches!(c, StreamChunk::BlockStart { block_type: BlockType::Thinking, .. })));
        assert!(in_thinking);

        // Second chunk: actual content (thinking done)
        let data2 = r#"{"id":"chatcmpl-1","choices":[{"delta":{"content":"Hi!"},"index":0}]}"#;

        let chunks2 = parse_sse_data(
            data2,
            &mut message_id,
            &mut tool_id,
            &mut tool_name,
            &mut block_index,
            &mut in_tool,
            &mut in_thinking,
        )
        .unwrap();

        // Should have BlockStop(thinking) + BlockStart(Text) + Delta(Text)
        assert!(chunks2.iter().any(|c| matches!(c, StreamChunk::BlockStop { .. })));
        assert!(chunks2
            .iter()
            .any(|c| matches!(c, StreamChunk::BlockStart { block_type: BlockType::Text, .. })));
        assert!(!in_thinking);
    }

    #[test]
    fn test_parse_error_response() {
        let body = r#"{"error":"model 'nonexistent' not found"}"#;
        let err = parse_error_response(404, body);
        assert!(matches!(err, LLMError::ModelNotFound { .. }));

        let err2 = parse_error_response(500, "internal server error");
        assert!(matches!(err2, LLMError::ProviderError { .. }));
    }

    #[test]
    fn test_convert_response_with_reasoning() {
        let resp = OllamaResponse {
            id: "chatcmpl-1".to_string(),
            model: "qwen3:8b".to_string(),
            choices: vec![OllamaChoice {
                message: OllamaMessage {
                    content: Some("Hi there!".to_string()),
                    reasoning: Some("The user said hi, I should respond.".to_string()),
                    tool_calls: None,
                },
                finish_reason: Some("stop".to_string()),
            }],
            usage: OllamaUsage {
                prompt_tokens: 10,
                completion_tokens: 20,
            },
        };

        let response = convert_response(resp);
        assert_eq!(response.content.len(), 2);
        assert!(matches!(&response.content[0], ContentBlock::Thinking(t) if t.contains("user said hi")));
        assert!(matches!(&response.content[1], ContentBlock::Text(t) if t == "Hi there!"));
    }
}
