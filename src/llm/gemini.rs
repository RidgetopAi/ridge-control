// TRC-007: Google Gemini provider - fully implemented but not yet used in main app
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

const GEMINI_API_URL: &str = "https://generativelanguage.googleapis.com/v1beta/models";

/// Google Gemini provider
pub struct GeminiProvider {
    api_key: String,
    http_client: Client,
    models: Vec<ModelInfo>,
    default_model: String,
}

impl GeminiProvider {
    pub fn new(api_key: impl Into<String>) -> Self {
        let api_key = api_key.into();
        let http_client = Client::new();

        let models = vec![
            ModelInfo::new("gemini-2.5-flash", "Gemini 2.5 Flash")
                .with_context_window(1_000_000)
                .with_max_output(8192),
            ModelInfo::new("gemini-2.5-pro", "Gemini 2.5 Pro")
                .with_thinking()
                .with_context_window(1_000_000)
                .with_max_output(8192),
            ModelInfo::new("gemini-2.0-flash", "Gemini 2.0 Flash")
                .with_context_window(1_000_000)
                .with_max_output(8192),
            ModelInfo::new("gemini-1.5-pro", "Gemini 1.5 Pro")
                .with_context_window(2_000_000)
                .with_max_output(8192),
            ModelInfo::new("gemini-1.5-flash", "Gemini 1.5 Flash")
                .with_context_window(1_000_000)
                .with_max_output(8192),
        ];

        Self {
            api_key,
            http_client,
            models,
            default_model: "gemini-2.5-flash".to_string(),
        }
    }

    fn build_request_body(&self, request: &LLMRequest) -> serde_json::Value {
        let mut contents: Vec<serde_json::Value> = Vec::new();

        for m in &request.messages {
            let role = match m.role {
                super::types::Role::User => "user",
                super::types::Role::Assistant => "model",
            };

            let parts: Vec<serde_json::Value> = m
                .content
                .iter()
                .filter_map(|c| match c {
                    ContentBlock::Text(text) => Some(json!({ "text": text })),
                    ContentBlock::Image(img) => {
                        let data = match &img.source {
                            super::types::ImageSource::Base64(b64) => b64.clone(),
                            super::types::ImageSource::Url(_) => return None,
                        };
                        Some(json!({
                            "inline_data": {
                                "mime_type": img.media_type,
                                "data": data
                            }
                        }))
                    }
                    ContentBlock::ToolUse(tool_use) => Some(json!({
                        "functionCall": {
                            "name": tool_use.name,
                            "args": tool_use.input
                        }
                    })),
                    ContentBlock::ToolResult(result) => {
                        let response = match &result.content {
                            super::types::ToolResultContent::Text(t) => json!({ "output": t }),
                            super::types::ToolResultContent::Json(j) => json!({ "output": j }),
                            super::types::ToolResultContent::Image(_) => {
                                json!({ "output": "[image]" })
                            }
                        };
                        Some(json!({
                            "functionResponse": {
                                "name": result.tool_use_id,
                                "response": response
                            }
                        }))
                    }
                    ContentBlock::Thinking(_) => None,
                })
                .collect();

            if !parts.is_empty() {
                contents.push(json!({
                    "role": role,
                    "parts": parts
                }));
            }
        }

        let mut body = json!({
            "contents": contents,
        });

        if let Some(system) = &request.system {
            body["systemInstruction"] = json!({
                "parts": [{ "text": system }]
            });
        }

        let mut generation_config = json!({});

        if let Some(max_tokens) = request.max_tokens {
            generation_config["maxOutputTokens"] = json!(max_tokens);
        }

        if let Some(temp) = request.temperature {
            generation_config["temperature"] = json!(temp);
        }

        if generation_config.as_object().is_some_and(|o| !o.is_empty()) {
            body["generationConfig"] = generation_config;
        }

        if !request.tools.is_empty() {
            let function_declarations: Vec<serde_json::Value> = request
                .tools
                .iter()
                .map(|t| {
                    json!({
                        "name": t.name,
                        "description": t.description,
                        "parameters": t.input_schema
                    })
                })
                .collect();
            body["tools"] = json!([{
                "functionDeclarations": function_declarations
            }]);
        }

        body
    }

    fn get_endpoint(&self, model: &str, streaming: bool) -> String {
        let action = if streaming {
            "streamGenerateContent"
        } else {
            "generateContent"
        };
        format!(
            "{}/{}:{}?key={}",
            GEMINI_API_URL, model, action, self.api_key
        )
    }
}

