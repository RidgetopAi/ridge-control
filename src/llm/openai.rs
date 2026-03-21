// TRC-006: OpenAI provider - supports both Chat Completions and Responses API
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

const OPENAI_CHAT_URL: &str = "https://api.openai.com/v1/chat/completions";
const OPENAI_RESPONSES_URL: &str = "https://api.openai.com/v1/responses";

/// OpenAI GPT provider
pub struct OpenAIProvider {
    api_key: String,
    http_client: Client,
    models: Vec<ModelInfo>,
    default_model: String,
}

/// Check if a model requires the Responses API (not supported on Chat Completions)
fn requires_responses_api(model: &str) -> bool {
    // Pro models are Responses API only (return 404 on /v1/chat/completions)
    model.contains("-pro")
}

impl OpenAIProvider {
    pub fn new(api_key: impl Into<String>) -> Self {
        let api_key = api_key.into();
        let http_client = Client::new();

        let models = vec![
            // GPT-5 series (latest)
            ModelInfo::new("gpt-5.4-2026-03-05", "GPT-5.4")
                .with_context_window(256_000)
                .with_max_output(32_768),
            ModelInfo::new("gpt-5.4-pro-2026-03-05", "GPT-5.4 Pro")
                .with_thinking()
                .with_context_window(256_000)
                .with_max_output(32_768),
            ModelInfo::new("gpt-5.2-2025-12-11", "GPT-5.2")
                .with_context_window(256_000)
                .with_max_output(32_768),
            ModelInfo::new("gpt-5.2-pro-2025-12-11", "GPT-5.2 Pro")
                .with_thinking()
                .with_context_window(256_000)
                .with_max_output(32_768),
            ModelInfo::new("gpt-5-mini-2025-08-07", "GPT-5 Mini")
                .with_context_window(128_000)
                .with_max_output(16_384),
            // GPT-4 series
            ModelInfo::new("gpt-4o", "GPT-4o")
                .with_context_window(128_000)
                .with_max_output(16_384),
            ModelInfo::new("gpt-4o-mini", "GPT-4o Mini")
                .with_context_window(128_000)
                .with_max_output(16_384),
            ModelInfo::new("gpt-4-turbo", "GPT-4 Turbo")
                .with_context_window(128_000)
                .with_max_output(4_096),
            // o-series (reasoning)
            ModelInfo::new("o1", "o1 Reasoning")
                .with_thinking()
                .with_context_window(200_000)
                .with_max_output(100_000),
            ModelInfo::new("o1-mini", "o1 Mini")
                .with_thinking()
                .with_context_window(128_000)
                .with_max_output(65_536),
            ModelInfo::new("o3-mini", "o3 Mini")
                .with_thinking()
                .with_context_window(200_000)
                .with_max_output(100_000),
        ];

        Self {
            api_key,
            http_client,
            models,
            default_model: "gpt-5.2-2025-12-11".to_string(),
        }
    }

    fn get_model<'a>(&'a self, request: &'a LLMRequest) -> &'a str {
        if request.model.is_empty() {
            &self.default_model
        } else {
            &request.model
        }
    }

    // ========================================================================
    // Chat Completions API (/v1/chat/completions)
    // ========================================================================

