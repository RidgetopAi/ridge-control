// TRC-008: xAI Grok provider - fully implemented but not yet used in main app
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

const GROK_API_URL: &str = "https://api.x.ai/v1/chat/completions";

/// xAI Grok provider (OpenAI-compatible API)
pub struct GrokProvider {
    api_key: String,
    http_client: Client,
    models: Vec<ModelInfo>,
    default_model: String,
}

impl GrokProvider {
    pub fn new(api_key: impl Into<String>) -> Self {
        let api_key = api_key.into();
        let http_client = Client::new();

        let models = vec![
            ModelInfo::new("grok-4", "Grok 4")
                .with_thinking()
                .with_context_window(256_000)
                .with_max_output(32_768),
            ModelInfo::new("grok-4-fast-reasoning", "Grok 4 Fast (Reasoning)")
                .with_thinking()
                .with_context_window(2_000_000)
                .with_max_output(32_768),
            ModelInfo::new("grok-4-fast-non-reasoning", "Grok 4 Fast")
                .with_context_window(2_000_000)
                .with_max_output(32_768),
            ModelInfo::new("grok-4-1-fast-reasoning", "Grok 4.1 Fast (Reasoning)")
                .with_thinking()
                .with_context_window(2_000_000)
                .with_max_output(32_768),
            ModelInfo::new("grok-4-1-fast-non-reasoning", "Grok 4.1 Fast")
                .with_context_window(2_000_000)
                .with_max_output(32_768),
            ModelInfo::new("grok-code-fast-1", "Grok Code Fast")
                .with_thinking()
                .with_context_window(256_000)
                .with_max_output(32_768),
            ModelInfo::new("grok-3", "Grok 3")
                .with_context_window(131_072)
                .with_max_output(16_384),
            ModelInfo::new("grok-3-mini", "Grok 3 Mini")
                .with_context_window(131_072)
                .with_max_output(16_384),
            ModelInfo::new("grok-2-1212", "Grok 2")
                .with_context_window(131_072)
                .with_max_output(8_192),
            ModelInfo::new("grok-2-vision-1212", "Grok 2 Vision")
                .with_context_window(32_768)
                .with_max_output(8_192),
        ];

        Self {
            api_key,
            http_client,
            models,
            default_model: "grok-4-fast-non-reasoning".to_string(),
        }
    }

