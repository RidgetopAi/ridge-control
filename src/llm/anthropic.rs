// Anthropic provider - some response types for non-streaming mode
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

const ANTHROPIC_API_URL: &str = "https://api.anthropic.com/v1/messages";
const ANTHROPIC_VERSION: &str = "2023-06-01";

/// Anthropic Claude provider
pub struct AnthropicProvider {
    api_key: String,
    http_client: Client,
    models: Vec<ModelInfo>,
    default_model: String,
}

impl AnthropicProvider {
    pub fn new(api_key: impl Into<String>) -> Self {
        let api_key = api_key.into();
        let http_client = Client::new();

        let models = vec![
            // Claude 4.5 series (latest)
            ModelInfo::new("claude-opus-4-5-20251101", "Claude Opus 4.5")
                .with_thinking()
                .with_context_window(200_000)
                .with_max_output(16384),
            ModelInfo::new("claude-sonnet-4-5-20250929", "Claude Sonnet 4.5")
                .with_thinking()
                .with_context_window(200_000)
                .with_max_output(16384),
            ModelInfo::new("claude-haiku-4-5-20251001", "Claude Haiku 4.5")
                .with_thinking()
                .with_context_window(200_000)
                .with_max_output(8192),
            // Claude 4 series
            ModelInfo::new("claude-sonnet-4-20250514", "Claude Sonnet 4")
                .with_thinking()
                .with_context_window(200_000)
                .with_max_output(8192),
            ModelInfo::new("claude-opus-4-20250514", "Claude Opus 4")
                .with_thinking()
                .with_context_window(200_000)
                .with_max_output(8192),
            // Claude 3.5 series
            ModelInfo::new("claude-3-5-sonnet-20241022", "Claude 3.5 Sonnet")
                .with_context_window(200_000)
                .with_max_output(8192),
            ModelInfo::new("claude-3-5-haiku-20241022", "Claude 3.5 Haiku")
                .with_context_window(200_000)
                .with_max_output(8192),
        ];

        Self {
            api_key,
            http_client,
            models,
            default_model: "claude-sonnet-4-5-20250929".to_string(),
        }
    }

    fn build_request_body(&self, request: &LLMRequest) -> serde_json::Value {
        let messages: Vec<serde_json::Value> = request
            .messages
            .iter()
            .map(|m| {
                let role = match m.role {
                    super::types::Role::User => "user",
                    super::types::Role::Assistant => "assistant",
                };

                let content: Vec<serde_json::Value> = m
                    .content
                    .iter()
                    .filter_map(|c| match c {
                        ContentBlock::Text(text) => Some(json!({
                            "type": "text",
                            "text": text
                        })),
                        ContentBlock::ToolUse(tool_use) => Some(json!({
                            "type": "tool_use",
                            "id": tool_use.id,
                            "name": tool_use.name,
                            "input": tool_use.input
                        })),
                        ContentBlock::ToolResult(result) => Some(json!({
                            "type": "tool_result",
                            "tool_use_id": result.tool_use_id,
                            "content": match &result.content {
                                super::types::ToolResultContent::Text(t) => t.clone(),
                                super::types::ToolResultContent::Json(j) => j.to_string(),
                                super::types::ToolResultContent::Image(_) => "[image]".to_string(),
                            },
                            "is_error": result.is_error
                        })),
                        _ => None,
                    })
                    .collect();

                json!({
                    "role": role,
                    "content": content
                })
            })
            .collect();

        let model_to_use = if request.model.is_empty() { &self.default_model } else { &request.model };
        tracing::info!("Anthropic API request - model: '{}' (request.model was: '{}')", model_to_use, request.model);
        
        let mut body = json!({
            "model": model_to_use,
            "messages": messages,
            "max_tokens": request.max_tokens.unwrap_or(4096),
            "stream": request.stream,
        });

        if let Some(system) = &request.system {
            body["system"] = json!(system);
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
                        "name": t.name,
                        "description": t.description,
                        "input_schema": t.input_schema
                    })
                })
                .collect();
            body["tools"] = json!(tools);
        }

        body
    }
}