#[async_trait]
impl Provider for GeminiProvider {
    fn name(&self) -> &str {
        "google"
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
                | Capability::CodeExecution
        )
    }

    async fn complete(&self, request: LLMRequest) -> Result<LLMResponse, LLMError> {
        let model = if request.model.is_empty() {
            &self.default_model
        } else {
            &request.model
        };

        let body = self.build_request_body(&request);
        let url = self.get_endpoint(model, false);

        let response = self
            .http_client
            .post(&url)
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

        let resp: GeminiResponse = response.json().await.map_err(|e| LLMError::ParseError {
            message: e.to_string(),
        })?;

        Ok(convert_response(resp, model))
    }

    async fn stream(&self, request: LLMRequest) -> Result<StreamBox, LLMError> {
        let model = if request.model.is_empty() {
            self.default_model.clone()
        } else {
            request.model.clone()
        };

        let body = self.build_request_body(&request);
        let url = self.get_endpoint(&model, true);

        let response = self
            .http_client
            .post(&url)
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
            parse_stream(byte_stream, tx, model).await;
        });

        let stream: StreamBox = Box::pin(ReceiverStream::new(rx));
        Ok(stream)
    }
}

async fn parse_stream(
    mut byte_stream: impl Stream<Item = Result<bytes::Bytes, reqwest::Error>> + Unpin,
    tx: mpsc::Sender<Result<StreamChunk, LLMError>>,
    model: String,
) {
    let mut buffer = String::new();
    let mut sent_start = false;
    let mut block_index: usize = 0;
    let mut in_function_call = false;

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

        while let Some(json_chunk) = extract_json_object(&mut buffer) {
            if let Some(chunks) = parse_json_chunk(
                &json_chunk,
                &model,
                &mut sent_start,
                &mut block_index,
                &mut in_function_call,
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

fn extract_json_object(buffer: &mut String) -> Option<String> {
    let trimmed = buffer.trim_start();

    if let Some(stripped) = trimmed.strip_prefix('[') {
        *buffer = stripped.to_string();
        return extract_json_object(buffer);
    }

    if let Some(stripped) = trimmed.strip_prefix(',') {
        *buffer = stripped.to_string();
        return extract_json_object(buffer);
    }

    if let Some(stripped) = trimmed.strip_prefix(']') {
        *buffer = stripped.to_string();
        return None;
    }

    if !trimmed.starts_with('{') {
        return None;
    }

    let mut depth = 0;
    let mut in_string = false;
    let mut escape = false;

    for (i, c) in trimmed.char_indices() {
        if escape {
            escape = false;
            continue;
        }

        match c {
            '\\' if in_string => escape = true,
            '"' => in_string = !in_string,
            '{' if !in_string => depth += 1,
            '}' if !in_string => {
                depth -= 1;
                if depth == 0 {
                    let json_obj = trimmed[..=i].to_string();
                    *buffer = trimmed[i + 1..].to_string();
                    return Some(json_obj);
                }
            }
            _ => {}
        }
    }

    None
}

fn parse_json_chunk(
    json_str: &str,
    _model: &str,
    sent_start: &mut bool,
    block_index: &mut usize,
    in_function_call: &mut bool,
) -> Option<Vec<StreamChunk>> {
    let json: serde_json::Value = serde_json::from_str(json_str).ok()?;
    let mut chunks = Vec::new();

    if !*sent_start {
        let message_id = json["responseId"]
            .as_str()
            .unwrap_or(&format!("gemini-{}", uuid::Uuid::new_v4()))
            .to_string();
        chunks.push(StreamChunk::Start { message_id });
        *sent_start = true;
    }

    if let Some(candidates) = json["candidates"].as_array() {
        for candidate in candidates {
            if let Some(content) = candidate["content"].as_object() {
                if let Some(parts) = content.get("parts").and_then(|p| p.as_array()) {
                    for part in parts {
                        if let Some(text) = part["text"].as_str() {
                            if *in_function_call {
                                chunks.push(StreamChunk::BlockStop {
                                    index: *block_index,
                                });
                                *block_index += 1;
                                *in_function_call = false;
                            }

                            if *block_index == 0
                                || !chunks
                                    .iter()
                                    .any(|c| matches!(c, StreamChunk::BlockStart { .. }))
                            {
                                chunks.push(StreamChunk::BlockStart {
                                    index: *block_index,
                                    block_type: BlockType::Text,
                                });
                            }

                            chunks.push(StreamChunk::Delta(StreamDelta::Text(text.to_string())));
                        }

                        if let Some(function_call) = part.get("functionCall") {
                            if !*in_function_call {
                                if *block_index > 0 {
                                    chunks.push(StreamChunk::BlockStop {
                                        index: *block_index - 1,
                                    });
                                }
                                chunks.push(StreamChunk::BlockStart {
                                    index: *block_index,
                                    block_type: BlockType::ToolUse,
                                });
                                *in_function_call = true;
                            }

                            let name = function_call["name"].as_str().unwrap_or("").to_string();
                            let args = function_call
                                .get("args")
                                .map(|a| a.to_string())
                                .unwrap_or_default();

                            chunks.push(StreamChunk::Delta(StreamDelta::ToolInput {
                                id: format!("call_{}", uuid::Uuid::new_v4()),
                                name: Some(name),
                                input_json: args,
                            }));
                        }
                    }
                }
            }

            if let Some(finish_reason) = candidate["finishReason"].as_str() {
                let reason = match finish_reason {
                    "STOP" => StopReason::EndTurn,
                    "MAX_TOKENS" => StopReason::MaxTokens,
                    "SAFETY" => StopReason::ContentFilter,
                    "RECITATION" => StopReason::ContentFilter,
                    "TOOL_USE" | "FUNCTION_CALL" => StopReason::ToolUse,
                    _ => StopReason::EndTurn,
                };

                if *block_index > 0 || *in_function_call {
                    chunks.push(StreamChunk::BlockStop {
                        index: *block_index,
                    });
                }

                let usage = json["usageMetadata"].as_object().map(|u| Usage {
                    input_tokens: u
                        .get("promptTokenCount")
                        .and_then(|v| v.as_u64())
                        .unwrap_or(0) as u32,
                    output_tokens: u
                        .get("candidatesTokenCount")
                        .and_then(|v| v.as_u64())
                        .unwrap_or(0) as u32,
                    thinking_tokens: None,
                });

                chunks.push(StreamChunk::Stop { reason, usage });
            }
        }
    }

    if let Some(error) = json.get("error") {
        let message = error["message"]
            .as_str()
            .unwrap_or("Unknown error")
            .to_string();
        chunks.push(StreamChunk::Error(LLMError::ProviderError {
            status: error["code"].as_u64().unwrap_or(500) as u16,
            message,
        }));
    }

    if chunks.is_empty() {
        None
    } else {
        Some(chunks)
    }
}

fn parse_error_response(status: u16, body: &str) -> LLMError {
    if let Ok(json) = serde_json::from_str::<serde_json::Value>(body) {
        let error = &json["error"];
        let message = error["message"]
            .as_str()
            .unwrap_or("Unknown error")
            .to_string();
        let status_code = error["code"].as_u64().unwrap_or(status as u64) as u16;

        match status {
            401 | 403 => LLMError::AuthError { message },
            429 => LLMError::RateLimit {
                retry_after_secs: 60,
            },
            400 => {
                if message.contains("API key") {
                    LLMError::AuthError { message }
                } else {
                    LLMError::InvalidRequest { message }
                }
            }
            404 => LLMError::ModelNotFound { model: message },
            _ => LLMError::ProviderError {
                status: status_code,
                message,
            },
        }
    } else {
        LLMError::ProviderError {
            status,
            message: body.to_string(),
        }
    }
}

fn convert_response(resp: GeminiResponse, model: &str) -> LLMResponse {
    let candidate = resp.candidates.into_iter().next().unwrap_or_default();
    let mut content: Vec<ContentBlock> = Vec::new();

    if let Some(parts) = candidate.content.and_then(|c| c.parts) {
        for part in parts {
            if let Some(text) = part.text {
                content.push(ContentBlock::Text(text));
            }
            if let Some(function_call) = part.function_call {
                content.push(ContentBlock::ToolUse(ToolUse {
                    id: format!("call_{}", uuid::Uuid::new_v4()),
                    name: function_call.name,
                    input: function_call.args.unwrap_or(serde_json::Value::Null),
                }));
            }
        }
    }

    let stop_reason = match candidate.finish_reason.as_deref() {
        Some("STOP") => StopReason::EndTurn,
        Some("MAX_TOKENS") => StopReason::MaxTokens,
        Some("SAFETY") | Some("RECITATION") => StopReason::ContentFilter,
        Some("TOOL_USE") | Some("FUNCTION_CALL") => StopReason::ToolUse,
        _ => StopReason::EndTurn,
    };

    let usage = resp.usage_metadata.map_or(Usage::default(), |u| Usage {
        input_tokens: u.prompt_token_count,
        output_tokens: u.candidates_token_count,
        thinking_tokens: None,
    });

    LLMResponse {
        id: resp.response_id.unwrap_or_else(|| format!("gemini-{}", uuid::Uuid::new_v4())),
        model: model.to_string(),
        content,
        stop_reason,
        usage,
    }
}

#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct GeminiResponse {
    candidates: Vec<GeminiCandidate>,
    usage_metadata: Option<GeminiUsageMetadata>,
    #[serde(default)]
    response_id: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct GeminiCandidate {
    content: Option<GeminiContent>,
    finish_reason: Option<String>,
}

#[derive(Debug, Deserialize)]
struct GeminiContent {
    parts: Option<Vec<GeminiPart>>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct GeminiPart {
    text: Option<String>,
    function_call: Option<GeminiFunctionCall>,
}

#[derive(Debug, Deserialize)]
struct GeminiFunctionCall {
    name: String,
    args: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct GeminiUsageMetadata {
    prompt_token_count: u32,
    candidates_token_count: u32,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_json_object_single() {
        let mut buffer = r#"{"text": "hello"}"#.to_string();
        let result = extract_json_object(&mut buffer);
        assert_eq!(result, Some(r#"{"text": "hello"}"#.to_string()));
        assert!(buffer.is_empty());
    }

    #[test]
    fn test_extract_json_object_array() {
        let mut buffer = r#"[{"text": "hello"}, {"text": "world"}]"#.to_string();
        let result1 = extract_json_object(&mut buffer);
        assert_eq!(result1, Some(r#"{"text": "hello"}"#.to_string()));
        let result2 = extract_json_object(&mut buffer);
        assert_eq!(result2, Some(r#"{"text": "world"}"#.to_string()));
    }

    #[test]
    fn test_extract_json_object_nested() {
        let mut buffer = r#"{"outer": {"inner": "value"}}"#.to_string();
        let result = extract_json_object(&mut buffer);
        assert_eq!(
            result,
            Some(r#"{"outer": {"inner": "value"}}"#.to_string())
        );
    }

    #[test]
    fn test_parse_error_response_auth() {
        let body = r#"{"error":{"code":403,"message":"API key not valid"}}"#;
        let err = parse_error_response(403, body);
        assert!(matches!(err, LLMError::AuthError { .. }));
    }

    #[test]
    fn test_parse_error_response_rate_limit() {
        let body = r#"{"error":{"code":429,"message":"Rate limit exceeded"}}"#;
        let err = parse_error_response(429, body);
        assert!(matches!(err, LLMError::RateLimit { .. }));
    }

    #[test]
    fn test_convert_response_text() {
        let resp = GeminiResponse {
            candidates: vec![GeminiCandidate {
                content: Some(GeminiContent {
                    parts: Some(vec![GeminiPart {
                        text: Some("Hello, world!".to_string()),
                        function_call: None,
                    }]),
                }),
                finish_reason: Some("STOP".to_string()),
            }],
            usage_metadata: Some(GeminiUsageMetadata {
                prompt_token_count: 10,
                candidates_token_count: 5,
            }),
            response_id: Some("test-123".to_string()),
        };

        let response = convert_response(resp, "gemini-2.5-flash");
        assert_eq!(response.id, "test-123");
        assert_eq!(response.model, "gemini-2.5-flash");
        assert!(matches!(response.stop_reason, StopReason::EndTurn));
        assert_eq!(response.usage.input_tokens, 10);
        assert_eq!(response.usage.output_tokens, 5);
    }

    #[test]
    fn test_convert_response_function_call() {
        let resp = GeminiResponse {
            candidates: vec![GeminiCandidate {
                content: Some(GeminiContent {
                    parts: Some(vec![GeminiPart {
                        text: None,
                        function_call: Some(GeminiFunctionCall {
                            name: "get_weather".to_string(),
                            args: Some(json!({"location": "London"})),
                        }),
                    }]),
                }),
                finish_reason: Some("FUNCTION_CALL".to_string()),
            }],
            usage_metadata: None,
            response_id: None,
        };

        let response = convert_response(resp, "gemini-2.5-flash");
        assert!(matches!(response.stop_reason, StopReason::ToolUse));
        assert_eq!(response.content.len(), 1);
        if let ContentBlock::ToolUse(tool) = &response.content[0] {
            assert_eq!(tool.name, "get_weather");
        } else {
            panic!("Expected ToolUse content block");
        }
    }
}