    fn build_request_body(&self, request: &LLMRequest) -> serde_json::Value {
        let mut messages: Vec<serde_json::Value> = Vec::new();

        if let Some(system) = &request.system {
            messages.push(json!({
                "role": "system",
                "content": system
            }));
        }

        for m in &request.messages {
            let role = match m.role {
                super::types::Role::User => "user",
                super::types::Role::Assistant => "assistant",
            };

            let content = self.convert_content_blocks(&m.content);
            
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
            } else if content != json!(null) && content != json!([]) {
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

                let mut msg = json!({
                    "role": role,
                    "content": content
                });

                if !tool_calls.is_empty() {
                    msg["tool_calls"] = json!(tool_calls);
                }

                messages.push(msg);
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
            body["stream_options"] = json!({
                "include_usage": true
            });
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
                ContentBlock::Image(img) => {
                    let url = match &img.source {
                        super::types::ImageSource::Base64(b64) => {
                            format!("data:{};base64,{}", img.media_type, b64)
                        }
                        super::types::ImageSource::Url(url) => url.clone(),
                    };
                    Some(json!({
                        "type": "image_url",
                        "image_url": {
                            "url": url
                        }
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
impl Provider for GrokProvider {
    fn name(&self) -> &str {
        "grok"
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
            Capability::Streaming
                | Capability::ToolUse
                | Capability::Vision
                | Capability::Reasoning
                | Capability::LiveSearch
        )
    }

    async fn complete(&self, request: LLMRequest) -> Result<LLMResponse, LLMError> {
        let mut req = request;
        req.stream = false;

        let body = self.build_request_body(&req);

        let response = self
            .http_client
            .post(GROK_API_URL)
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

        let resp: GrokResponse = response.json().await.map_err(|e| LLMError::ParseError {
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
            .post(GROK_API_URL)
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

        tokio::spawn(async move {
            parse_sse_stream(byte_stream, tx).await;
        });

        let stream: StreamBox = Box::pin(ReceiverStream::new(rx));
        Ok(stream)
    }
}

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
) -> Option<Vec<StreamChunk>> {
    let json: serde_json::Value = serde_json::from_str(data).ok()?;
    let mut chunks = Vec::new();

    if message_id.is_empty() {
        if let Some(id) = json["id"].as_str() {
            *message_id = id.to_string();
            chunks.push(StreamChunk::Start {
                message_id: message_id.clone(),
            });
            chunks.push(StreamChunk::BlockStart {
                index: 0,
                block_type: BlockType::Text,
            });
        }
    }

    if let Some(choices) = json["choices"].as_array() {
        for choice in choices {
            let delta = &choice["delta"];

            if let Some(tool_calls) = delta["tool_calls"].as_array() {
                for tool_call in tool_calls {
                    let tc_index = tool_call["index"].as_u64().unwrap_or(0) as usize;
                    
                    if let Some(id) = tool_call["id"].as_str() {
                        if !*in_tool_block && *block_index > 0 {
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
                        });
                        *block_index = tc_index + 2;
                    }
                    
                    if let Some(func) = tool_call["function"].as_object() {
                        if let Some(args) = func.get("arguments").and_then(|a| a.as_str()) {
                            if !args.is_empty() {
                                chunks.push(StreamChunk::Delta(StreamDelta::ToolInput {
                                    id: current_tool_id.clone(),
                                    name: if current_tool_name.is_empty() {
                                        None
                                    } else {
                                        Some(current_tool_name.clone())
                                    },
                                    input_json: args.to_string(),
                                }));
                            }
                        }
                    }
                }
            }

            if let Some(content) = delta["content"].as_str() {
                if !content.is_empty() {
                    if *in_tool_block {
                        *in_tool_block = false;
                        chunks.push(StreamChunk::BlockStop { index: *block_index - 1 });
                        chunks.push(StreamChunk::BlockStart {
                            index: *block_index,
                            block_type: BlockType::Text,
                        });
                        *block_index += 1;
                    }
                    chunks.push(StreamChunk::Delta(StreamDelta::Text(content.to_string())));
                }
            }

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
                    thinking_tokens: u.get("completion_tokens_details")
                        .and_then(|d| d.get("reasoning_tokens"))
                        .and_then(|v| v.as_u64())
                        .map(|v| v as u32),
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

fn convert_response(resp: GrokResponse) -> LLMResponse {
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
            thinking_tokens: resp.usage.completion_tokens_details
                .and_then(|d| d.reasoning_tokens),
        },
    }
}

#[derive(Debug, Deserialize)]
struct GrokResponse {
    id: String,
    model: String,
    choices: Vec<GrokChoice>,
    usage: GrokUsage,
}

#[derive(Debug, Deserialize, Default)]
struct GrokChoice {
    message: GrokMessage,
    finish_reason: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
struct GrokMessage {
    content: Option<String>,
    tool_calls: Option<Vec<GrokToolCall>>,
}

#[derive(Debug, Deserialize)]
struct GrokToolCall {
    id: String,
    #[serde(rename = "type")]
    #[allow(dead_code)]
    call_type: String,
    function: GrokFunction,
}

#[derive(Debug, Deserialize)]
struct GrokFunction {
    name: String,
    arguments: String,
}

#[derive(Debug, Deserialize, Default)]
struct GrokUsage {
    prompt_tokens: u32,
    completion_tokens: u32,
    completion_tokens_details: Option<GrokCompletionDetails>,
}

#[derive(Debug, Deserialize)]
struct GrokCompletionDetails {
    reasoning_tokens: Option<u32>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_sse_text_delta() {
        let data = r#"{"id":"chatcmpl-grok123","choices":[{"delta":{"content":"Hello from Grok"},"index":0}]}"#;
        let mut message_id = String::new();
        let mut tool_id = String::new();
        let mut tool_name = String::new();
        let mut block_index = 0;
        let mut in_tool = false;

        let chunks = parse_sse_data(
            data,
            &mut message_id,
            &mut tool_id,
            &mut tool_name,
            &mut block_index,
            &mut in_tool,
        )
        .unwrap();

        assert!(chunks.len() >= 2);
        assert!(matches!(chunks[0], StreamChunk::Start { .. }));
        assert!(matches!(chunks[1], StreamChunk::BlockStart { block_type: BlockType::Text, .. }));
        assert!(matches!(&chunks[2], StreamChunk::Delta(StreamDelta::Text(t)) if t == "Hello from Grok"));
    }

    #[test]
    fn test_parse_sse_stop() {
        let data = r#"{"id":"chatcmpl-grok123","choices":[{"delta":{},"finish_reason":"stop","index":0}]}"#;
        let mut message_id = "chatcmpl-grok123".to_string();
        let mut tool_id = String::new();
        let mut tool_name = String::new();
        let mut block_index = 1;
        let mut in_tool = false;

        let chunks = parse_sse_data(
            data,
            &mut message_id,
            &mut tool_id,
            &mut tool_name,
            &mut block_index,
            &mut in_tool,
        )
        .unwrap();

        assert!(chunks.iter().any(|c| matches!(c, StreamChunk::Stop { reason: StopReason::EndTurn, .. })));
    }

    #[test]
    fn test_parse_sse_tool_call() {
        let data = r#"{"id":"chatcmpl-grok123","choices":[{"delta":{"tool_calls":[{"index":0,"id":"call_xyz","type":"function","function":{"name":"search_web","arguments":""}}]},"index":0}]}"#;
        let mut message_id = "chatcmpl-grok123".to_string();
        let mut tool_id = String::new();
        let mut tool_name = String::new();
        let mut block_index = 1;
        let mut in_tool = false;

        let chunks = parse_sse_data(
            data,
            &mut message_id,
            &mut tool_id,
            &mut tool_name,
            &mut block_index,
            &mut in_tool,
        )
        .unwrap();

        assert!(chunks.iter().any(|c| matches!(c, StreamChunk::BlockStart { block_type: BlockType::ToolUse, .. })));
        assert_eq!(tool_id, "call_xyz");
        assert_eq!(tool_name, "search_web");
    }

    #[test]
    fn test_convert_response() {
        let resp = GrokResponse {
            id: "chatcmpl-grok123".to_string(),
            model: "grok-4".to_string(),
            choices: vec![GrokChoice {
                message: GrokMessage {
                    content: Some("Hello from Grok!".to_string()),
                    tool_calls: None,
                },
                finish_reason: Some("stop".to_string()),
            }],
            usage: GrokUsage {
                prompt_tokens: 15,
                completion_tokens: 8,
                completion_tokens_details: Some(GrokCompletionDetails {
                    reasoning_tokens: Some(100),
                }),
            },
        };

        let response = convert_response(resp);
        assert_eq!(response.id, "chatcmpl-grok123");
        assert_eq!(response.model, "grok-4");
        assert!(matches!(response.stop_reason, StopReason::EndTurn));
        assert_eq!(response.usage.input_tokens, 15);
        assert_eq!(response.usage.output_tokens, 8);
        assert_eq!(response.usage.thinking_tokens, Some(100));
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

    #[test]
    fn test_convert_response_with_tool_calls() {
        let resp = GrokResponse {
            id: "chatcmpl-grok456".to_string(),
            model: "grok-4-fast-reasoning".to_string(),
            choices: vec![GrokChoice {
                message: GrokMessage {
                    content: None,
                    tool_calls: Some(vec![GrokToolCall {
                        id: "call_abc".to_string(),
                        call_type: "function".to_string(),
                        function: GrokFunction {
                            name: "search_web".to_string(),
                            arguments: r#"{"query":"weather"}"#.to_string(),
                        },
                    }]),
                },
                finish_reason: Some("tool_calls".to_string()),
            }],
            usage: GrokUsage {
                prompt_tokens: 20,
                completion_tokens: 15,
                completion_tokens_details: None,
            },
        };

        let response = convert_response(resp);
        assert_eq!(response.id, "chatcmpl-grok456");
        assert!(matches!(response.stop_reason, StopReason::ToolUse));
        assert_eq!(response.content.len(), 1);
        assert!(matches!(&response.content[0], ContentBlock::ToolUse(tu) if tu.name == "search_web"));
    }

    #[test]
    fn test_provider_info() {
        let provider = GrokProvider::new("test-key");
        assert_eq!(provider.name(), "grok");
        assert_eq!(provider.default_model(), "grok-4-fast-non-reasoning");
        assert!(provider.supports(Capability::Streaming));
        assert!(provider.supports(Capability::ToolUse));
        assert!(provider.supports(Capability::Vision));
        assert!(provider.supports(Capability::Reasoning));
        assert!(provider.supports(Capability::LiveSearch));
        assert!(!provider.supports(Capability::Thinking));
        assert!(provider.models().len() >= 8);
    }
}
