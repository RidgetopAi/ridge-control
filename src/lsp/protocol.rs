//! JSON-RPC protocol handling for LSP
//!
//! Implements the JSON-RPC 2.0 message format used by LSP,
//! including Content-Length header parsing for stdio transport.
//!
//! Note: These types are used indirectly through LSP tools in llm::ToolExecutor.
//! The dead_code warnings are false positives.

#![allow(dead_code)]

use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicI64, Ordering};

/// JSON-RPC version constant
pub const JSONRPC_VERSION: &str = "2.0";

/// JSON-RPC request message
#[derive(Debug, Clone, Serialize)]
pub struct JsonRpcRequest {
    pub jsonrpc: &'static str,
    pub id: i64,
    pub method: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub params: Option<serde_json::Value>,
}

impl JsonRpcRequest {
    pub fn new(id: i64, method: impl Into<String>, params: Option<serde_json::Value>) -> Self {
        Self {
            jsonrpc: JSONRPC_VERSION,
            id,
            method: method.into(),
            params,
        }
    }

    /// Encode request to LSP wire format with Content-Length header
    pub fn encode(&self) -> Result<Vec<u8>, serde_json::Error> {
        let body = serde_json::to_string(self)?;
        let message = format!("Content-Length: {}\r\n\r\n{}", body.len(), body);
        Ok(message.into_bytes())
    }
}

/// JSON-RPC notification (no id, no response expected)
#[derive(Debug, Clone, Serialize)]
pub struct JsonRpcNotification {
    pub jsonrpc: &'static str,
    pub method: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub params: Option<serde_json::Value>,
}

impl JsonRpcNotification {
    pub fn new(method: impl Into<String>, params: Option<serde_json::Value>) -> Self {
        Self {
            jsonrpc: JSONRPC_VERSION,
            method: method.into(),
            params,
        }
    }

    /// Encode notification to LSP wire format with Content-Length header
    pub fn encode(&self) -> Result<Vec<u8>, serde_json::Error> {
        let body = serde_json::to_string(self)?;
        let message = format!("Content-Length: {}\r\n\r\n{}", body.len(), body);
        Ok(message.into_bytes())
    }
}

/// JSON-RPC response (incoming from server)
#[derive(Debug, Clone, Deserialize)]
pub struct JsonRpcResponse {
    pub jsonrpc: String,
    pub id: Option<i64>,
    #[serde(default)]
    pub result: Option<serde_json::Value>,
    #[serde(default)]
    pub error: Option<JsonRpcError>,
}

/// JSON-RPC response (outgoing to server - for responding to server requests)
#[derive(Debug, Clone, Serialize)]
pub struct JsonRpcResponseOut {
    pub jsonrpc: &'static str,
    pub id: serde_json::Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<JsonRpcErrorOut>,
}

/// JSON-RPC error for outgoing responses
#[derive(Debug, Clone, Serialize)]
pub struct JsonRpcErrorOut {
    pub code: i32,
    pub message: String,
}

impl JsonRpcResponseOut {
    /// Create a success response
    pub fn success(id: serde_json::Value, result: serde_json::Value) -> Self {
        Self {
            jsonrpc: JSONRPC_VERSION,
            id,
            result: Some(result),
            error: None,
        }
    }

    /// Create a success response with null result
    pub fn success_null(id: serde_json::Value) -> Self {
        Self {
            jsonrpc: JSONRPC_VERSION,
            id,
            result: Some(serde_json::Value::Null),
            error: None,
        }
    }

    /// Create an error response
    pub fn error(id: serde_json::Value, code: i32, message: impl Into<String>) -> Self {
        Self {
            jsonrpc: JSONRPC_VERSION,
            id,
            result: None,
            error: Some(JsonRpcErrorOut {
                code,
                message: message.into(),
            }),
        }
    }

    /// Encode response to LSP wire format with Content-Length header
    pub fn encode(&self) -> Result<Vec<u8>, serde_json::Error> {
        let body = serde_json::to_string(self)?;
        let message = format!("Content-Length: {}\r\n\r\n{}", body.len(), body);
        Ok(message.into_bytes())
    }
}

impl JsonRpcResponse {
    /// Check if this response is an error
    pub fn is_error(&self) -> bool {
        self.error.is_some()
    }

    /// Get the result, or error if present
    pub fn into_result(self) -> Result<serde_json::Value, JsonRpcError> {
        if let Some(error) = self.error {
            Err(error)
        } else {
            Ok(self.result.unwrap_or(serde_json::Value::Null))
        }
    }
}

/// JSON-RPC error object
#[derive(Debug, Clone, Deserialize)]
pub struct JsonRpcError {
    pub code: i32,
    pub message: String,
    #[serde(default)]
    pub data: Option<serde_json::Value>,
}

impl std::fmt::Display for JsonRpcError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "JSON-RPC error {}: {}", self.code, self.message)
    }
}

impl std::error::Error for JsonRpcError {}