#[async_trait]
impl Provider for AnthropicProvider {
    fn name(&self) -> &str {
        "anthropic"
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
            Capability::Streaming | Capability::ToolUse | Capability::Vision | Capability::Thinking
        )
    }

    async fn complete(&self, request: LLMRequest) -> Result<LLMResponse, LLMError> {
        let mut req = request;
        req.stream = false;

        let body = self.build_request_body(&req);

        let response = self
            .http_client
            .post(ANTHROPIC_API_URL)
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", ANTHROPIC_VERSION)
            .header("content-type", "application/json")
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

        let resp: AnthropicResponse = response.json().await.map_err(|e| LLMError::ParseError {
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
            .post(ANTHROPIC_API_URL)
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", ANTHROPIC_VERSION)
            .header("content-type", "application/json")
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

    async fn test_key(&self) -> Result<(), LLMError> {
        let body = json!({
            "model": "claude-3-5-haiku-20241022",
            "messages": [{"role": "user", "content": [{"type": "text", "text": "Hi"}]}],
            "max_tokens": 1,
        });

        let response = self
            .http_client
            .post(ANTHROPIC_API_URL)
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", ANTHROPIC_VERSION)
            .header("content-type", "application/json")
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

async fn parse_sse_stream(
    mut byte_stream: impl Stream<Item = Result<bytes::Bytes, reqwest::Error>> + Unpin,
    tx: mpsc::Sender<Result<StreamChunk, LLMError>>,
) {
    let mut buffer = String::new();

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

        while let Some(pos) = buffer.find("\n\n") {
            let event_str = buffer[..pos].to_string();
            buffer = buffer[pos + 2..].to_string();

            if let Some(chunk) = parse_sse_event(&event_str) {
                if tx.send(Ok(chunk)).await.is_err() {
                    return;
                }
            }
        }
    }
}

fn parse_sse_event(event_str: &str) -> Option<StreamChunk> {
    let mut event_type = String::new();
    let mut data = String::new();

    for line in event_str.lines() {
        if let Some(et) = line.strip_prefix("event: ") {
            event_type = et.to_string();
        } else if let Some(d) = line.strip_prefix("data: ") {
            data = d.to_string();
        }
    }

    if data.is_empty() {
        return None;
    }

    let json: serde_json::Value = serde_json::from_str(&data).ok()?;

    match event_type.as_str() {
        "message_start" => {
            let message_id = json["message"]["id"].as_str()?.to_string();
            Some(StreamChunk::Start { message_id })
        }
        "content_block_start" => {
            let index = json["index"].as_u64()? as usize;
            let content_block = &json["content_block"];
            let block_type = match content_block["type"].as_str()? {
                "text" => BlockType::Text,
                "tool_use" => BlockType::ToolUse,
                "thinking" => BlockType::Thinking,
                _ => BlockType::Text,
            };
            Some(StreamChunk::BlockStart { index, block_type })
        }
        "content_block_delta" => {
            let delta = &json["delta"];
            let delta_type = delta["type"].as_str()?;

            let stream_delta = match delta_type {
                "text_delta" => {
                    let text = delta["text"].as_str()?.to_string();
                    StreamDelta::Text(text)
                }
                "input_json_delta" => {
                    let partial_json = delta["partial_json"].as_str()?.to_string();
                    StreamDelta::ToolInput {
                        id: String::new(),
                        name: None,
                        input_json: partial_json,
                    }
                }
                "thinking_delta" => {
                    let thinking = delta["thinking"].as_str()?.to_string();
                    StreamDelta::Thinking(thinking)
                }
                _ => return None,
            };

            Some(StreamChunk::Delta(stream_delta))
        }
        "content_block_stop" => {
            let index = json["index"].as_u64()? as usize;
            Some(StreamChunk::BlockStop { index })
        }
        "message_delta" => {
            let stop_reason = json["delta"]["stop_reason"].as_str().map(|s| match s {
                "end_turn" => StopReason::EndTurn,
                "max_tokens" => StopReason::MaxTokens,
                "stop_sequence" => StopReason::StopSequence,
                "tool_use" => StopReason::ToolUse,
                _ => StopReason::EndTurn,
            });

            let usage = json["usage"].as_object().map(|u| Usage {
                input_tokens: u.get("input_tokens").and_then(|v| v.as_u64()).unwrap_or(0) as u32,
                output_tokens: u.get("output_tokens").and_then(|v| v.as_u64()).unwrap_or(0) as u32,
                thinking_tokens: None,
            });

            stop_reason.map(|reason| StreamChunk::Stop { reason, usage })
        }
        "message_stop" => None,
        "ping" => None,
        "error" => {
            let error_msg = json["error"]["message"]
                .as_str()
                .unwrap_or("Unknown error")
                .to_string();
            Some(StreamChunk::Error(LLMError::ProviderError {
                status: 500,
                message: error_msg,
            }))
        }
        _ => None,
    }
}

fn parse_error_response(status: u16, body: &str) -> LLMError {
    if let Ok(json) = serde_json::from_str::<serde_json::Value>(body) {
        let message = json["error"]["message"]
            .as_str()
            .unwrap_or("Unknown error")
            .to_string();
        let error_type = json["error"]["type"].as_str().unwrap_or("");

        match error_type {
            "authentication_error" => LLMError::AuthError { message },
            "rate_limit_error" => LLMError::RateLimit {
                retry_after_secs: 60,
            },
            "invalid_request_error" => LLMError::InvalidRequest { message },
            _ => LLMError::ProviderError { status, message },
        }
    } else {
        LLMError::ProviderError {
            status,
            message: body.to_string(),
        }
    }
}

fn convert_response(resp: AnthropicResponse) -> LLMResponse {
    let content: Vec<ContentBlock> = resp
        .content
        .into_iter()
        .filter_map(|c| match c.content_type.as_str() {
            "text" => Some(ContentBlock::Text(c.text.unwrap_or_default())),
            "tool_use" => Some(ContentBlock::ToolUse(ToolUse {
                id: c.id.unwrap_or_default(),
                name: c.name.unwrap_or_default(),
                input: c.input.unwrap_or(serde_json::Value::Null),
            })),
            "thinking" => Some(ContentBlock::Thinking(c.thinking.unwrap_or_default())),
            _ => None,
        })
        .collect();

    let stop_reason = match resp.stop_reason.as_deref() {
        Some("end_turn") => StopReason::EndTurn,
        Some("max_tokens") => StopReason::MaxTokens,
        Some("stop_sequence") => StopReason::StopSequence,
        Some("tool_use") => StopReason::ToolUse,
        _ => StopReason::EndTurn,
    };

    LLMResponse {
        id: resp.id,
        model: resp.model,
        content,
        stop_reason,
        usage: Usage {
            input_tokens: resp.usage.input_tokens,
            output_tokens: resp.usage.output_tokens,
            thinking_tokens: None,
        },
    }
}

#[derive(Debug, Deserialize)]
struct AnthropicResponse {
    id: String,
    model: String,
    content: Vec<AnthropicContentBlock>,
    stop_reason: Option<String>,
    usage: AnthropicUsage,
}

#[derive(Debug, Deserialize)]
struct AnthropicContentBlock {
    #[serde(rename = "type")]
    content_type: String,
    text: Option<String>,
    id: Option<String>,
    name: Option<String>,
    input: Option<serde_json::Value>,
    thinking: Option<String>,
}

#[derive(Debug, Deserialize)]
struct AnthropicUsage {
    input_tokens: u32,
    output_tokens: u32,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_sse_message_start() {
        let event = "event: message_start\ndata: {\"message\":{\"id\":\"msg_123\"}}";
        let chunk = parse_sse_event(event);
        assert!(matches!(chunk, Some(StreamChunk::Start { message_id }) if message_id == "msg_123"));
    }

    #[test]
    fn test_parse_sse_text_delta() {
        let event = "event: content_block_delta\ndata: {\"delta\":{\"type\":\"text_delta\",\"text\":\"Hello\"}}";
        let chunk = parse_sse_event(event);
        assert!(matches!(chunk, Some(StreamChunk::Delta(StreamDelta::Text(t))) if t == "Hello"));
    }

    #[test]
    fn test_parse_sse_block_start() {
        let event = "event: content_block_start\ndata: {\"index\":0,\"content_block\":{\"type\":\"text\"}}";
        let chunk = parse_sse_event(event);
        assert!(matches!(chunk, Some(StreamChunk::BlockStart { index: 0, block_type: BlockType::Text })));
    }
}