    fn build_chat_request_body(&self, request: &LLMRequest) -> serde_json::Value {
        let mut messages: Vec<serde_json::Value> = Vec::new();

        // OpenAI uses a system message as the first message
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

            let content = self.convert_content_blocks(&m.content);

            // Check if this is a tool result message
            let has_tool_results = m.content.iter().any(|c| matches!(c, ContentBlock::ToolResult(_)));

            if has_tool_results {
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
                let tool_calls: Vec<serde_json::Value> = m.content.iter()
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

        let model = self.get_model(request);

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

        if request.stream {
            body["stream_options"] = json!({ "include_usage": true });
        }

        body
    }

    // ========================================================================
    // Responses API (/v1/responses)
    // ========================================================================

    fn build_responses_request_body(&self, request: &LLMRequest) -> serde_json::Value {
        let mut input: Vec<serde_json::Value> = Vec::new();

        // Convert messages to Responses API input format
        for m in &request.messages {
            let has_tool_results = m.content.iter().any(|c| matches!(c, ContentBlock::ToolResult(_)));

            if has_tool_results {
                // Tool results → function_call_output items
                for block in &m.content {
                    if let ContentBlock::ToolResult(result) = block {
                        let output_text = match &result.content {
                            super::types::ToolResultContent::Text(t) => t.clone(),
                            super::types::ToolResultContent::Json(j) => j.to_string(),
                            super::types::ToolResultContent::Image(_) => "[image]".to_string(),
                        };
                        input.push(json!({
                            "type": "function_call_output",
                            "call_id": result.tool_use_id,
                            "output": output_text
                        }));
                    }
                }
            } else {
                match m.role {
                    super::types::Role::User => {
                        // User messages
                        let content = self.convert_responses_content(&m.content);
                        let is_empty = content.is_null() || content.as_array().map_or(false, |a| a.is_empty());
                        if !is_empty {
                            input.push(json!({
                                "role": "user",
                                "content": content
                            }));
                        }
                    }
                    super::types::Role::Assistant => {
                        // Assistant messages: text → message items, tool_use → function_call items
                        let text_parts: Vec<&str> = m.content.iter()
                            .filter_map(|c| if let ContentBlock::Text(t) = c { Some(t.as_str()) } else { None })
                            .collect();

                        if !text_parts.is_empty() {
                            let content_items: Vec<serde_json::Value> = text_parts.iter()
                                .map(|t| json!({"type": "output_text", "text": t}))
                                .collect();
                            input.push(json!({
                                "type": "message",
                                "role": "assistant",
                                "content": content_items
                            }));
                        }

                        for block in &m.content {
                            if let ContentBlock::ToolUse(tool_use) = block {
                                input.push(json!({
                                    "type": "function_call",
                                    "call_id": tool_use.id,
                                    "name": tool_use.name,
                                    "arguments": tool_use.input.to_string()
                                }));
                            }
                        }
                    }
                }
            }
        }

        let model = self.get_model(request);

        let mut body = json!({
            "model": model,
            "input": input,
            "stream": request.stream,
        });

        // System prompt → instructions
        if let Some(system) = &request.system {
            body["instructions"] = json!(system);
        }

        if let Some(max_tokens) = request.max_tokens {
            body["max_output_tokens"] = json!(max_tokens);
        }

        if let Some(temp) = request.temperature {
            body["temperature"] = json!(temp);
        }

        // Tools use flatter format in Responses API
        if !request.tools.is_empty() {
            let tools: Vec<serde_json::Value> = request
                .tools
                .iter()
                .map(|t| {
                    json!({
                        "type": "function",
                        "name": t.name,
                        "description": t.description,
                        "parameters": t.input_schema
                    })
                })
                .collect();
            body["tools"] = json!(tools);
        }

        body
    }

    /// Convert content blocks to Responses API input content format
    fn convert_responses_content(&self, content: &[ContentBlock]) -> serde_json::Value {
        let parts: Vec<serde_json::Value> = content
            .iter()
            .filter_map(|c| match c {
                ContentBlock::Text(text) => Some(json!({
                    "type": "input_text",
                    "text": text
                })),
                ContentBlock::Image(img) => {
                    let url = match &img.source {
                        super::types::ImageSource::Base64(b64) => {
                            format!("data:{};base64,{}", img.media_type, b64)
                        }
                        super::types::ImageSource::Url(url) => url.clone(),
                    };
                    Some(json!({
                        "type": "input_image",
                        "image_url": url
                    }))
                }
                _ => None,
            })
            .collect();

        // Simplify single text to string
        if parts.len() == 1 {
            if let Some(text) = parts[0].get("text") {
                return text.clone();
            }
        }

        json!(parts)
    }

    // ========================================================================
    // Chat Completions content helpers
    // ========================================================================

    fn convert_content_blocks(&self, content: &[ContentBlock]) -> serde_json::Value {
        let parts: Vec<serde_json::Value> = content
            .iter()
            .filter_map(|c| match c {
                ContentBlock::Text(text) => Some(json!({
                    "type": "text",
                    "text": text
                })),
                ContentBlock::Image(img) => {
                    let url = match &img.source {
                        super::types::ImageSource::Base64(b64) => {
                            format!("data:{};base64,{}", img.media_type, b64)
                        }
                        super::types::ImageSource::Url(url) => url.clone(),
                    };
                    Some(json!({
                        "type": "image_url",
                        "image_url": { "url": url }
                    }))
                }
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
impl Provider for OpenAIProvider {
    fn name(&self) -> &str {
        "openai"
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
            Capability::Streaming | Capability::ToolUse | Capability::Vision | Capability::Reasoning
        )
    }

    async fn complete(&self, request: LLMRequest) -> Result<LLMResponse, LLMError> {
        let mut req = request;
        req.stream = false;

        let model = self.get_model(&req).to_string();
        let use_responses = requires_responses_api(&model);

        let (url, body) = if use_responses {
            (OPENAI_RESPONSES_URL, self.build_responses_request_body(&req))
        } else {
            (OPENAI_CHAT_URL, self.build_chat_request_body(&req))
        };

        let response = self
            .http_client
            .post(url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await
            .map_err(|e| LLMError::NetworkError {
                message: e.to_string(),
            })?;

        let status = response.status();
        if !status.is_success() {
            let text = response.text().await.unwrap_or_default();
            return Err(parse_error_response(status.as_u16(), &text));
        }

        if use_responses {
            let resp: ResponsesApiResponse = response.json().await.map_err(|e| LLMError::ParseError {
                message: e.to_string(),
            })?;
            Ok(convert_responses_response(resp))
        } else {
            let resp: ChatResponse = response.json().await.map_err(|e| LLMError::ParseError {
                message: e.to_string(),
            })?;
            Ok(convert_chat_response(resp))
        }
    }

    async fn stream(&self, request: LLMRequest) -> Result<StreamBox, LLMError> {
        let mut req = request;
        req.stream = true;

        let model = self.get_model(&req).to_string();
        let use_responses = requires_responses_api(&model);

        let (url, body) = if use_responses {
            (OPENAI_RESPONSES_URL, self.build_responses_request_body(&req))
        } else {
            (OPENAI_CHAT_URL, self.build_chat_request_body(&req))
        };

        let response = self
            .http_client
            .post(url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await
            .map_err(|e| LLMError::NetworkError {
                message: e.to_string(),
            })?;

        let status = response.status();
        if !status.is_success() {
            let text = response.text().await.unwrap_or_default();
            return Err(parse_error_response(status.as_u16(), &text));
        }

        let (tx, rx) = mpsc::channel::<Result<StreamChunk, LLMError>>(32);
        let byte_stream = response.bytes_stream();

        if use_responses {
            tokio::spawn(async move {
                parse_responses_sse_stream(byte_stream, tx).await;
            });
        } else {
            tokio::spawn(async move {
                parse_chat_sse_stream(byte_stream, tx).await;
            });
        }

        let stream: StreamBox = Box::pin(ReceiverStream::new(rx));
        Ok(stream)
    }

    async fn test_key(&self) -> Result<(), LLMError> {
        let body = json!({
            "model": "gpt-4o-mini",
            "messages": [{"role": "user", "content": "Hi"}],
            "max_completion_tokens": 1,
        });

        let response = self
            .http_client
            .post(OPENAI_CHAT_URL)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await
            .map_err(|e| LLMError::NetworkError {
                message: e.to_string(),
            })?;

        let status = response.status();
        if !status.is_success() {
            let text = response.text().await.unwrap_or_default();
            return Err(parse_error_response(status.as_u16(), &text));
        }

        Ok(())
    }
}

// ============================================================================
// Chat Completions SSE Parser
// ============================================================================

async fn parse_chat_sse_stream(
    mut byte_stream: impl Stream<Item = Result<bytes::Bytes, reqwest::Error>> + Unpin,
    tx: mpsc::Sender<Result<StreamChunk, LLMError>>,
) {
    let mut buffer = String::new();
    let mut message_id = String::new();
    let mut current_tool_id = String::new();
    let mut current_tool_name = String::new();
    let mut block_index: usize = 0;
    let mut in_tool_block = false;

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

                if let Some(chunks) = parse_chat_sse_data(
                    data,
                    &mut message_id,
                    &mut current_tool_id,
                    &mut current_tool_name,
                    &mut block_index,
                    &mut in_tool_block,
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

fn parse_chat_sse_data(
    data: &str,
    message_id: &mut String,
    current_tool_id: &mut String,
    current_tool_name: &mut String,
    block_index: &mut usize,
    in_tool_block: &mut bool,
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
            chunks.push(StreamChunk::BlockStart {
                index: 0,
                block_type: BlockType::Text,
                tool_id: None,
                tool_name: None,
            });
            // Fix: increment block_index so BlockStop fires correctly
            *block_index = 1;
        }
    }

    // Process choices
    if let Some(choices) = json["choices"].as_array() {
        for choice in choices {
            let delta = &choice["delta"];

            // Check for tool calls
            if let Some(tool_calls) = delta["tool_calls"].as_array() {
                for tool_call in tool_calls {
                    let tc_index = tool_call["index"].as_u64().unwrap_or(0) as usize;

                    // New tool call starting
                    if let Some(id) = tool_call["id"].as_str() {
                        if *block_index > 0 {
                            chunks.push(StreamChunk::BlockStop { index: *block_index - 1 });
                        }

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

                    // Tool call arguments delta
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

            // Process text content
            if let Some(content) = delta["content"].as_str() {
                if !content.is_empty() {
                    if *in_tool_block {
                        *in_tool_block = false;
                        chunks.push(StreamChunk::BlockStop { index: *block_index - 1 });
                        chunks.push(StreamChunk::BlockStart {
                            index: *block_index,
                            block_type: BlockType::Text,
                            tool_id: None,
                            tool_name: None,
                        });
                        *block_index += 1;
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
                    "content_filter" => StopReason::ContentFilter,
                    _ => StopReason::EndTurn,
                };

                if *block_index > 0 {
                    chunks.push(StreamChunk::BlockStop { index: *block_index - 1 });
                }

                let usage = json["usage"].as_object().map(|u| Usage {
                    input_tokens: u.get("prompt_tokens").and_then(|v| v.as_u64()).unwrap_or(0) as u32,
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

// ============================================================================
// Responses API SSE Parser
// ============================================================================

async fn parse_responses_sse_stream(
    mut byte_stream: impl Stream<Item = Result<bytes::Bytes, reqwest::Error>> + Unpin,
    tx: mpsc::Sender<Result<StreamChunk, LLMError>>,
) {
    let mut buffer = String::new();
    let mut started = false;
    let mut block_index: usize = 0;
    let mut in_text_block = false;
    // Track tool call_ids by output_index for BlockStart/BlockStop
    let mut tool_block_indices: std::collections::HashMap<usize, usize> = std::collections::HashMap::new();

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

            // Responses API SSE has "event: <type>" and "data: <json>" lines
            // We only need the data lines - the type is embedded in the JSON
            if let Some(data) = line.strip_prefix("data: ") {
                if let Some(chunks) = parse_responses_sse_data(
                    data,
                    &mut started,
                    &mut block_index,
                    &mut in_text_block,
                    &mut tool_block_indices,
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

fn parse_responses_sse_data(
    data: &str,
    started: &mut bool,
    block_index: &mut usize,
    in_text_block: &mut bool,
    tool_block_indices: &mut std::collections::HashMap<usize, usize>,
) -> Option<Vec<StreamChunk>> {
    let json: serde_json::Value = serde_json::from_str(data).ok()?;
    let event_type = json["type"].as_str()?;
    let mut chunks = Vec::new();

    match event_type {
        "response.created" => {
            if !*started {
                *started = true;
                let response_id = json["response"]["id"].as_str().unwrap_or("").to_string();
                chunks.push(StreamChunk::Start {
                    message_id: response_id,
                });
            }
        }

        "response.output_item.added" => {
            let item_type = json["item"]["type"].as_str().unwrap_or("");
            let output_index = json["output_index"].as_u64().unwrap_or(0) as usize;

            match item_type {
                "function_call" => {
                    // Close any open text block
                    if *in_text_block {
                        chunks.push(StreamChunk::BlockStop { index: *block_index - 1 });
                        *in_text_block = false;
                    }

                    let call_id = json["item"]["call_id"].as_str().unwrap_or("").to_string();
                    let name = json["item"]["name"].as_str().unwrap_or("").to_string();

                    let bi = *block_index;
                    tool_block_indices.insert(output_index, bi);

                    chunks.push(StreamChunk::BlockStart {
                        index: bi,
                        block_type: BlockType::ToolUse,
                        tool_id: Some(call_id),
                        tool_name: if name.is_empty() { None } else { Some(name) },
                    });
                    *block_index += 1;
                }
                _ => {
                    // message type - text content will come via content_part.added
                }
            }
        }

        "response.content_part.added" => {
            let part_type = json["part"]["type"].as_str().unwrap_or("");
            if part_type == "output_text" {
                let bi = *block_index;
                chunks.push(StreamChunk::BlockStart {
                    index: bi,
                    block_type: BlockType::Text,
                    tool_id: None,
                    tool_name: None,
                });
                *block_index += 1;
                *in_text_block = true;
            }
        }

        "response.output_text.delta" => {
            if let Some(delta) = json["delta"].as_str() {
                if !delta.is_empty() {
                    chunks.push(StreamChunk::Delta(StreamDelta::Text(delta.to_string())));
                }
            }
        }

        "response.function_call_arguments.delta" => {
            let output_index = json["output_index"].as_u64().unwrap_or(0) as usize;
            if let Some(delta) = json["delta"].as_str() {
                if !delta.is_empty() {
                    let bi = tool_block_indices.get(&output_index).copied().unwrap_or(0);
                    chunks.push(StreamChunk::Delta(StreamDelta::ToolInput {
                        block_index: bi,
                        input_json: delta.to_string(),
                    }));
                }
            }
        }

        "response.content_part.done" => {
            if *in_text_block {
                chunks.push(StreamChunk::BlockStop { index: *block_index - 1 });
                *in_text_block = false;
            }
        }

        "response.output_item.done" => {
            let item_type = json["item"]["type"].as_str().unwrap_or("");
            let output_index = json["output_index"].as_u64().unwrap_or(0) as usize;

            if item_type == "function_call" {
                if let Some(&bi) = tool_block_indices.get(&output_index) {
                    chunks.push(StreamChunk::BlockStop { index: bi });
                }
            }
        }

        "response.completed" => {
            // Extract usage from the completed response
            let usage = json["response"]["usage"].as_object().map(|u| Usage {
                input_tokens: u.get("input_tokens").and_then(|v| v.as_u64()).unwrap_or(0) as u32,
                output_tokens: u.get("output_tokens").and_then(|v| v.as_u64()).unwrap_or(0) as u32,
                thinking_tokens: u.get("output_tokens_details")
                    .and_then(|d| d.get("reasoning_tokens"))
                    .and_then(|v| v.as_u64())
                    .filter(|&v| v > 0)
                    .map(|v| v as u32),
            });

            // Determine stop reason from response status and output
            let status = json["response"]["status"].as_str().unwrap_or("completed");
            let has_tool_calls = json["response"]["output"].as_array()
                .map(|arr| arr.iter().any(|item| item["type"].as_str() == Some("function_call")))
                .unwrap_or(false);

            let reason = if has_tool_calls {
                StopReason::ToolUse
            } else if status == "incomplete" {
                StopReason::MaxTokens
            } else {
                StopReason::EndTurn
            };

            chunks.push(StreamChunk::Stop { reason, usage });
        }

        "response.failed" => {
            let error_msg = json["response"]["error"]["message"]
                .as_str()
                .unwrap_or("Unknown error")
                .to_string();
            chunks.push(StreamChunk::Error(LLMError::ProviderError {
                status: 500,
                message: error_msg,
            }));
        }

        // Ignore other events (response.in_progress, *.done duplicates, etc.)
        _ => {}
    }

    if chunks.is_empty() {
        None
    } else {
        Some(chunks)
    }
}

// ============================================================================
// Error handling (shared)
// ============================================================================

fn parse_error_response(status: u16, body: &str) -> LLMError {
    if let Ok(json) = serde_json::from_str::<serde_json::Value>(body) {
        let message = json["error"]["message"]
            .as_str()
            .unwrap_or("Unknown error")
            .to_string();
        let error_type = json["error"]["type"].as_str().unwrap_or("");
        let error_code = json["error"]["code"].as_str().unwrap_or("");

        match (error_type, error_code) {
            ("invalid_api_key", _) | (_, "invalid_api_key") => LLMError::AuthError { message },
            ("rate_limit_error", _) | (_, "rate_limit_exceeded") => LLMError::RateLimit {
                retry_after_secs: 60,
            },
            ("invalid_request_error", _) => LLMError::InvalidRequest { message },
            ("model_not_found", _) | (_, "model_not_found") => LLMError::ModelNotFound {
                model: message.clone(),
            },
            ("content_filter", _) | (_, "content_policy_violation") => {
                LLMError::ContentFiltered { reason: message }
            }
            _ => LLMError::ProviderError { status, message },
        }
    } else {
        LLMError::ProviderError {
            status,
            message: body.to_string(),
        }
    }
}

// ============================================================================
// Chat Completions response types and conversion
// ============================================================================

#[derive(Debug, Deserialize)]
struct ChatResponse {
    id: String,
    model: String,
    choices: Vec<ChatChoice>,
    usage: ChatUsage,
}

#[derive(Debug, Deserialize, Default)]
struct ChatChoice {
    message: ChatMessage,
    finish_reason: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
struct ChatMessage {
    content: Option<String>,
    tool_calls: Option<Vec<ChatToolCall>>,
}

#[derive(Debug, Deserialize)]
struct ChatToolCall {
    id: String,
    #[serde(rename = "type")]
    call_type: String,
    function: ChatFunction,
}

#[derive(Debug, Deserialize)]
struct ChatFunction {
    name: String,
    arguments: String,
}

#[derive(Debug, Deserialize, Default)]
struct ChatUsage {
    prompt_tokens: u32,
    completion_tokens: u32,
}

fn convert_chat_response(resp: ChatResponse) -> LLMResponse {
    let choice = resp.choices.into_iter().next().unwrap_or_default();
    let mut content: Vec<ContentBlock> = Vec::new();

    if let Some(text) = choice.message.content {
        if !text.is_empty() {
            content.push(ContentBlock::Text(text));
        }
    }

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
        Some("content_filter") => StopReason::ContentFilter,
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

// ============================================================================
// Responses API response types and conversion
// ============================================================================

#[derive(Debug, Deserialize)]
struct ResponsesApiResponse {
    id: String,
    model: String,
    status: String,
    output: Vec<ResponsesOutputItem>,
    usage: Option<ResponsesUsage>,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
enum ResponsesOutputItem {
    #[serde(rename = "message")]
    Message {
        id: String,
        content: Vec<ResponsesContentPart>,
    },
    #[serde(rename = "function_call")]
    FunctionCall {
        id: String,
        call_id: String,
        name: String,
        arguments: String,
    },
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
enum ResponsesContentPart {
    #[serde(rename = "output_text")]
    OutputText { text: String },
}

#[derive(Debug, Deserialize)]
struct ResponsesUsage {
    input_tokens: u32,
    output_tokens: u32,
    output_tokens_details: Option<ResponsesOutputTokenDetails>,
}

#[derive(Debug, Deserialize)]
struct ResponsesOutputTokenDetails {
    reasoning_tokens: Option<u32>,
}

fn convert_responses_response(resp: ResponsesApiResponse) -> LLMResponse {
    let mut content: Vec<ContentBlock> = Vec::new();
    let mut has_tool_calls = false;

    for item in &resp.output {
        match item {
            ResponsesOutputItem::Message { content: parts, .. } => {
                for part in parts {
                    match part {
                        ResponsesContentPart::OutputText { text } => {
                            if !text.is_empty() {
                                content.push(ContentBlock::Text(text.clone()));
                            }
                        }
                    }
                }
            }
            ResponsesOutputItem::FunctionCall { call_id, name, arguments, .. } => {
                has_tool_calls = true;
                let input: serde_json::Value =
                    serde_json::from_str(arguments).unwrap_or(serde_json::Value::Null);
                content.push(ContentBlock::ToolUse(ToolUse {
                    id: call_id.clone(),
                    name: name.clone(),
                    input,
                }));
            }
        }
    }

    let stop_reason = if has_tool_calls {
        StopReason::ToolUse
    } else if resp.status == "incomplete" {
        StopReason::MaxTokens
    } else {
        StopReason::EndTurn
    };

    let usage = resp.usage.map(|u| Usage {
        input_tokens: u.input_tokens,
        output_tokens: u.output_tokens,
        thinking_tokens: u.output_tokens_details
            .and_then(|d| d.reasoning_tokens)
            .filter(|&v| v > 0),
    }).unwrap_or_default();

    LLMResponse {
        id: resp.id,
        model: resp.model,
        content,
        stop_reason,
        usage,
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_requires_responses_api() {
        assert!(requires_responses_api("gpt-5.2-pro-2025-12-11"));
        assert!(requires_responses_api("gpt-5-pro-2025-10-06"));
        assert!(!requires_responses_api("gpt-5.2-2025-12-11"));
        assert!(!requires_responses_api("gpt-4o"));
        assert!(!requires_responses_api("gpt-4o-mini"));
        assert!(!requires_responses_api("o3-mini"));
    }

    #[test]
    fn test_parse_chat_sse_text_delta() {
        let data = r#"{"id":"chatcmpl-123","choices":[{"delta":{"content":"Hello"},"index":0}]}"#;
        let mut message_id = String::new();
        let mut tool_id = String::new();
        let mut tool_name = String::new();
        let mut block_index = 0;
        let mut in_tool = false;

        let chunks = parse_chat_sse_data(
            data,
            &mut message_id,
            &mut tool_id,
            &mut tool_name,
            &mut block_index,
            &mut in_tool,
        )
        .unwrap();

        assert!(chunks.len() >= 3);
        assert!(matches!(chunks[0], StreamChunk::Start { .. }));
        assert!(matches!(chunks[1], StreamChunk::BlockStart { block_type: BlockType::Text, .. }));
        assert!(matches!(&chunks[2], StreamChunk::Delta(StreamDelta::Text(t)) if t == "Hello"));
        // block_index should now be 1 (fix for the bug)
        assert_eq!(block_index, 1);
    }

    #[test]
    fn test_parse_chat_sse_stop() {
        let data = r#"{"id":"chatcmpl-123","choices":[{"delta":{},"finish_reason":"stop","index":0}]}"#;
        let mut message_id = "chatcmpl-123".to_string();
        let mut tool_id = String::new();
        let mut tool_name = String::new();
        let mut block_index = 1;
        let mut in_tool = false;

        let chunks = parse_chat_sse_data(
            data,
            &mut message_id,
            &mut tool_id,
            &mut tool_name,
            &mut block_index,
            &mut in_tool,
        )
        .unwrap();

        // Should have BlockStop then Stop
        assert!(chunks.iter().any(|c| matches!(c, StreamChunk::BlockStop { index: 0 })));
        assert!(chunks.iter().any(|c| matches!(c, StreamChunk::Stop { reason: StopReason::EndTurn, .. })));
    }

    #[test]
    fn test_parse_chat_sse_tool_call() {
        let data = r#"{"id":"chatcmpl-123","choices":[{"delta":{"tool_calls":[{"index":0,"id":"call_abc","type":"function","function":{"name":"get_weather","arguments":""}}]},"index":0}]}"#;
        let mut message_id = "chatcmpl-123".to_string();
        let mut tool_id = String::new();
        let mut tool_name = String::new();
        let mut block_index = 1;
        let mut in_tool = false;

        let chunks = parse_chat_sse_data(
            data,
            &mut message_id,
            &mut tool_id,
            &mut tool_name,
            &mut block_index,
            &mut in_tool,
        )
        .unwrap();

        assert!(chunks.iter().any(|c| matches!(c, StreamChunk::BlockStop { index: 0 })));
        assert!(chunks.iter().any(|c| matches!(c, StreamChunk::BlockStart { block_type: BlockType::ToolUse, .. })));
        assert_eq!(tool_id, "call_abc");
        assert_eq!(tool_name, "get_weather");
    }

    #[test]
    fn test_convert_chat_response() {
        let resp = ChatResponse {
            id: "chatcmpl-123".to_string(),
            model: "gpt-4o".to_string(),
            choices: vec![ChatChoice {
                message: ChatMessage {
                    content: Some("Hello, world!".to_string()),
                    tool_calls: None,
                },
                finish_reason: Some("stop".to_string()),
            }],
            usage: ChatUsage {
                prompt_tokens: 10,
                completion_tokens: 5,
            },
        };

        let response = convert_chat_response(resp);
        assert_eq!(response.id, "chatcmpl-123");
        assert_eq!(response.model, "gpt-4o");
        assert!(matches!(response.stop_reason, StopReason::EndTurn));
        assert_eq!(response.usage.input_tokens, 10);
        assert_eq!(response.usage.output_tokens, 5);
    }

    #[test]
    fn test_parse_responses_text_delta() {
        let data = r#"{"type":"response.output_text.delta","content_index":0,"delta":"Hello!","item_id":"msg_123","output_index":0,"sequence_number":4}"#;
        let mut started = true;
        let mut block_index = 1;
        let mut in_text = true;
        let mut tool_indices = std::collections::HashMap::new();

        let chunks = parse_responses_sse_data(
            data,
            &mut started,
            &mut block_index,
            &mut in_text,
            &mut tool_indices,
        )
        .unwrap();

        assert_eq!(chunks.len(), 1);
        assert!(matches!(&chunks[0], StreamChunk::Delta(StreamDelta::Text(t)) if t == "Hello!"));
    }

    #[test]
    fn test_parse_responses_function_call() {
        let data = r#"{"type":"response.output_item.added","item":{"id":"fc_123","type":"function_call","status":"in_progress","arguments":"","call_id":"call_abc","name":"file_read"},"output_index":0,"sequence_number":2}"#;
        let mut started = true;
        let mut block_index = 0;
        let mut in_text = false;
        let mut tool_indices = std::collections::HashMap::new();

        let chunks = parse_responses_sse_data(
            data,
            &mut started,
            &mut block_index,
            &mut in_text,
            &mut tool_indices,
        )
        .unwrap();

        assert!(chunks.iter().any(|c| matches!(c, StreamChunk::BlockStart { block_type: BlockType::ToolUse, .. })));
        assert_eq!(block_index, 1);
        assert_eq!(*tool_indices.get(&0).unwrap(), 0);
    }

    #[test]
    fn test_parse_responses_completed() {
        let data = r#"{"type":"response.completed","response":{"id":"resp_123","status":"completed","output":[],"usage":{"input_tokens":20,"output_tokens":10,"output_tokens_details":{"reasoning_tokens":0}},"model":"gpt-5.2-pro"},"sequence_number":8}"#;
        let mut started = true;
        let mut block_index = 1;
        let mut in_text = false;
        let mut tool_indices = std::collections::HashMap::new();

        let chunks = parse_responses_sse_data(
            data,
            &mut started,
            &mut block_index,
            &mut in_text,
            &mut tool_indices,
        )
        .unwrap();

        assert!(chunks.iter().any(|c| matches!(c, StreamChunk::Stop { reason: StopReason::EndTurn, .. })));
    }

    #[test]
    fn test_parse_error_response() {
        let body = r#"{"error":{"message":"Invalid API key","type":"invalid_api_key","code":"invalid_api_key"}}"#;
        let err = parse_error_response(401, body);
        assert!(matches!(err, LLMError::AuthError { .. }));

        let body = r#"{"error":{"message":"Rate limit exceeded","type":"rate_limit_error"}}"#;
        let err = parse_error_response(429, body);
        assert!(matches!(err, LLMError::RateLimit { .. }));
    }
}