/// Standard JSON-RPC error codes
pub mod error_codes {
    pub const PARSE_ERROR: i32 = -32700;
    pub const INVALID_REQUEST: i32 = -32600;
    pub const METHOD_NOT_FOUND: i32 = -32601;
    pub const INVALID_PARAMS: i32 = -32602;
    pub const INTERNAL_ERROR: i32 = -32603;

    // LSP-specific error codes
    pub const SERVER_NOT_INITIALIZED: i32 = -32002;
    pub const UNKNOWN_ERROR_CODE: i32 = -32001;
    pub const REQUEST_CANCELLED: i32 = -32800;
    pub const CONTENT_MODIFIED: i32 = -32801;
}

/// Atomic ID generator for JSON-RPC requests
pub struct IdGenerator(AtomicI64);

impl IdGenerator {
    pub fn new() -> Self {
        Self(AtomicI64::new(1))
    }

    pub fn next(&self) -> i64 {
        self.0.fetch_add(1, Ordering::SeqCst)
    }
}

impl Default for IdGenerator {
    fn default() -> Self {
        Self::new()
    }
}

/// Parse Content-Length header from LSP message headers
///
/// LSP uses HTTP-like headers before the JSON body:
/// ```text
/// Content-Length: 123\r\n
/// \r\n
/// {"jsonrpc": "2.0", ...}
/// ```
pub fn parse_content_length(headers: &str) -> Option<usize> {
    for line in headers.lines() {
        let line = line.trim();
        if line.to_lowercase().starts_with("content-length:") {
            return line
                .split(':')
                .nth(1)
                .and_then(|len| len.trim().parse().ok());
        }
    }
    None
}

/// LSP message reader state machine
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReadState {
    /// Reading headers
    Headers,
    /// Reading body of specified length
    Body(usize),
}

/// Buffer for reading LSP messages
pub struct MessageBuffer {
    header_buf: String,
    state: ReadState,
}

impl MessageBuffer {
    pub fn new() -> Self {
        Self {
            header_buf: String::new(),
            state: ReadState::Headers,
        }
    }

    /// Process a line of input, returns content length when headers complete
    pub fn process_header_line(&mut self, line: &str) -> Option<usize> {
        if line.trim().is_empty() {
            // Empty line marks end of headers
            let content_len = parse_content_length(&self.header_buf);
            self.header_buf.clear();
            content_len
        } else {
            self.header_buf.push_str(line);
            self.header_buf.push('\n');
            None
        }
    }

    /// Reset the buffer state
    pub fn reset(&mut self) {
        self.header_buf.clear();
        self.state = ReadState::Headers;
    }
}

impl Default for MessageBuffer {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_content_length() {
        let headers = "Content-Length: 123\r\nContent-Type: application/json\r\n";
        assert_eq!(parse_content_length(headers), Some(123));

        let headers = "content-length: 456\r\n";
        assert_eq!(parse_content_length(headers), Some(456));

        let headers = "X-Custom: value\r\n";
        assert_eq!(parse_content_length(headers), None);
    }

    #[test]
    fn test_request_encode() {
        let req = JsonRpcRequest::new(1, "initialize", Some(serde_json::json!({"foo": "bar"})));
        let encoded = req.encode().unwrap();
        let encoded_str = String::from_utf8(encoded).unwrap();

        assert!(encoded_str.starts_with("Content-Length:"));
        assert!(encoded_str.contains("\r\n\r\n"));
        assert!(encoded_str.contains("\"jsonrpc\":\"2.0\""));
        assert!(encoded_str.contains("\"id\":1"));
        assert!(encoded_str.contains("\"method\":\"initialize\""));
    }

    #[test]
    fn test_notification_encode() {
        let notif = JsonRpcNotification::new("initialized", None);
        let encoded = notif.encode().unwrap();
        let encoded_str = String::from_utf8(encoded).unwrap();

        assert!(encoded_str.starts_with("Content-Length:"));
        assert!(encoded_str.contains("\"method\":\"initialized\""));
        // Notifications should not have an id
        assert!(!encoded_str.contains("\"id\":"));
    }

    #[test]
    fn test_id_generator() {
        let gen = IdGenerator::new();
        assert_eq!(gen.next(), 1);
        assert_eq!(gen.next(), 2);
        assert_eq!(gen.next(), 3);
    }

    #[test]
    fn test_response_into_result() {
        let success = JsonRpcResponse {
            jsonrpc: "2.0".into(),
            id: Some(1),
            result: Some(serde_json::json!({"data": "test"})),
            error: None,
        };
        assert!(success.into_result().is_ok());

        let error = JsonRpcResponse {
            jsonrpc: "2.0".into(),
            id: Some(1),
            result: None,
            error: Some(JsonRpcError {
                code: -32600,
                message: "Invalid Request".into(),
                data: None,
            }),
        };
        assert!(error.into_result().is_err());
    }

    #[test]
    fn test_message_buffer() {
        let mut buf = MessageBuffer::new();

        // Process header lines
        assert_eq!(buf.process_header_line("Content-Length: 50"), None);
        assert_eq!(buf.process_header_line("Content-Type: application/json"), None);

        // Empty line completes headers
        assert_eq!(buf.process_header_line(""), Some(50));
    }
}
